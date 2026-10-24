//! The settings panel and its fields.

use super::*;

#[gpui_kit::test]
fn the_settings_shortcut_and_buttons_open_and_close_the_panel(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(SETTINGS, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(panel_open(&ws, cx), "the shortcut opens it");
    cx.update_window(handle, |_, window, cx| {
        window.press(SETTINGS, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!panel_open(&ws, cx), "and closes it");
    settle_slide(handle, cx);
    ws.read_with(cx, |ws, _| assert!(ws.settings.is_none(), "slid out"));
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    assert!(panel_open(&ws, cx), "the gear opens it");
    cx.update_window(handle, |_, window, cx| window.click("settings-close", cx))
        .unwrap();
    assert!(!panel_open(&ws, cx), "the close button closes it");
}

#[gpui_kit::test]
fn the_panel_opens_on_appearance_and_tabs_switch_sections(cx: &mut TestAppContext) {
    let (handle, _ws) = open(cx);
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("field-theme.mode").is_some());
        assert!(window.try_find("shortcut-new_terminal").is_none());
        window.click("settings-tab-shortcuts", cx);
        window.render_frame(cx);
        assert!(window.try_find("shortcut-new_terminal").is_some());
        assert!(window.try_find("field-theme.mode").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_start_at_login_switch_reads_the_system_and_changes_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let home = tempfile::tempdir().unwrap();
    let entry = with_autostart(&ws, cx, home.path());
    let on = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.autostart.on())
    };
    // Someone added the entry with the system's own tools: the panel finds it when it opens.
    entry.set(true).unwrap();
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    cx.run_until_parked();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-general", cx)
    })
    .unwrap();
    frame(handle, cx);
    assert!(on(&ws, cx), "read from the system");
    // A disabled switch can't take focus, which is what its snapshot shows of it.
    let disabled = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.find("autostart-switch").focused().is_none()
        })
        .unwrap()
    };
    assert!(!disabled(cx));
    // Switched off here: the switch follows at once, the system a moment later (off the UI
    // thread), and the switch is disabled until then.
    cx.update_window(handle, |_, window, cx| {
        window.click("field-general.autostart", cx)
    })
    .unwrap();
    assert!(!on(&ws, cx));
    assert!(entry.is_on(), "not written on the UI thread");
    assert!(disabled(cx), "no second click while it is written");
    cx.update_window(handle, |_, window, cx| {
        window.click("field-general.autostart", cx)
    })
    .unwrap();
    assert!(!on(&ws, cx), "a click meanwhile does nothing");
    cx.run_until_parked();
    assert!(!entry.is_on(), "removed from the system");
    assert!(!on(&ws, cx));
    assert!(!disabled(cx), "and the switch is back");
    cx.update_window(handle, |_, window, cx| {
        window.click("field-general.autostart", cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert!(on(&ws, cx));
    assert!(entry.is_on(), "written to the system");
    // Removed by hand while the panel is shut: the panel opening again shows it off.
    cx.update_window(handle, |_, window, cx| window.click("settings-close", cx))
        .unwrap();
    entry.set(false).unwrap();
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    cx.run_until_parked();
    assert!(!on(&ws, cx));
}

#[gpui_kit::test]
fn a_reading_fired_before_a_change_never_lands_over_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let home = tempfile::tempdir().unwrap();
    let entry = with_autostart(&ws, cx, home.path());
    // The panel's reading is on its way when the switch is clicked.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_autostart(cx);
            ws.set_autostart(true, window, cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    assert!(entry.is_on());
    assert!(
        ws.read_with(cx, |ws, _| ws.autostart.on()),
        "the older reading is dropped"
    );
}

/// Opens a workspace with `entry` and a data folder (None: a second Templum open, which keeps
/// nothing) and lets its startup finish.
fn start_with(
    entry: crate::autostart::Autostart,
    data: Option<&std::path::Path>,
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let data = data.map(std::path::Path::to_path_buf);
    cx.update(|cx| {
        gpui_kit::init(cx);
        bind_keys(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| Workspace::with(Some(quiet()), Some(entry), None, data, window, cx))
        })
        .expect("open window");
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn startup_points_an_entry_that_is_on_at_this_executable(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let on =
        |program: &str| crate::autostart::Autostart::in_dir("linux", home.path(), program, false);
    // Templum was moved: the entry the person turned on names where it was.
    let old = home.path().join("old/templum");
    on(old.to_str().unwrap()).set(true).unwrap();
    start_with(on("/opt/templum"), Some(data.path()), cx);
    let path = home.path().join(".config/autostart/templum.desktop");
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("Exec=\"/opt/templum\""), "{text}");
}

#[gpui_kit::test]
fn a_second_templum_leaves_the_entry_to_the_first(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    let on =
        |program: &str| crate::autostart::Autostart::in_dir("linux", home.path(), program, false);
    let old = home.path().join("old/templum");
    on(old.to_str().unwrap()).set(true).unwrap();
    // No data folder: this one started empty beside the first.
    start_with(on("/opt/templum"), None, cx);
    let path = home.path().join(".config/autostart/templum.desktop");
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("old/templum"), "{text}");
}

#[gpui_kit::test]
fn a_start_at_login_that_cant_be_written_says_why_and_stays_off(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let home = tempfile::tempdir().unwrap();
    // A file where the autostart folder should be.
    std::fs::create_dir_all(home.path().join(".config")).unwrap();
    std::fs::write(home.path().join(".config/autostart"), "").unwrap();
    with_autostart(&ws, cx, home.path());
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-general", cx);
        window.render_frame(cx);
        window.click("field-general.autostart", cx)
    })
    .unwrap();
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(!ws.autostart.on(), "the switch shows what the system has");
        assert!(!ws.autostart.changing);
        let error = ws.autostart.error.as_deref().unwrap();
        assert!(
            error.starts_with("Couldn't turn starting at login on: "),
            "{error}"
        );
    });
}

#[gpui_kit::test]
fn the_blink_switch_edits_the_config_and_its_reset_removes_the_key(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-terminal", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("reset-cursor.blink").is_none(),
            "no reset while at the default"
        );
        window.click("field-cursor.blink", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(ws.read(cx).config.cursor.blink, "the switch turned it on");
        assert!(window.try_find("reset-cursor.blink").is_some());
        window.click("reset-cursor.blink", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(!ws.read(cx).config.cursor.blink, "back to the default");
    })
    .unwrap();
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap_or_default();
    assert!(!text.contains("blink"), "{text}");
}

#[gpui_kit::test]
fn the_integration_and_notification_switches_turn_them_off_and_back(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    let on = |ws: &Workspace| {
        (
            ws.config.shell.integration,
            ws.config.terminal.notifications,
            ws.config.usage.claude_limits,
            ws.config.connections.enabled,
            ws.config.connections.sound,
            ws.config.canvas.focus_follows_pointer,
            ws.config.terminal.copy_on_select,
        )
    };
    for (tab, field, reset, line) in [
        (
            "settings-tab-shell",
            "field-shell.integration",
            "reset-shell.integration",
            "integration = false",
        ),
        (
            "settings-tab-terminal",
            "field-terminal.notifications",
            "reset-terminal.notifications",
            "notifications = false",
        ),
        (
            "settings-tab-usage",
            "field-usage.claude_limits",
            "reset-usage.claude_limits",
            "claude_limits = false",
        ),
        (
            "settings-tab-connections",
            "field-connections.enabled",
            "reset-connections.enabled",
            "enabled = false",
        ),
        (
            "settings-tab-connections",
            "field-connections.sound",
            "reset-connections.sound",
            "sound = false",
        ),
        (
            "settings-tab-canvas",
            "field-canvas.focus_follows_pointer",
            "reset-canvas.focus_follows_pointer",
            "focus_follows_pointer = false",
        ),
        (
            "settings-tab-terminal",
            "field-terminal.copy_on_select",
            "reset-terminal.copy_on_select",
            "copy_on_select = false",
        ),
    ] {
        cx.update_window(handle, |_, window, cx| {
            window.click(tab, cx);
            window.render_frame(cx);
            assert!(window.try_find(reset).is_none());
            // Under three rows of tabs, a switch may be below the fold of a small window.
            window.scroll(
                "settings-scroll",
                ScrollDelta::Pixels(point(px(0.), px(-5000.))),
                cx,
            );
            window.render_frame(cx);
            window.click(field, cx);
            window.render_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
        ws.update(cx, |ws, _| ws.save());
        let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
        assert!(text.contains(line), "{text}");
        cx.update_window(handle, |_, window, cx| {
            window.click(reset, cx);
            window.render_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
    }
    assert_eq!(
        ws.read_with(cx, |ws, _| on(ws)),
        (true, true, true, true, true, true, true),
        "back on"
    );
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
    assert!(
        !text.contains("integration")
            && !text.contains("notifications")
            && !text.contains("claude_limits")
            && !text.contains("enabled")
            && !text.contains("sound")
            && !text.contains("focus_follows_pointer")
            && !text.contains("copy_on_select"),
        "{text}"
    );
}

#[gpui_kit::test]
fn connections_give_the_servers_command_to_copy_and_ask_before_gemini(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-connections", cx);
        window.render_frame(cx);
        assert!(window.try_find("connections-command").is_some());
        assert!(
            window
                .try_find("field-connections.question_timeout")
                .is_some(),
            "Questions wait"
        );
        window.click("connections-copy", cx);
        // Below the questions' rows, maybe below the fold of a small window.
        window.scroll(
            "settings-scroll",
            ScrollDelta::Pixels(point(px(0.), px(-5000.))),
            cx,
        );
        window.render_frame(cx);
        window.click("gemini-setup", cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "it asks first");
        window.within("dialog").click("cancel", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let bin = data.path().join("bin").join(if cfg!(windows) {
        "templum.exe"
    } else {
        "templum"
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|c| c.text()),
        Some(format!("{} mcp", bin.display()))
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.gemini_job.is_none(), "Cancel runs nothing")
    });
}

#[gpui_kit::test]
fn the_shell_list_sets_the_program_and_its_arguments(cx: &mut TestAppContext) {
    use templum_terminal::ShellOption;
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    // Two shells "found" on this computer.
    ws.update(cx, |ws, _| {
        ws.shells = vec![
            ShellOption {
                label: "Git Bash".into(),
                program: "C:\\Git\\bin\\bash.exe".into(),
                args: vec!["--login".into(), "-i".into()],
            },
            ShellOption {
                label: "PowerShell 7".into(),
                program: "pwsh".into(),
                args: vec!["-NoLogo".into()],
            },
        ];
        ws.default_shell = "Git Bash".into();
    });
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-shell", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("reset-shell.shell").is_none(),
            "the default"
        );
        window
            .within("field-shell.shell")
            .click("menu-shell.shell", cx);
        window.render_frame(cx);
        // Default (Git Bash), Git Bash, PowerShell 7.
        window.within("popup-menu").click(2usize, cx);
        window.render_frame(cx);
        window.render_frame(cx);
        let shell = ws.read(cx).config.shell.clone();
        assert_eq!(shell.program.as_deref(), Some("pwsh"));
        assert_eq!(shell.args, ["-NoLogo"]);
        assert!(window.try_find("reset-shell.shell").is_some());
    })
    .unwrap();
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
    assert!(
        text.contains("program = \"pwsh\"") && text.contains("-NoLogo"),
        "{text}"
    );
    cx.update_window(handle, |_, window, cx| {
        window.click("reset-shell.shell", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        let shell = ws.read(cx).config.shell.clone();
        assert_eq!(
            (shell.program, shell.args.len()),
            (None, 0),
            "back to the default"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn esc_closes_the_panel_only_from_inside_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(SETTINGS, cx);
        window.render_frame(cx);
        window.press("escape", cx);
    })
    .unwrap();
    assert!(!panel_open(&ws, cx), "Esc in the panel closes it");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(SETTINGS, cx);
    })
    .unwrap();
    settle_slide(handle, cx);
    // Click the terminal's left part, clear of the panel on the right.
    cx.update_window(handle, |_, window, cx| {
        let (o, r) = {
            let ws = ws.read(cx);
            let rect = ws.space.canvas.get(id).unwrap().rect;
            (ws.origin, ws.space.canvas.camera.rect_to_screen(rect))
        };
        let at = (
            f32::from(o.x) + r.x + 20.0,
            f32::from(o.y) + r.y + templum_canvas::chrome::TITLE_H + 20.0,
        );
        click(window, cx, at, 1);
        window.render_frame(cx);
        window.press("escape", cx);
        let focused = ws.read(cx).focus_handle.is_focused(window);
        assert!(focused, "keys go to the terminal");
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.focused, Some(id)));
    assert!(panel_open(&ws, cx), "Esc went to the terminal");
}

#[gpui_kit::test]
fn closing_the_panel_saves_at_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(SETTINGS, cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.update(cx, |ws, cx| {
        ws.edit(
            Edit::set("cursor", "blink", true),
            |c| c.cursor.blink = true,
            cx,
        )
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(SETTINGS, cx);
    })
    .unwrap();
    let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(text.contains("blink = true"), "{text}");
}

#[gpui_kit::test]
fn a_number_applies_on_enter_or_a_step_not_while_typing(cx: &mut TestAppContext) {
    use crate::settings::{NumberState, SCROLLBACK};
    use gpui_kit::component::input::{InputEvent, NumberInputEvent, StepAction};
    let (handle, ws) = open(cx);
    let state = cx
        .update_window(handle, |_, window, cx| {
            let ws = ws.clone();
            cx.new(|cx| NumberState::new(ws, &SCROLLBACK, 10_000.0, window, cx))
        })
        .unwrap();
    let scrollback =
        |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.config.terminal.scrollback);
    // Typing "20000" passes through "2": applied, it would trim every terminal's history.
    cx.update_window(handle, |_, window, cx| {
        let input = state.read(cx).input.clone();
        input.update(cx, |input, cx| input.set_value("2", window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(scrollback(cx), 10_000, "not while typing");
    cx.update_window(handle, |_, window, cx| {
        let input = state.read(cx).input.clone();
        input.update(cx, |input, cx| {
            input.set_value("20000", window, cx);
            cx.emit(InputEvent::PressEnter {
                secondary: false,
                shift: false,
            });
        });
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(scrollback(cx), 20_000, "Enter applies it");
    cx.update_window(handle, |_, window, cx| {
        let input = state.read(cx).input.clone();
        input.update(cx, |_, cx| {
            cx.emit(NumberInputEvent::Step(StepAction::Decrement))
        });
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(scrollback(cx), 19_000, "a step applies at once");
}

#[gpui_kit::test]
fn a_reset_or_another_folder_choice_drops_what_was_typed(cx: &mut TestAppContext) {
    use crate::settings::{
        args_error, args_text, folder_hint, folder_text, set_args, set_folder, set_folder_kind,
    };
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    ws.update(cx, |ws, cx| set_args(ws, "--login \"a b", cx));
    frame(handle, cx);
    // What the field's "reset to default" does.
    ws.update(cx, |ws, cx| {
        ws.edit(Edit::remove("shell", "args"), |c| c.shell.args.clear(), cx)
    });
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(args_text(ws), "");
        assert_eq!(args_error(ws), None);
    });
    // "~" typed, then Home chosen, then "A folder…" again (it starts at the home folder).
    let steps: [fn(&mut Workspace, &mut Context<Workspace>); 4] = [
        |ws, cx| set_folder_kind(ws, "folder", cx),
        |ws, cx| set_folder(ws, "~", cx),
        |ws, cx| set_folder_kind(ws, "home", cx),
        |ws, cx| set_folder_kind(ws, "folder", cx),
    ];
    for step in steps {
        ws.update(cx, step);
        frame(handle, cx);
    }
    let home = std::env::home_dir().unwrap().display().to_string();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            folder_text(ws),
            home,
            "the folder in effect, not the old \"~\""
        );
        assert_eq!(folder_hint(ws), None);
    });
}

#[gpui_kit::test]
fn the_default_font_removes_the_family(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    ws.update(cx, |ws, cx| {
        crate::settings::set_family(ws, "Some Mono", cx)
    });
    frame(handle, cx);
    ws.update(cx, |ws, _| ws.save());
    let path = dir.path().join("config.toml");
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("family = \"Some Mono\"")
    );
    ws.update(cx, |ws, cx| crate::settings::set_family(ws, "", cx));
    frame(handle, cx);
    ws.update(cx, |ws, _| {
        assert_eq!(ws.config.font.family, None);
        ws.save();
    });
    assert!(!std::fs::read_to_string(&path).unwrap().contains("family"));
}

#[gpui_kit::test]
fn an_unclosed_quote_waits_for_the_closing_one(cx: &mut TestAppContext) {
    use crate::settings::set_args;
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    ws.update(cx, |ws, cx| set_args(ws, "--login \"a b", cx));
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        assert!(ws.config.shell.args.is_empty(), "not applied");
        assert_eq!(
            crate::settings::args_text(ws),
            "--login \"a b",
            "kept as typed"
        );
        assert_eq!(crate::settings::args_error(ws), Some("Unclosed quote"));
    });
    ws.update(cx, |ws, cx| set_args(ws, "--login \"a b\"", cx));
    frame(handle, cx);
    ws.update(cx, |ws, _| {
        assert_eq!(ws.config.shell.args, ["--login", "a b"]);
        assert_eq!(crate::settings::args_error(ws), None);
        ws.save();
    });
    let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(text.contains("args = [\"--login\", \"a b\"]"), "{text}");
}

#[gpui_kit::test]
fn a_bare_tilde_waits_for_the_rest_of_the_path(cx: &mut TestAppContext) {
    use crate::config::WorkingDirectory;
    use crate::settings::{folder_hint, folder_text, set_folder, set_folder_kind};
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    let target = dir.path().join("work");
    ws.update(cx, |ws, cx| set_folder_kind(ws, "folder", cx));
    frame(handle, cx);
    ws.update(cx, |ws, cx| set_folder(ws, "~", cx));
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        assert!(
            matches!(ws.config.shell.working_directory, WorkingDirectory::Path(_)),
            "\"~\" alone doesn't switch to Home"
        );
        assert_eq!(folder_text(ws), "~", "kept as typed");
        assert!(folder_hint(ws).is_some(), "and the field says why");
    });
    let typed = target.display().to_string();
    ws.update(cx, |ws, cx| set_folder(ws, &typed, cx));
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.config.shell.working_directory,
            WorkingDirectory::Path(target.clone())
        );
        assert_eq!(folder_text(ws), typed);
        assert_eq!(folder_hint(ws), None);
    });
    // Written now, while the temporary folder still exists.
    ws.update(cx, |ws, _| ws.save());
}

#[gpui_kit::test]
fn start_folder_and_program_choices_write_the_file(cx: &mut TestAppContext) {
    use crate::settings::{set_folder_kind, set_program};
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    let path = dir.path().join("config.toml");
    let mut check = |change: &dyn Fn(&mut Workspace, &mut Context<Workspace>), want: &str| {
        ws.update(cx, |ws, cx| change(ws, cx));
        frame(handle, cx);
        ws.update(cx, |ws, _| ws.save());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(want), "want {want:?} in {text}");
    };
    check(
        &|ws, cx| set_folder_kind(ws, "home", cx),
        "working_directory = \"~\"",
    );
    check(
        &|ws, cx| set_folder_kind(ws, "inherit", cx),
        "working_directory = \"inherit\"",
    );
    check(&|ws, cx| set_program(ws, "fish", cx), "program = \"fish\"");
    ws.update(cx, |ws, cx| set_program(ws, "", cx));
    frame(handle, cx);
    ws.update(cx, |ws, _| ws.save());
    assert!(!std::fs::read_to_string(&path).unwrap().contains("program"));
}

#[gpui_kit::test]
fn the_program_is_saved_trimmed_but_typing_keeps_its_spaces(cx: &mut TestAppContext) {
    use crate::settings::{program_text, set_program};
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    // Halfway through typing "C:\Program Files\…".
    ws.update(cx, |ws, cx| set_program(ws, "C:\\Program ", cx));
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.config.shell.program.as_deref(), Some("C:\\Program"));
        assert_eq!(
            program_text(ws),
            "C:\\Program ",
            "the space stays in the field"
        );
    });
    ws.update(cx, |ws, cx| set_program(ws, " fish ", cx));
    frame(handle, cx);
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(text.contains("program = \"fish\""), "{text}");
    ws.update(cx, |ws, cx| set_program(ws, "  ", cx));
    frame(handle, cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.config.shell.program, None));
}

#[gpui_kit::test]
fn the_tools_section_shows_gits_version_and_sets_the_program(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.toggle_settings(window, cx));
    })
    .unwrap();
    settle(handle, cx);
    // The panel slides in in real time: clicks land once it is at rest.
    settle_slide(handle, cx);
    ws.read_with(cx, |ws, _| {
        let version = ws.git_version.clone().unwrap().unwrap();
        assert!(version.starts_with("git version "), "{version}");
    });
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-tools", cx);
        window.render_frame(cx);
        assert!(window.try_find("field-git.program").is_some());
        assert!(
            window.try_find("reset-git.program").is_none(),
            "the default"
        );
    })
    .unwrap();
    // What the field calls on each change, as the shell program's tests do.
    ws.update(cx, |ws, cx| {
        crate::settings::set_git_program(ws, "/opt/git", cx)
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        assert_eq!(ws.read(cx).config.git.program, "/opt/git");
        assert!(window.try_find("reset-git.program").is_some());
    })
    .unwrap();
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
    assert!(
        text.contains("[git]") && text.contains("program = \"/opt/git\""),
        "{text}"
    );
}

// Re-graded from minor: a space can be typed in Settings ▸ Tools' Git field.
#[gpui_kit::test]
fn the_git_program_field_keeps_a_typed_space(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    ws.update(cx, |ws, cx| {
        crate::settings::set_git_program(ws, "C:\\Program ", cx)
    });
    ws.update(cx, |ws, _| {
        let git = &ws.pending.as_ref().unwrap_or(&ws.config).git;
        assert_eq!(
            git.program, "C:\\Program ",
            "the field shows what was typed"
        );
        assert_eq!(git.program(), "C:\\Program", "what runs is trimmed");
    });
    ws.update(cx, |ws, cx| crate::settings::set_git_program(ws, "  ", cx));
    ws.update(cx, |ws, _| {
        assert_eq!(
            ws.pending.as_ref().unwrap_or(&ws.config).git.program(),
            "git"
        )
    });
}

/// Opens Settings ▸ Agents, scrolled to its end: the accounts.
fn open_accounts(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-agents", cx);
        window.render_frame(cx);
        window.scroll(
            "settings-scroll",
            ScrollDelta::Pixels(point(px(0.), px(-8000.))),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

/// Picks the option at `index` of the dropdown `id`.
fn pick(handle: AnyWindowHandle, cx: &mut TestAppContext, id: &'static str, index: usize) {
    cx.update_window(handle, |_, window, cx| {
        window.click(id, cx);
        window.render_frame(cx);
        window.within("popup-menu").click(index, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// The Add an account dialog with Claude Code chosen (whatever else is installed here) and
/// `name` typed into its field.
fn add_account(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    name: &str,
) {
    cx.update_window(handle, |_, window, cx| {
        window.click("add-account", cx);
        window.render_frame(cx);
        assert!(gpui_kit::component::WindowExt::has_active_dialog(
            window, cx
        ));
    })
    .unwrap();
    // The dialog slides in for a quarter of a second: a click while it moves can miss.
    std::thread::sleep(std::time::Duration::from_millis(300));
    frame(handle, cx);
    let claude = ws.read_with(cx, |ws, _| {
        crate::settings::accounts::agent_choices(&ws.agents)
            .iter()
            .position(|(id, _)| id == "claude")
            .unwrap()
    });
    pick(handle, cx, "account-agent", claude);
    cx.update_window(handle, |_, window, cx| {
        window.click("account-name", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.simulate_input(handle, name);
    frame(handle, cx);
}

/// The dialog's Create (Save, when editing).
fn create(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        window.click("account-save", cx);
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Clicks `id` and draws the result.
fn press(handle: AnyWindowHandle, cx: &mut TestAppContext, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Opens the edit dialog of the account `id`, once it has slid in.
fn edit_account(handle: AnyWindowHandle, cx: &mut TestAppContext, id: &'static str) {
    press(handle, cx, id);
    std::thread::sleep(std::time::Duration::from_millis(300));
    frame(handle, cx);
}

/// The temporary folders an account test points the commands and the agents' own folders at.
fn account_dirs() -> (tempfile::TempDir, tempfile::TempDir) {
    let (own, commands) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    crate::agents::OWN_FOLDERS.set(Some(own.path().to_path_buf()));
    crate::agents::COMMANDS.set(Some(commands.path().to_path_buf()));
    (own, commands)
}

#[gpui_kit::test]
fn creating_an_account_by_name_saves_it_and_logs_in(cx: &mut TestAppContext) {
    let (own, commands) = account_dirs();
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_accounts(handle, cx);
    add_account(handle, &ws, cx, "claudinho");
    create(handle, cx);
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(config.path().join(crate::config::FILE)).unwrap();
    let account = &crate::config::parse(&text).unwrap().agents["claudinho"];
    assert_eq!(account.of.as_deref(), Some("claude"));
    assert_eq!(account.name, None, "{text}");
    // Same as Claude Code: every item it can share, shared with it.
    let kind = templum_usage::accounts::kind("claude").unwrap();
    let files = templum_usage::accounts::files_share();
    let items = kind
        .items
        .iter()
        .filter(|i| files || i.paths[0].ends_with('/'))
        .count();
    let share = account.share.as_ref().unwrap();
    assert_eq!(share.len(), items, "{text}");
    assert!(share.values().all(|to| to == "claude"), "{text}");
    #[cfg(unix)]
    assert_eq!(
        std::fs::read_link(data.path().join("accounts/claudinho/skills")).unwrap(),
        own.path().join("claude").join("skills")
    );
    let _ = own;
    assert!(templum_usage::accounts::command::is_own(
        &commands.path().join("claudinho"),
        "claudinho",
        data.path()
    ));
    cx.update_window(handle, |_, window, cx| {
        assert!(!gpui_kit::component::WindowExt::has_active_dialog(
            window, cx
        ));
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let logging_in = ws
            .space
            .terminals()
            .any(|(_, p)| p.account.as_deref() == Some("claudinho"));
        assert!(logging_in, "a block of the account, to log in");
    });
}

#[gpui_kit::test]
fn a_broken_name_keeps_create_from_saving(cx: &mut TestAppContext) {
    let (_own, commands) = account_dirs();
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_accounts(handle, cx);
    for broken in ["Claudinho", "claude", "cd"] {
        add_account(handle, &ws, cx, broken);
        create(handle, cx);
        cx.update_window(handle, |_, window, cx| {
            assert!(
                gpui_kit::component::WindowExt::has_active_dialog(window, cx),
                "{broken}: the dialog stays"
            );
            assert!(window.try_find("account-name-note").is_some());
            // The next name starts a new dialog.
            gpui_kit::component::WindowExt::close_all_dialogs(window, cx);
            window.render_frame(cx);
        })
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        frame(handle, cx);
    }
    ws.read_with(cx, |ws, _| {
        let agents = &ws.pending.as_ref().unwrap_or(&ws.config).agents;
        assert!(agents.is_empty(), "{:?}", agents.keys().collect::<Vec<_>>());
    });
    assert_eq!(std::fs::read_dir(commands.path()).unwrap().count(), 0);
}

#[gpui_kit::test]
fn fresh_shares_nothing_and_custom_shows_the_items(cx: &mut TestAppContext) {
    let (_own, _commands) = account_dirs();
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_accounts(handle, cx);
    add_account(handle, &ws, cx, "claudio");
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find("account-share-skills").is_none(),
            "Same: no rows"
        );
    })
    .unwrap();
    press(handle, cx, "account-setup-custom");
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find("account-share-skills").is_some(),
            "Custom: a row per item"
        );
    })
    .unwrap();
    press(handle, cx, "account-setup-fresh");
    create(handle, cx);
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(config.path().join(crate::config::FILE)).unwrap();
    let agents = crate::config::parse(&text).unwrap().agents;
    assert_eq!(agents["claudio"].share, None, "{text}");
}

#[gpui_kit::test]
fn editing_an_older_account_keeps_its_name_and_folder(cx: &mut TestAppContext) {
    let (_own, _commands) = account_dirs();
    let (config, data, chosen) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    // A real item of its own in the way of the link Same makes.
    std::fs::create_dir_all(chosen.path().join("skills").join("mine")).unwrap();
    let file = format!(
        "[agents.claude2]\nof = \"claude\"\nname = \"Claude Code 2\"\nfolder = '{}'\n",
        chosen.path().display()
    );
    std::fs::write(config.path().join(crate::config::FILE), file).unwrap();
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_accounts(handle, cx);
    edit_account(handle, cx, "edit-account-claude2");
    press(handle, cx, "account-setup-same");
    create(handle, cx);
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(config.path().join(crate::config::FILE)).unwrap();
    let account = &crate::config::parse(&text).unwrap().agents["claude2"];
    assert_eq!(account.name.as_deref(), Some("Claude Code 2"), "{text}");
    // The same folder, whether the home is written `~` or not.
    let folder = crate::launcher::expand(account.folder.as_deref().unwrap());
    assert_eq!(folder, chosen.path(), "{text}");
    assert!(account.share.as_ref().is_some_and(|s| !s.is_empty()));
    // Its own files in a folder the person chose are never moved.
    assert!(chosen.path().join("skills").join("mine").is_dir());
    assert!(!chosen.path().join("skills.own").exists());
    // And Fresh takes the sharing back out.
    edit_account(handle, cx, "edit-account-claude2");
    press(handle, cx, "account-setup-fresh");
    create(handle, cx);
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(config.path().join(crate::config::FILE)).unwrap();
    let account = &crate::config::parse(&text).unwrap().agents["claude2"];
    assert_eq!(account.share, None, "{text}");
    assert_eq!(account.name.as_deref(), Some("Claude Code 2"), "{text}");
}

#[gpui_kit::test]
fn the_account_dialog_sets_its_shell_without_wsl(cx: &mut TestAppContext) {
    use templum_terminal::ShellOption;
    let (_own, _commands) = account_dirs();
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    // claude3 was written by hand, for WSL: it opens the default, and Save keeps what is written.
    std::fs::write(
        config.path().join(crate::config::FILE),
        "[agents.claude2]\nof = \"claude\"\n[agents.claude3]\nof = \"claude\"\nshell = \"WSL\"\n",
    )
    .unwrap();
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_accounts(handle, cx);
    // Opening Settings read this computer's shells: the made-up ones come after.
    let (program, args) = quiet();
    ws.update(cx, |ws, _| {
        ws.shells = vec![
            ShellOption {
                label: "WSL".into(),
                program: "C:/Windows/System32/wsl.exe".into(),
                args: vec![],
            },
            ShellOption {
                label: "Quiet".into(),
                program,
                args,
            },
        ];
    });
    edit_account(handle, cx, "edit-account-claude2");
    pick(handle, cx, "menu-account-shell", 1); // Default, Quiet: WSL is left out
    create(handle, cx);
    edit_account(handle, cx, "edit-account-claude3");
    create(handle, cx);
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(config.path().join(crate::config::FILE)).unwrap();
    let agents = crate::config::parse(&text).unwrap().agents;
    assert_eq!(agents["claude2"].shell.as_deref(), Some("Quiet"), "{text}");
    assert_eq!(agents["claude3"].shell.as_deref(), Some("WSL"), "{text}");
}

/// A folder set up by hand opens Edit on Custom, with its links read: one to the agent's own
/// item is that choice, one elsewhere is the person's and has no dropdown.
#[cfg(unix)]
#[gpui_kit::test]
fn a_folder_set_up_by_hand_opens_edit_on_custom(cx: &mut TestAppContext) {
    let (own, _commands) = account_dirs();
    let skills = own.path().join("claude").join("skills");
    std::fs::create_dir_all(&skills).unwrap();
    let mine = tempfile::tempdir().unwrap();
    let dotfiles = mine.path().join("dotfiles").join("agents");
    std::fs::create_dir_all(&dotfiles).unwrap();
    let folder = mine.path().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    std::os::unix::fs::symlink(&skills, folder.join("skills")).unwrap();
    std::os::unix::fs::symlink(&dotfiles, folder.join("agents")).unwrap();
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let file = format!(
        "[agents.claude2]\nof = \"claude\"\nfolder = '{}'\n",
        folder.display()
    );
    std::fs::write(config.path().join(crate::config::FILE), file).unwrap();
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_accounts(handle, cx);
    edit_account(handle, cx, "edit-account-claude2");
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("account-share-skills").is_some());
        assert!(
            window.try_find("account-share-agents").is_none(),
            "the person's link: a line in place of its dropdown"
        );
    })
    .unwrap();
    create(handle, cx);
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(config.path().join(crate::config::FILE)).unwrap();
    let agents = crate::config::parse(&text).unwrap().agents;
    assert_eq!(
        agents["claude2"].share.as_ref().unwrap()["skills"],
        "claude",
        "{text}"
    );
    assert_eq!(std::fs::read_link(folder.join("skills")).unwrap(), skills);
    assert_eq!(std::fs::read_link(folder.join("agents")).unwrap(), dotfiles);
}

#[gpui_kit::test]
fn the_agents_rows_leave_accounts_out_and_reset_all_leaves_them(cx: &mut TestAppContext) {
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(
        config.path().join(crate::config::FILE),
        format!(
            "[agents.claude]\ncommand = \"claude --model opus\"\n{}",
            super::agents::CLAUDE2
        ),
    )
    .unwrap();
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_accounts(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(
            window.try_find("field-agents.claude2").is_none(),
            "not among the agents"
        );
        assert!(
            window.try_find("field-accounts.claude2").is_some(),
            "under Accounts"
        );
        window.scroll(
            "settings-scroll",
            ScrollDelta::Pixels(point(px(0.), px(8000.))),
            cx,
        );
        window.render_frame(cx);
        window.click("settings-reset-section", cx);
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let agents = &ws.pending.as_ref().unwrap_or(&ws.config).agents;
        assert!(
            !agents.contains_key("claude"),
            "Claude Code back to Templum's"
        );
        assert!(agents.contains_key("claude2"), "the account stays");
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_folder_change_waits_for_the_blocks_and_keeps_the_old_one(cx: &mut TestAppContext) {
    let (data, elsewhere) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    super::agents::with_agents(
        &ws,
        cx,
        "[agents.claude2]\nof = \"claude\"\n[agents.claude3]\nof = \"claude\"\n",
    );
    let folder = tempfile::tempdir().unwrap();
    let block = super::agents::launch(handle, &ws, cx, "claude2", folder.path());
    let old = data.path().join("accounts").join("claude2");
    let new = elsewhere.path().join("w");
    let moved = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                let entry = ws
                    .agents
                    .iter()
                    .find(|a| a.id == "claude2")
                    .unwrap()
                    .clone();
                let draft = crate::agents::AccountDraft {
                    folder: Some(new.display().to_string()),
                    ..crate::agents::AccountDraft::of(&entry)
                };
                ws.save_account("claude2", &draft, window, cx)
            })
        })
        .unwrap()
    };
    let refused = moved(&ws, cx).unwrap_err();
    assert_eq!(
        refused,
        "Claude Code (claude2) has 1 block: close them first to change its folder"
    );
    ws.update(cx, |ws, cx| {
        ws.space.blocks.remove(&block);
        ws.space.canvas.remove(block);
        cx.notify();
    });
    assert_eq!(moved(&ws, cx), Ok("claude2".to_string()));
    assert!(new.is_dir(), "its choices applied in the new folder");
    assert!(old.is_dir(), "the old one, with its login, stays");
}

#[cfg(unix)]
#[gpui_kit::test]
fn log_in_opens_a_block_of_the_account(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let script = "echo \"dir=$CLAUDE_CONFIG_DIR;\"; exec cat";
    let command = ("sh".to_string(), vec!["-c".to_string(), script.to_string()]);
    let (handle, ws) = open_as(cx, Some(command), None, Some(data.path().to_path_buf()));
    super::agents::with_agents(&ws, cx, super::agents::CLAUDE2);
    let id = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.log_in("claude2", window, cx))
        })
        .unwrap()
        .expect("a block");
    let account = data.path().join("accounts").join("claude2");
    super::bus::screen_with(&ws, cx, id, &format!("dir={};", account.display()));
    // The test's shell echoes it: no agent ever runs here.
    super::bus::screen_with(&ws, cx, id, "claude auth login");
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.terminal(id).unwrap().account.as_deref(),
            Some("claude2")
        );
    });
}

/// Deletes the account `id`, as its dialog's Delete does once asked.
fn delete(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: &str) -> Result<(), String> {
    ws.update(cx, |ws, cx| ws.delete_account(id, cx))
}

#[gpui_kit::test]
fn deleting_an_account_removes_its_folder_only(cx: &mut TestAppContext) {
    let own = tempfile::tempdir().unwrap();
    crate::agents::OWN_FOLDERS.set(Some(own.path().to_path_buf()));
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    super::agents::with_agents(
        &ws,
        cx,
        "[agents.claude2]\nof = \"claude\"\nshare = { skills = \"claude\" }\n",
    );
    let folder = tempfile::tempdir().unwrap();
    let block = super::agents::launch(handle, &ws, cx, "claude2", folder.path());
    ws.update(cx, |ws, cx| {
        ws.space.blocks.remove(&block);
        ws.space.canvas.remove(block);
        cx.notify();
    });
    let keep = own.path().join("claude").join("skills").join("keep");
    std::fs::write(&keep, "mine").unwrap();
    let account = data.path().join("accounts").join("claude2");
    assert!(account.join("skills").is_symlink());
    assert_eq!(delete(&ws, cx, "claude2"), Ok(()));
    assert!(!account.exists(), "its folder goes");
    assert_eq!(
        std::fs::read(&keep).unwrap(),
        b"mine",
        "what it shared stays"
    );
    ws.read_with(cx, |ws, _| {
        let agents = &ws.pending.as_ref().unwrap_or(&ws.config).agents;
        assert!(!agents.contains_key("claude2"));
    });
}

#[gpui_kit::test]
fn an_account_in_use_or_shared_isnt_deleted(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    super::agents::with_agents(
        &ws,
        cx,
        concat!(
            "[agents.claude2]\nof = \"claude\"\nname = \"Claude Code 2\"\n",
            "[agents.claude4]\nof = \"claude\"\nname = \"Claude Code 4\"\n",
            "share = { skills = \"claude2\" }\n",
        ),
    );
    let folder = tempfile::tempdir().unwrap();
    let running = super::agents::launch(handle, &ws, cx, "claude2", folder.path());
    // Its agent quit there: only its shell, still the account's.
    let shell = super::agents::launch(handle, &ws, cx, "claude2", folder.path());
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(shell).unwrap().seen.agent = None
    });
    assert_eq!(
        delete(&ws, cx, "claude2"),
        Err("Claude Code 2 has 2 blocks: close them first".to_string())
    );
    ws.update(cx, |ws, cx| {
        for block in [running, shell] {
            ws.space.blocks.remove(&block);
            ws.space.canvas.remove(block);
        }
        cx.notify();
    });
    assert_eq!(
        delete(&ws, cx, "claude2"),
        Err("Claude Code 4 shares Claude Code 2's skills: change it first".to_string())
    );
    assert!(
        data.path().join("accounts").join("claude2").is_dir(),
        "nothing deleted"
    );
}

#[gpui_kit::test]
fn a_chosen_folder_stays(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let chosen = tempfile::tempdir().unwrap();
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    super::agents::with_agents(
        &ws,
        cx,
        &format!(
            "[agents.claude2]\nof = \"claude\"\nfolder = '{}'\n",
            chosen.path().display()
        ),
    );
    assert_eq!(delete(&ws, cx, "claude2"), Ok(()));
    assert!(
        chosen.path().is_dir(),
        "a folder the person chose is theirs"
    );
}

#[gpui_kit::test]
fn an_account_folder_in_templums_folder_for_accounts_is_refused(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let claude2 = data.path().join("accounts").join("claude2");
    std::fs::create_dir_all(&claude2).unwrap();
    std::fs::write(claude2.join(".credentials.json"), "claude3's login").unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    // Also through a folder that isn't there: the OS makes it, then goes back up.
    for folder in [
        claude2.clone(),
        data.path().join("nothing/../accounts/claude2"),
    ] {
        // Written before Templum refused it: test agents are read without the data folder.
        super::agents::with_agents(
            &ws,
            cx,
            &format!(
                "[agents.claude2]\nof = \"claude\"\n[agents.claude3]\nof = \"claude\"\nname = \"Claude Code 3\"\nfolder = '{}'\n",
                folder.display()
            ),
        );
        assert_eq!(
            delete(&ws, cx, "claude2"),
            Err(
                "Claude Code 3's folder is in Claude Code (claude2)'s: change it first".to_string()
            )
        );
        assert_eq!(
            std::fs::read(claude2.join(".credentials.json")).unwrap(),
            b"claude3's login"
        );
        let saved = cx
            .update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| {
                    let entry = ws
                        .agents
                        .iter()
                        .find(|a| a.id == "claude3")
                        .unwrap()
                        .clone();
                    let draft = crate::agents::AccountDraft::of(&entry);
                    ws.save_account("claude3", &draft, window, cx)
                })
            })
            .unwrap();
        assert!(
            saved
                .unwrap_err()
                .contains("is Templum's folder for accounts: leave Folder empty"),
            "{}",
            folder.display()
        );
    }
}

/// A folder the person chose keeps its own files, whatever it shares: ~/Documents with a
/// `backups/` (a name of Claude Code's), Claude Code's own folder for a Codex account, an empty one.
#[cfg(unix)]
#[gpui_kit::test]
fn a_chosen_folders_own_files_are_never_moved(cx: &mut TestAppContext) {
    let own = tempfile::tempdir().unwrap();
    crate::agents::OWN_FOLDERS.set(Some(own.path().to_path_buf()));
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let mine = tempfile::tempdir().unwrap();
    let documents = mine.path().join("Documents");
    std::fs::create_dir_all(documents.join("backups")).unwrap();
    std::fs::create_dir_all(documents.join("projects").join("app")).unwrap();
    let claudes = own.path().join("claude");
    std::fs::create_dir_all(claudes.join("skills").join("mine")).unwrap();
    let empty = mine.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let mut save = |id: &str, of: &str, folder: &std::path::Path, keys: &[&str]| {
        let draft = crate::agents::AccountDraft {
            of: of.to_string(),
            show: true,
            folder: Some(folder.display().to_string()),
            share: keys
                .iter()
                .map(|k| (k.to_string(), of.to_string()))
                .collect(),
            ..Default::default()
        };
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.save_account(id, &draft, window, cx))
        })
        .unwrap()
        .unwrap()
    };
    save("claude2", "claude", &documents, &["sessions", "skills"]);
    assert!(documents.join("projects").join("app").is_dir());
    assert!(!is_link(&documents.join("projects")) && !documents.join("projects.own").exists());
    assert!(
        is_link(&documents.join("skills")),
        "what is missing is linked"
    );
    save("codex2", "codex", &claudes, &["skills"]);
    assert!(claudes.join("skills").join("mine").is_dir());
    assert!(!is_link(&claudes.join("skills")) && !claudes.join("skills.own").exists());
    save("claude3", "claude", &empty, &["skills", "settings"]);
    assert_eq!(
        std::fs::read_link(empty.join("skills")).unwrap(),
        claudes.join("skills")
    );
}

#[cfg(unix)]
fn is_link(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// The row's Log in opens a block of the account; its dialog's Delete asks, then deletes it.
#[gpui_kit::test]
fn an_accounts_row_logs_in_and_its_dialog_deletes_it(cx: &mut TestAppContext) {
    let (_own, commands) = account_dirs();
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(
        config.path().join(crate::config::FILE),
        super::agents::CLAUDE2,
    )
    .unwrap();
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_accounts(handle, cx);
    assert!(
        commands.path().join("claude2").exists(),
        "made at its start"
    );
    let blocks = ws.read_with(cx, |ws, _| ws.space.blocks.len());
    cx.update_window(handle, |_, window, cx| {
        window.click("log-in-claude2", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let logged = ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.blocks.len(), blocks + 1);
        ws.space
            .terminals()
            .find(|(_, p)| p.account.as_deref() == Some("claude2"))
            .map(|(id, _)| id)
            .expect("a block of the account")
    });
    ws.update(cx, |ws, cx| {
        ws.space.blocks.remove(&logged);
        ws.space.canvas.remove(logged);
        cx.notify();
    });
    open_accounts(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("edit-account-claude2", cx);
        window.render_frame(cx);
    })
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    frame(handle, cx);
    press(handle, cx, "delete-account");
    std::thread::sleep(std::time::Duration::from_millis(300));
    frame(handle, cx);
    let account = data.path().join("accounts").join("claude2");
    assert!(account.is_dir(), "asked first");
    cx.update_window(handle, |_, window, cx| {
        // The question, over the account's dialog.
        window.within("dialog-1").click("ok", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!account.exists());
    assert!(
        !commands.path().join("claude2").exists(),
        "its command goes too"
    );
    ws.read_with(cx, |ws, _| {
        let agents = &ws.pending.as_ref().unwrap_or(&ws.config).agents;
        assert!(!agents.contains_key("claude2"));
    });
}

/// Settings ▸ Agents and its Add an account dialog look for the agents' programs when they open
/// or change, not at each frame: one PATH scan per agent per frame made the tab lag.
#[gpui_kit::test]
fn the_agents_tab_looks_for_programs_when_it_opens_not_each_frame(cx: &mut TestAppContext) {
    let (_own, _commands) = account_dirs();
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_accounts(handle, cx);
    // The looks for a program made while it is drawn 10 times.
    let drawn = |cx: &mut TestAppContext| {
        let before = crate::agents::LOOKS.get();
        cx.update_window(handle, |_, window, cx| {
            for _ in 0..10 {
                window.render_frame(cx);
            }
        })
        .unwrap();
        crate::agents::LOOKS.get() - before
    };
    assert_eq!(drawn(cx), 0, "the agents' rows");
    add_account(handle, &ws, cx, "claudinho");
    assert_eq!(drawn(cx), 0, "the dialog, with a name typed");
    cx.update_window(handle, |_, window, cx| {
        gpui_kit::component::WindowExt::close_all_dialogs(window, cx)
    })
    .unwrap();
    let mine = crate::agents::Draft {
        name: "Mine".into(),
        command: "mine".into(),
        resume: String::new(),
        show: true,
        ..Default::default()
    };
    ws.update(cx, |ws, cx| ws.save_agent(None, &mine, cx).unwrap());
    frame(handle, cx);
    assert_eq!(drawn(cx), 0, "an agent added while the tab shows");
}

/// An item Custom can't offer says why under its paths, in the room of the row: beside them, at
/// the right, the note took all of it and "Settings" broke at each letter.
#[cfg(unix)]
#[gpui_kit::test]
fn an_item_that_cant_be_chosen_keeps_the_rows_room(cx: &mut TestAppContext) {
    let (_own, _commands) = account_dirs();
    let (config, data, chosen, dotfiles) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    // The person's own link: Custom shows it, with no list.
    std::os::unix::fs::symlink(
        dotfiles.path().join("settings.json"),
        chosen.path().join("settings.json"),
    )
    .unwrap();
    let file = format!(
        "[agents.claude2]\nof = \"claude\"\nfolder = '{}'\n",
        chosen.path().display()
    );
    std::fs::write(config.path().join(crate::config::FILE), file).unwrap();
    let (handle, _ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_accounts(handle, cx);
    edit_account(handle, cx, "edit-account-claude2");
    press(handle, cx, "account-setup-custom");
    cx.update_window(handle, |_, window, _| {
        let width = |key: &str| {
            let id = gpui_kit::ElementId::Name(format!("account-item-{key}").into());
            window.find(id).bounds().size.width
        };
        assert!(
            window.try_find("account-share-settings").is_none(),
            "no list"
        );
        // Its column has the row; one with a list leaves it room.
        assert!(width("settings") > width("skills"));
        assert!(width("skills") > px(100.), "{:?}", width("skills"));
    })
    .unwrap();
}
