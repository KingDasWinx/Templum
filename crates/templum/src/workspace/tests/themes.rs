//! Themes: the system appearance, the theme picker and the crossfade.

use super::*;

#[gpui_kit::test]
fn a_theme_change_crossfades_the_chrome(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let light = ws.read_with(cx, |ws, _| {
        assert_eq!(ws.ui, ws.theme.ui, "the first theme shows at once");
        ws.theme.ui
    });
    ws.update(cx, |ws, cx| ws.set_dark(true, cx));
    // The fade starts on the first frame that draws the new theme.
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.executor().advance_clock(crate::motion::THEME / 2);
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_ne!(ws.ui, ws.theme.ui, "fading");
        assert_ne!(ws.ui, light, "fading");
    });
    cx.executor()
        .advance_clock(crate::motion::THEME + std::time::Duration::from_millis(10));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    ws.read_with(cx, |ws, _| assert_eq!(ws.ui, ws.theme.ui, "settled"));
}

#[gpui_kit::test]
fn follows_the_system_appearance(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    // The test window is light.
    ws.read_with(cx, |ws, _| assert_eq!(ws.theme.name, "Catppuccin Latte"));
    ws.update(cx, |ws, cx| ws.set_dark(true, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(NEW, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "Catppuccin Mocha");
        assert_eq!(ws.space.terminals().count(), 2);
        for pane in ws.space.terminals().map(|(_, p)| p) {
            assert_eq!(
                pane.term.palette(),
                ws.theme.palette,
                "old and new terminals"
            );
        }
    });
    cx.update(|cx| {
        let kit = gpui_kit::component::Theme::global(cx);
        assert!(kit.mode.is_dark());
    });
    ws.read_with(cx, |ws, cx| {
        assert_eq!(
            gpui_kit::component::Theme::global(cx).background,
            ws.theme.ui.canvas,
            "gpui-kit's root paints the opaque canvas"
        );
        assert_eq!(
            gpui_kit::component::Theme::global(cx).accent_foreground,
            ws.theme.on_accent(),
            "a selected row's text reads on the accent"
        );
    });
}

#[gpui_kit::test]
fn config_names_pick_the_theme(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "[theme]\nlight = \"One Light\"\n",
    )
    .unwrap();
    let (_, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    ws.read_with(cx, |ws, _| assert_eq!(ws.theme.name, "One Light"));
}

#[gpui_kit::test]
fn theme_picker_previews_cancels_and_saves(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("theme-button", cx);
        window.render_frame(cx);
        window.hover(("theme-item", builtin_index("One Dark")), cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "One Dark", "hover previews")
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("escape", cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "Catppuccin Latte", "Esc puts it back");
        assert!(ws.picker.is_none());
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("theme-button", cx);
        window.render_frame(cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "Catppuccin Frappé");
        assert!(ws.picker.is_none());
    });
    let saved = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(saved.contains("light = \"Catppuccin Frappé\""), "{saved}");
}

#[gpui_kit::test]
fn clicking_outside_the_picker_cancels(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("theme-button", cx);
        window.render_frame(cx);
        window.hover(("theme-item", builtin_index("Apple Light")), cx);
        window.render_frame(cx);
        window.click("picker-backdrop", cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "Catppuccin Latte");
        assert!(ws.picker.is_none());
        assert_eq!(
            ws.space.canvas.blocks().len(),
            1,
            "the click didn't reach the canvas"
        );
    });
}

#[gpui_kit::test]
fn a_broken_config_is_not_overwritten(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[theme\n").unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("theme-button", cx);
        window.render_frame(cx);
        window.click(("theme-item", builtin_index("One Dark")), cx);
        window.render_frame(cx);
        assert!(window.try_find("picker-error").is_some());
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "One Dark", "the choice applies anyway");
        assert!(ws.picker.is_some(), "the picker stays open to show why");
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[theme\n");
}

#[gpui_kit::test]
fn appearance_change_closes_the_picker(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("theme-button", cx);
        window.render_frame(cx);
        window.hover(("theme-item", builtin_index("One Light")), cx);
    })
    .unwrap();
    ws.update(cx, |ws, cx| ws.set_dark(true, cx));
    ws.read_with(cx, |ws, _| {
        assert!(ws.picker.is_none());
        assert_eq!(ws.theme.name, "Catppuccin Mocha");
    });
}

#[gpui_kit::test]
fn appearance_event_without_a_mode_change_keeps_the_picker(cx: &mut TestAppContext) {
    // GPUI also reports appearance on Wayland decoration and window-control changes.
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("theme-button", cx);
        window.render_frame(cx);
        window.hover(("theme-item", builtin_index("One Dark")), cx);
    })
    .unwrap();
    ws.update(cx, |ws, cx| ws.set_dark(false, cx));
    ws.read_with(cx, |ws, _| {
        assert!(ws.picker.is_some(), "still light: the picker stays open");
        assert_eq!(ws.theme.name, "One Dark", "and the preview stays");
    });
}

#[gpui_kit::test]
fn the_picker_shortcut_toggles_and_others_wait(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(PICKER, cx);
        window.render_frame(cx);
        window.press(NEW, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.picker.is_some());
        assert_eq!(
            ws.space.canvas.blocks().len(),
            1,
            "no new terminal under the picker"
        );
    });
    cx.update_window(handle, |_, window, cx| window.press(PICKER, cx))
        .unwrap();
    ws.read_with(cx, |ws, _| assert!(ws.picker.is_none(), "closed like Esc"));
}

#[gpui_kit::test]
fn choosing_in_dark_mode_saves_the_dark_theme(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    ws.update(cx, |ws, cx| ws.set_dark(true, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("theme-button", cx);
        window.render_frame(cx);
        window.click(("theme-item", builtin_index("One Dark")), cx);
    })
    .unwrap();
    let saved = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(saved.contains("dark = \"One Dark\""), "{saved}");
    assert!(!saved.contains("light"), "{saved}");
}

/// A light theme, with `background` as its background.
fn light_theme(background: &str) -> String {
    format!(
        "[colors.primary]\nbackground = \"{background}\"\nforeground = \"#202020\"\n\n\
         [colors.normal]\nblack = \"#000000\"\nred = \"#aa0000\"\ngreen = \"#00aa00\"\n\
         yellow = \"#aaaa00\"\nblue = \"#0000aa\"\nmagenta = \"#aa00aa\"\ncyan = \"#00aaaa\"\n\
         white = \"#aaaaaa\"\n"
    )
}

/// Writes `<dir>/themes/<name>.toml`.
fn write_theme(dir: &std::path::Path, name: &str, text: &str) -> std::path::PathBuf {
    let themes = dir.join("themes");
    std::fs::create_dir_all(&themes).unwrap();
    let path = themes.join(format!("{name}.toml"));
    std::fs::write(&path, text).unwrap();
    path
}

fn poll(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.poll_config(window, cx))
    })
    .unwrap();
}

fn rgb(v: u32) -> templum_terminal::Rgb {
    templum_terminal::Rgb {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    }
}

#[gpui_kit::test]
fn saving_the_theme_file_applies_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_theme(dir.path(), "mine", &light_theme("#fafafa"));
    write_config(dir.path(), "[theme]\nlight = \"mine\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    poll(handle, &ws, cx);
    let first = ws.read_with(cx, |ws, _| ws.theme.clone());
    assert_eq!(first.palette.background, rgb(0xfafafa));
    std::fs::write(&path, light_theme("#f0f0e0") + "# changed\n").unwrap();
    poll(handle, &ws, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.palette.background, rgb(0xf0f0e0));
        let entry = ws.themes.iter().find(|e| e.theme.name == "mine").unwrap();
        assert_eq!(entry.theme, ws.theme, "the list has it too");
        for pane in ws.space.terminals().map(|(_, p)| p) {
            assert_eq!(pane.term.palette(), ws.theme.palette);
        }
    });
    assert_eq!(notification_count(handle, cx), 0);
}

#[gpui_kit::test]
fn a_broken_theme_file_keeps_the_last_good_one_and_says_so_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_theme(dir.path(), "mine", &light_theme("#fafafa"));
    write_config(dir.path(), "[theme]\nlight = \"mine\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    poll(handle, &ws, cx);
    std::fs::write(&path, light_theme("#f0f0e0").replace("red = ", "rde = ")).unwrap();
    poll(handle, &ws, cx);
    poll(handle, &ws, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "mine");
        assert_eq!(ws.theme.palette.background, rgb(0xfafafa), "kept");
    });
    assert_eq!(notification_count(handle, cx), 1, "said once");
    std::fs::write(&path, light_theme("#f0f0e0")).unwrap();
    poll(handle, &ws, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.palette.background, rgb(0xf0f0e0))
    });
}

#[gpui_kit::test]
fn a_theme_edited_while_the_other_mode_is_on_shows_when_switching(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write_theme(dir.path(), "day", &light_theme("#fafafa"));
    let night = write_theme(dir.path(), "night", &light_theme("#101010"));
    write_config(dir.path(), "[theme]\nlight = \"day\"\ndark = \"night\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    poll(handle, &ws, cx);
    std::fs::write(&night, light_theme("#202030") + "\n").unwrap();
    poll(handle, &ws, cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.theme.name, "day"));
    ws.update(cx, |ws, cx| ws.set_dark(true, cx));
    poll(handle, &ws, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "night");
        assert_eq!(ws.theme.palette.background, rgb(0x202030));
    });
}

fn new_theme(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    name: &str,
    cx: &mut TestAppContext,
) -> bool {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_theme(name, window, cx))
    })
    .unwrap()
}

#[gpui_kit::test]
fn new_theme_copies_the_colors_and_becomes_the_modes_theme(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    let latte = ws.read_with(cx, |ws, _| ws.theme.clone());
    assert!(new_theme(handle, &ws, " Mine ", cx));
    frame(handle, cx);
    let path = dir.path().join("themes").join("Mine.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# Mine: a Templum theme, made from Catppuccin Latte."));
    ws.update(cx, |ws, _| ws.save());
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "Mine");
        assert_eq!(ws.theme.palette, latte.palette, "the same colors");
        assert_eq!(ws.theme_path(), Some(path.clone()));
        assert_eq!(ws.config.theme.light, "Mine");
    });
    let config = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(config.contains("light = \"Mine\""), "{config}");
    assert!(!new_theme(handle, &ws, "Mine", cx), "taken");
    assert!(!new_theme(handle, &ws, "a/b", cx), "not a file name");
    assert_eq!(notification_count(handle, cx), 2, "both said why");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        text,
        "not overwritten"
    );
}

#[gpui_kit::test]
fn open_theme_file_opens_it_in_an_editor_block(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_theme(dir.path(), "mine", &light_theme("#fafafa"));
    write_config(dir.path(), "[theme]\nlight = \"mine\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_theme_file(window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let open = ws
            .space
            .blocks
            .values()
            .filter_map(|b| b.editor())
            .any(|e| e.tabs.iter().any(|t| t.path == path));
        assert!(open, "a tab on the theme's file");
    });
}

#[gpui_kit::test]
fn appearance_offers_duplicate_for_builtins_and_open_for_mine(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write_theme(dir.path(), "mine", &light_theme("#fafafa"));
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("theme-duplicate").is_some(),
            "Latte is built in"
        );
        assert!(window.try_find("theme-open-file").is_none());
        assert!(
            window.try_find("theme-color-5").is_none(),
            "no colors to change"
        );
    })
    .unwrap();
    ws.update(cx, |ws, cx| ws.set_mode_theme("mine", cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("theme-open-file").is_some(), "yours");
        assert!(window.try_find("theme-new").is_some());
        assert!(window.try_find("theme-duplicate").is_none());
        assert!(window.try_find("theme-color-5").is_some(), "its colors");
        assert!(
            window.try_find("theme-color-reset-20").is_none(),
            "accent derived"
        );
    })
    .unwrap();
}

fn set_color(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    i: usize,
    color: Option<u32>,
    cx: &mut TestAppContext,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.set_theme_color(i, color.map(rgb), window, cx)
        })
    })
    .unwrap();
}

/// Lets the color editor's write (SAVE_DELAY after the last change) happen.
fn settle_save(cx: &mut TestAppContext) {
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn color_edits_show_at_once_and_are_written_once_settled(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let text = format!(
        "# Mine.\n{}",
        light_theme("#fafafa").replace("red = ", "# Errors.\nred = ")
    );
    let path = write_theme(dir.path(), "mine", &text);
    write_config(dir.path(), "[theme]\nlight = \"mine\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    poll(handle, &ws, cx);
    set_color(handle, &ws, 5, Some(0xff0000), cx);
    set_color(handle, &ws, 5, Some(0xee1111), cx);
    set_color(handle, &ws, 20, Some(0xff8800), cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.palette.ansi[1], rgb(0xee1111), "shown at once");
        assert_eq!(ws.theme.ui.accent, crate::grid::hsla(rgb(0xff8800), 1.0));
        for pane in ws.space.terminals().map(|(_, p)| p) {
            assert_eq!(pane.term.palette(), ws.theme.palette);
        }
    });
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        text,
        "not written yet"
    );
    settle_save(cx);
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("# Errors.\nred = \"#ee1111\""), "{saved}");
    assert!(saved.starts_with("# Mine.\n"));
    assert!(saved.contains("[templum]\naccent = \"#ff8800\""), "{saved}");
    poll(handle, &ws, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.palette.ansi[1], rgb(0xee1111))
    });
    assert_eq!(notification_count(handle, cx), 0);
}

#[gpui_kit::test]
fn a_color_reset_takes_the_key_out(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let text = light_theme("#fafafa") + "\n[templum]\ncanvas = \"#000000\"\n";
    let path = write_theme(dir.path(), "mine", &text);
    write_config(dir.path(), "[theme]\nlight = \"mine\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    let derived = crate::theme::parse("x", &light_theme("#fafafa"))
        .unwrap()
        .ui
        .canvas;
    set_color(handle, &ws, 21, None, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.interface[1], None);
        assert_eq!(ws.theme.ui.canvas, derived, "worked out again");
    });
    settle_save(cx);
    assert!(!std::fs::read_to_string(&path).unwrap().contains("canvas"));
}

#[gpui_kit::test]
fn color_edits_reach_their_file_after_a_mode_switch(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let day = write_theme(dir.path(), "day", &light_theme("#fafafa"));
    let night = write_theme(dir.path(), "night", &light_theme("#101010"));
    write_config(dir.path(), "[theme]\nlight = \"day\"\ndark = \"night\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    set_color(handle, &ws, 5, Some(0xff0000), cx);
    ws.update(cx, |ws, cx| ws.set_dark(true, cx));
    set_color(handle, &ws, 6, Some(0x00ff00), cx);
    let written = std::fs::read_to_string(&day).unwrap();
    assert!(
        written.contains("red = \"#ff0000\""),
        "day's, before night's: {written}"
    );
    settle_save(cx);
    let night_text = std::fs::read_to_string(&night).unwrap();
    assert!(night_text.contains("green = \"#00ff00\""));
    assert!(
        !night_text.contains("#ff0000"),
        "day's change stayed in day"
    );
}

#[gpui_kit::test]
fn a_broken_theme_file_is_not_overwritten_by_the_editor(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_theme(dir.path(), "mine", &light_theme("#fafafa"));
    write_config(dir.path(), "[theme]\nlight = \"mine\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    poll(handle, &ws, cx);
    std::fs::write(&path, "[colors\n").unwrap();
    set_color(handle, &ws, 5, Some(0xff0000), cx);
    set_color(handle, &ws, 5, Some(0xee0000), cx);
    settle_save(cx);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[colors\n",
        "left alone"
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.palette.ansi[1], rgb(0xaa0000), "not shown")
    });
    // The message, "Not saved: mine.toml has an error on line 1", is `describe`'s (config.rs).
    assert_eq!(notification_count(handle, cx), 1, "one for both changes");
}

/// A dark theme, with `background` as its background.
fn dark_theme(background: &str) -> String {
    light_theme(background).replace("#202020", "#e0e0e0")
}

/// Lets a hovered gallery card be previewed: it is once the mouse rests on it.
fn rest_on_a_card(cx: &mut TestAppContext) {
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
}

fn open_gallery(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| crate::gallery::open(ws, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_gallery_previews_restores_and_installs(cx: &mut TestAppContext) {
    // The gallery slides in by the clock: still, a click finds what the frame drew.
    no_motion(cx);
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    crate::gallery::tests::seed(
        data.path(),
        &[
            ("dawn_light.toml", &light_theme("#fdf6e3")),
            ("paper.toml", &light_theme("#f2eede")),
            ("night_owl.toml", &dark_theme("#011627")),
        ],
    );
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_gallery(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("gallery-theme-Paper").is_some());
        assert!(
            window.try_find("gallery-theme-Night Owl").is_some(),
            "the other mode's themes too"
        );
        window.hover("gallery-theme-Dawn Light", cx);
        window.hover("gallery-theme-Paper", cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.theme.name, "Catppuccin Latte",
            "passing over: no preview yet"
        )
    });
    rest_on_a_card(cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.theme.palette.background,
            rgb(0xf2eede),
            "only the one it rests on"
        )
    });
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "Catppuccin Latte", "put back")
    });
    assert!(!config.path().join("themes").exists(), "nothing installed");

    open_gallery(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.hover("gallery-theme-Paper", cx);
        window.click("gallery-theme-Paper", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let file = config.path().join("themes").join("Paper.toml");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.starts_with("# From https://github.com/alacritty/alacritty-theme (themes/paper.toml)")
    );
    ws.update(cx, |ws, _| ws.save());
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "Paper");
        assert_eq!(ws.theme_path(), Some(file.clone()), "yours now: editable");
    });
    let saved = std::fs::read_to_string(config.path().join("config.toml")).unwrap();
    assert!(saved.contains("light = \"Paper\""), "{saved}");

    // Installed already: chosen as it is, nothing written over.
    std::fs::write(&file, light_theme("#ffffff")).unwrap();
    ws.update(cx, |ws, cx| ws.set_mode_theme("Catppuccin Latte", cx));
    open_gallery(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("gallery-theme-Paper", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "Paper");
        assert_eq!(
            ws.theme.palette.background,
            rgb(0xffffff),
            "the installed one"
        );
    });
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        light_theme("#ffffff")
    );
    assert_eq!(notification_count(handle, cx), 0);

    // A dark theme, chosen in light mode, becomes the light theme, as in the picker.
    open_gallery(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("gallery-theme-Night Owl", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "Night Owl");
        assert_eq!(ws.config.theme.light, "Night Owl");
    });
}

#[gpui_kit::test]
fn the_picker_opens_the_gallery(cx: &mut TestAppContext) {
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    crate::gallery::tests::seed(data.path(), &[("paper.toml", &light_theme("#f2eede"))]);
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("theme-button", cx);
        window.render_frame(cx);
        window.click("picker-gallery", cx);
        window.render_frame(cx);
        assert!(window.try_find("gallery-theme-Paper").is_some());
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.picker.is_none(), "the picker closed")
    });
    // Without a data folder there is no cache: no way in.
    let (handle, _) = open_with(cx, Some(config.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("theme-button", cx);
        window.render_frame(cx);
        assert!(window.try_find("picker-gallery").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_hand_edit_and_pending_color_edits_both_land(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_theme(dir.path(), "mine", &light_theme("#fafafa"));
    write_config(dir.path(), "[theme]\nlight = \"mine\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    poll(handle, &ws, cx);
    set_color(handle, &ws, 5, Some(0xff0000), cx);
    std::fs::write(&path, light_theme("#f0f0e0") + "# by hand\n").unwrap();
    poll(handle, &ws, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.palette.background, rgb(0xf0f0e0), "the hand's");
        assert_eq!(ws.theme.palette.ansi[1], rgb(0xff0000), "the editor's");
    });
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("red = \"#ff0000\"") && text.contains("# by hand"),
        "{text}"
    );
}

#[gpui_kit::test]
fn a_change_that_cant_show_keeps_the_ones_before(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_theme(dir.path(), "mine", &light_theme("#fafafa"));
    write_config(dir.path(), "[theme]\nlight = \"mine\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    set_color(handle, &ws, 5, Some(0xff0000), cx);
    std::fs::write(&path, "[colors\n").unwrap();
    set_color(handle, &ws, 6, Some(0x00ff00), cx);
    assert_eq!(notification_count(handle, cx), 1, "this one can't show");
    std::fs::write(&path, light_theme("#fafafa")).unwrap();
    settle_save(cx);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("red = \"#ff0000\""),
        "the first still written: {text}"
    );
    assert!(!text.contains("#00ff00"));
}

#[gpui_kit::test]
fn choosing_the_theme_already_named_shows_it_at_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    ws.update(cx, |ws, cx| {
        let previewed = crate::theme::find(&ws.themes, "One Light").unwrap();
        ws.apply(previewed, cx);
        // The config names it already: the edit changes nothing, the screen still must.
        ws.set_mode_theme("Catppuccin Latte", cx);
        assert_eq!(ws.theme.name, "Catppuccin Latte");
    });
}

#[gpui_kit::test]
fn esc_in_the_picker_keeps_a_theme_saved_meanwhile(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_theme(dir.path(), "mine", &light_theme("#fafafa"));
    write_config(dir.path(), "[theme]\nlight = \"mine\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    poll(handle, &ws, cx);
    ws.update(cx, |ws, cx| ws.toggle_theme_picker(cx));
    std::fs::write(&path, light_theme("#f0f0e0") + "\n").unwrap();
    poll(handle, &ws, cx);
    ws.update(cx, |ws, cx| ws.cancel_theme(cx));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.palette.background, rgb(0xf0f0e0), "the saved one")
    });
}

#[gpui_kit::test]
fn an_install_that_fails_puts_the_theme_back(cx: &mut TestAppContext) {
    // The gallery slides in by the clock: still, a click finds what the frame drew.
    no_motion(cx);
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    crate::gallery::tests::seed(data.path(), &[("paper.toml", &light_theme("#f2eede"))]);
    // No themes folder can be made.
    std::fs::write(config.path().join("themes"), "").unwrap();
    let (handle, ws) = open_in(
        cx,
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    open_gallery(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.hover("gallery-theme-Paper", cx);
    })
    .unwrap();
    rest_on_a_card(cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.theme.name, "Paper", "previewed"));
    cx.update_window(handle, |_, window, cx| {
        window.click("gallery-theme-Paper", cx)
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.theme.name, "Catppuccin Latte", "not the preview")
    });
    assert_eq!(notification_count(handle, cx), 1, "why");
}

#[gpui_kit::test]
fn a_save_says_why_both_files_failed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_theme(dir.path(), "mine", &light_theme("#fafafa"));
    write_config(dir.path(), "[theme]\nlight = \"mine\"\n");
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    set_color(handle, &ws, 5, Some(0xff0000), cx);
    ws.update(cx, |ws, cx| {
        ws.edit(Edit::set("font", "size", 20), |c| c.font.size = 20.0, cx)
    });
    std::fs::write(&path, "[colors\n").unwrap();
    write_config(dir.path(), "[font\n");
    let message = ws.update(cx, |ws, _| ws.save()).unwrap();
    assert!(
        message.contains("config.toml has an error on line 1"),
        "{message}"
    );
    assert!(
        message.contains("mine.toml has an error on line 1"),
        "{message}"
    );
}
