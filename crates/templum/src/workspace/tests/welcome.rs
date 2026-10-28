//! The welcome: when it shows, its pages, and what its choices change.

use super::*;
use crate::welcome::Page;

fn up(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> bool {
    ws.read_with(cx, |ws, _| ws.welcome.as_ref().is_some_and(|w| !w.closing))
}

fn page(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Page {
    ws.read_with(cx, |ws, _| ws.welcome.as_ref().unwrap().page)
}

fn blocks(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> usize {
    ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len())
}

/// A first run: empty config and data folders, the welcome open on its first page.
fn first_run(
    cx: &mut TestAppContext,
) -> (
    AnyWindowHandle,
    Entity<Workspace>,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (handle, ws) = open_welcomed(
        cx,
        Some(quiet()),
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    // The card and each page slide in in real time: a click mid-slide can miss its row.
    no_motion(cx);
    ws.update(cx, |ws, _| {
        ws.welcome.as_mut().unwrap().since -= crate::motion::SLIDE;
    });
    frame(handle, cx);
    (handle, ws, config, data)
}

/// Shows page `to`, and draws it.
fn go(handle: AnyWindowHandle, ws: &Entity<Workspace>, to: Page, cx: &mut TestAppContext) {
    ws.update(cx, |ws, cx| ws.welcome_page(to, cx));
    frame(handle, cx);
}

/// Scrolls the page to its end, where its last rows are.
fn to_bottom(window: &mut Window, cx: &mut gpui_kit::App) {
    window.scroll(
        "welcome-scroll",
        ScrollDelta::Pixels(point(px(0.0), px(-2000.0))),
        cx,
    );
}

#[gpui_kit::test]
fn a_first_run_welcomes_and_opens_its_terminal_after(cx: &mut TestAppContext) {
    let (handle, ws, _config, _data) = first_run(cx);
    assert!(up(&ws, cx));
    assert_eq!(page(&ws, cx), Page::Hello);
    assert_eq!(
        blocks(&ws, cx),
        0,
        "the terminal waits for the shell picked"
    );
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    assert!(!up(&ws, cx), "Esc skips it");
    assert_eq!(blocks(&ws, cx), 1);
    assert!(ws.read_with(cx, |ws, _| ws.space.pristine), "centered");
    // Fading out, it lets the canvas have the pointer and the keyboard.
    let id = first_id(&ws, cx);
    assert_eq!(ws.read_with(cx, |ws, _| ws.space.focused), Some(id));
}

#[gpui_kit::test]
fn a_saved_canvas_a_damaged_one_or_no_data_folder_means_no_welcome(cx: &mut TestAppContext) {
    let saved = tempfile::tempdir().unwrap();
    write_canvas(
        saved.path(),
        r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[]}"#,
    );
    let damaged = tempfile::tempdir().unwrap();
    write_canvas(damaged.path(), "{nope");
    for (data, terminals) in [
        (Some(saved.path().to_path_buf()), 0),
        (Some(damaged.path().to_path_buf()), 1),
        // `--bench`: nothing is kept, the terminal opens as it always did.
        (None, 1),
    ] {
        let (_, ws) = open_welcomed(cx, Some(quiet()), None, data.clone());
        assert!(ws.read_with(cx, |ws, _| ws.welcome.is_none()), "{data:?}");
        assert_eq!(blocks(&ws, cx), terminals, "{data:?}");
    }
}

#[gpui_kit::test]
fn next_back_enter_and_the_steps_move_between_pages(cx: &mut TestAppContext) {
    let (handle, ws, _config, _data) = first_run(cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(
            window.try_find("welcome-back").is_none(),
            "none on the first"
        );
        window.click("welcome-next", cx);
    })
    .unwrap();
    assert_eq!(page(&ws, cx), Page::Look);
    cx.update_window(handle, |_, window, cx| window.click("welcome-back", cx))
        .unwrap();
    assert_eq!(page(&ws, cx), Page::Hello);
    cx.update_window(handle, |_, window, cx| window.press("enter", cx))
        .unwrap();
    assert_eq!(page(&ws, cx), Page::Look, "Enter on the card goes on");
    cx.update_window(handle, |_, window, cx| window.click("welcome-step-4", cx))
        .unwrap();
    assert_eq!(page(&ws, cx), Page::Ready);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("welcome-skip").is_none(),
            "none on the last"
        );
        window.click("welcome-next", cx);
    })
    .unwrap();
    assert!(!up(&ws, cx), "the last page's button closes it");
    assert_eq!(blocks(&ws, cx), 1);
}

#[gpui_kit::test]
fn a_theme_card_sets_the_theme_of_the_mode_in_effect(cx: &mut TestAppContext) {
    let (handle, ws, config, _data) = first_run(cx);
    go(handle, &ws, Page::Look, cx);
    let dark = ws.read_with(cx, |ws, _| ws.dark);
    let name = if dark { "One Dark" } else { "One Light" };
    cx.update_window(handle, |_, window, cx| {
        let other = if dark { "One Light" } else { "One Dark" };
        assert!(
            window.try_find(format!("welcome-theme-{other}")).is_none(),
            "only the mode's themes"
        );
        window.click(format!("welcome-theme-{name}"), cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.config.theme.name(dark), name);
        assert_eq!(ws.theme.name, name, "applied");
    });
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(config.path().join(crate::config::FILE)).unwrap();
    let key = if dark { "dark" } else { "light" };
    assert!(text.contains(&format!("{key} = \"{name}\"")), "{text}");
}

#[gpui_kit::test]
fn a_shell_card_is_the_shell_of_the_first_terminal(cx: &mut TestAppContext) {
    use templum_terminal::ShellOption;
    let (handle, ws, config, _data) = first_run(cx);
    ws.update(cx, |ws, _| {
        ws.shells = vec![ShellOption {
            label: "PowerShell 7".into(),
            program: "pwsh".into(),
            args: vec!["-NoLogo".into()],
        }];
    });
    go(handle, &ws, Page::Shell, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("welcome-shell-0", cx);
        window.click("welcome-shell-default", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.config.shell.program.clone()),
        None,
        "Default takes it back"
    );
    // Chosen, then Esc at once: the terminal that opens already has it, and it is written.
    cx.update_window(handle, |_, window, cx| {
        window.click("welcome-shell-0", cx);
        window.press("escape", cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.config.shell.program.as_deref(), Some("pwsh"));
        assert_eq!(ws.config.shell.args, ["-NoLogo"]);
        assert_eq!(ws.space.canvas.blocks().len(), 1);
    });
    let text = std::fs::read_to_string(config.path().join(crate::config::FILE)).unwrap();
    assert!(text.contains("program = \"pwsh\""), "{text}");
}

#[gpui_kit::test]
fn the_shell_page_switches_shell_integration(cx: &mut TestAppContext) {
    let (handle, ws, _config, _data) = first_run(cx);
    go(handle, &ws, Page::Shell, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(
            window.try_find("field-shell.program").is_none(),
            "only the rows a newcomer needs"
        );
        to_bottom(window, cx);
        window.click("field-shell.integration", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!ws.read_with(cx, |ws, _| ws.config.shell.integration));
}

#[gpui_kit::test]
fn the_agents_page_lists_the_agents_found(cx: &mut TestAppContext) {
    let (handle, ws, _config, _data) = first_run(cx);
    ws.update(cx, |ws, _| {
        ws.welcome.as_mut().unwrap().installed = vec!["codex".into()];
    });
    go(handle, &ws, Page::Agents, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("field-agents.codex").is_some());
        assert!(window.try_find("field-agents.claude").is_none());
        assert!(window.try_find("welcome-no-agents").is_none());
        to_bottom(window, cx);
        assert!(window.try_find("field-usage.claude_limits").is_some());
    })
    .unwrap();
    ws.update(cx, |ws, _| ws.welcome.as_mut().unwrap().installed.clear());
    frame(handle, cx);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("welcome-no-agents").is_some(), "none found")
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_show_the_welcome_again_without_a_new_terminal(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| window.click("show-welcome", cx))
        .unwrap();
    assert!(up(&ws, cx));
    assert!(!panel_open(&ws, cx), "the panel makes way");
    frame(handle, cx);
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    assert!(!up(&ws, cx));
    assert_eq!(blocks(&ws, cx), 1, "the terminal that was there, alone");
}

#[gpui_kit::test]
fn quitting_from_a_first_runs_welcome_leaves_a_first_run(cx: &mut TestAppContext) {
    let (handle, ws, config, data) = first_run(cx);
    // The sidebar's button writes the workspace list; quitting writes the canvas.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.sidebar = !ws.sidebar;
            ws.write_index(window, cx);
            ws.save_canvas_now(cx);
        })
    })
    .unwrap();
    assert!(!data.path().join("workspaces").exists(), "nothing written");
    let (_, again) = open_welcomed(
        cx,
        Some(quiet()),
        Some(config.path().to_path_buf()),
        Some(data.path().to_path_buf()),
    );
    assert!(up(&again, cx), "welcomed again");
}

#[gpui_kit::test]
fn a_new_workspace_over_the_welcome_closes_it(cx: &mut TestAppContext) {
    let (handle, ws, _config, _data) = first_run(cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_space(window, cx))
    })
    .unwrap();
    assert!(!up(&ws, cx), "the keyboard goes to the new workspace");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.canvas.blocks().len(), 0, "the new one is empty");
        assert_eq!(
            ws.others[0].canvas.blocks().len(),
            1,
            "the first terminal is in the first workspace"
        );
    });
}

#[gpui_kit::test]
fn holding_the_quit_shortcut_over_the_welcome_is_not_a_second_press(cx: &mut TestAppContext) {
    #[cfg(target_os = "macos")]
    const QUIT: &str = "cmd-q";
    #[cfg(not(target_os = "macos"))]
    const QUIT: &str = "ctrl-shift-q";
    let (handle, ws, _config, _data) = first_run(cx);
    cx.update_window(handle, |_, window, cx| {
        window.press(QUIT, cx);
        window.dispatch_event(
            PlatformInput::KeyDown(gpui_kit::KeyDownEvent {
                keystroke: gpui_kit::Keystroke::parse(QUIT).unwrap(),
                is_held: true,
                prefer_character_input: false,
            }),
            cx,
        );
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.quit_armed.is_some(), "still waiting for a second press")
    });
}

#[gpui_kit::test]
fn a_double_click_on_skip_opens_one_terminal(cx: &mut TestAppContext) {
    let (handle, ws, _config, _data) = first_run(cx);
    cx.update_window(handle, |_, window, cx| {
        window.double_click("welcome-skip", cx)
    })
    .unwrap();
    assert!(!up(&ws, cx));
    assert_eq!(blocks(&ws, cx), 1);
}

#[gpui_kit::test]
fn the_theme_picker_over_the_welcome_has_its_keys(cx: &mut TestAppContext) {
    let (handle, ws, _config, _data) = first_run(cx);
    ws.update(cx, |ws, cx| ws.toggle_theme_picker(cx));
    frame(handle, cx);
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.picker.is_none(), "Esc closes the picker")
    });
    assert!(up(&ws, cx), "and only the picker");
    ws.update(cx, |ws, _| ws.usage_open = true);
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(!ws.usage_open, "Esc closes the popover")
    });
    assert!(up(&ws, cx), "and only the popover");
}

type Dirs = [tempfile::TempDir; 3];

/// A first run on its first page, with a start-at-login entry in a temporary home.
fn first_with_entry(
    cx: &mut TestAppContext,
) -> (
    AnyWindowHandle,
    Entity<Workspace>,
    crate::autostart::Autostart,
    Dirs,
) {
    let (handle, ws, config, data) = first_run(cx);
    let home = tempfile::tempdir().unwrap();
    let entry = with_autostart(&ws, cx, home.path());
    (handle, ws, entry, [config, data, home])
}

/// `first_with_entry`, on the last page.
fn ready(
    cx: &mut TestAppContext,
) -> (
    AnyWindowHandle,
    Entity<Workspace>,
    crate::autostart::Autostart,
    Dirs,
) {
    let (handle, ws, entry, dirs) = first_with_entry(cx);
    go(handle, &ws, Page::Ready, cx);
    (handle, ws, entry, dirs)
}

fn click_autostart(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        to_bottom(window, cx);
        window.click("field-general.autostart", cx);
    })
    .unwrap();
}

/// What the welcome's start-at-login switch shows.
fn shown(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> bool {
    ws.read_with(cx, |ws, _| {
        ws.welcome.as_ref().unwrap().autostart_shown(&ws.autostart)
    })
}

/// The workspace reads the system's entry again, as when the welcome opens.
fn read_system(ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    ws.update(cx, |ws, cx| ws.load_autostart(cx));
    cx.run_until_parked();
}

/// Presses Start using Templum (the last page's button) and lets the change reach the system.
fn start_using(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| window.click("welcome-next", cx))
        .unwrap();
    assert!(!up(ws, cx));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn a_first_run_offers_start_at_login_on_and_applies_it_when_finished(cx: &mut TestAppContext) {
    let (handle, ws, entry, _dirs) = ready(cx);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("field-general.autostart").is_some());
    })
    .unwrap();
    assert!(shown(&ws, cx));
    assert!(!entry.is_on(), "shown on, but nothing is written yet");
    start_using(handle, &ws, cx);
    assert!(entry.is_on(), "Start using Templum applies the default");
    assert!(ws.read_with(cx, |ws, _| ws.autostart.on()));
}

#[gpui_kit::test]
fn skipping_before_the_option_is_shown_never_starts_templum_at_login(cx: &mut TestAppContext) {
    let (handle, ws, entry, _dirs) = first_with_entry(cx);
    go(handle, &ws, Page::Look, cx);
    cx.update_window(handle, |_, window, cx| window.click("welcome-skip", cx))
        .unwrap();
    assert!(!up(&ws, cx));
    cx.run_until_parked();
    assert!(!entry.is_on(), "the person never saw the option");
    // Esc on the first page is the same.
    let (handle, ws, entry, _dirs) = first_with_entry(cx);
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    assert!(!up(&ws, cx));
    cx.run_until_parked();
    assert!(!entry.is_on());
}

#[gpui_kit::test]
fn leaving_the_last_page_by_esc_applies_what_it_shows(cx: &mut TestAppContext) {
    let (handle, ws, entry, _dirs) = ready(cx);
    assert!(shown(&ws, cx));
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    assert!(!up(&ws, cx));
    cx.run_until_parked();
    assert!(entry.is_on(), "it showed on, so on it is");
    // Having seen it, going back to an earlier page and skipping from there is the same.
    let (handle, ws, entry, _dirs) = ready(cx);
    go(handle, &ws, Page::Look, cx);
    cx.update_window(handle, |_, window, cx| window.click("welcome-skip", cx))
        .unwrap();
    assert!(!up(&ws, cx));
    cx.run_until_parked();
    assert!(entry.is_on());
}

#[gpui_kit::test]
fn switching_it_off_in_the_welcome_keeps_it_off(cx: &mut TestAppContext) {
    let (handle, ws, entry, _dirs) = ready(cx);
    click_autostart(handle, cx);
    assert!(!shown(&ws, cx));
    assert!(!entry.is_on(), "applied when it closes");
    start_using(handle, &ws, cx);
    assert!(!entry.is_on());
}

#[gpui_kit::test]
fn a_choice_made_in_the_welcome_applies_even_when_it_is_skipped(cx: &mut TestAppContext) {
    let (handle, ws, entry, _dirs) = first_with_entry(cx);
    // Chosen, then skipped from an earlier page: an explicit choice, so it applies.
    go(handle, &ws, Page::Ready, cx);
    click_autostart(handle, cx);
    click_autostart(handle, cx);
    go(handle, &ws, Page::Look, cx);
    cx.update_window(handle, |_, window, cx| window.click("welcome-skip", cx))
        .unwrap();
    assert!(!up(&ws, cx));
    cx.run_until_parked();
    assert!(entry.is_on());
}

#[gpui_kit::test]
fn a_dev_build_is_not_offered_to_start_at_login(cx: &mut TestAppContext) {
    let (handle, ws, _entry, _dirs) = ready(cx);
    let home = tempfile::tempdir().unwrap();
    let dev = crate::autostart::Autostart::in_dir("linux", home.path(), "/src/templum", true);
    ws.update(cx, |ws, _| ws.autostart.entry = Some(dev.clone()));
    assert!(!shown(&ws, cx), "its entry is a build's, off by default");
    start_using(handle, &ws, cx);
    assert!(!dev.is_on());
    // Still the person's to switch on.
    let (handle, ws, _entry, _dirs) = ready(cx);
    ws.update(cx, |ws, _| ws.autostart.entry = Some(dev.clone()));
    click_autostart(handle, cx);
    assert!(shown(&ws, cx));
    start_using(handle, &ws, cx);
    assert!(dev.is_on());
}

#[gpui_kit::test]
fn a_first_run_leaves_an_entry_the_system_switched_off_as_it_is(cx: &mut TestAppContext) {
    let (handle, ws, entry, dirs) = ready(cx);
    entry.set(true).unwrap();
    let path = dirs[2].path().join(".config/autostart/templum.desktop");
    let off = format!("{}Hidden=true\n", std::fs::read_to_string(&path).unwrap());
    std::fs::write(&path, &off).unwrap();
    read_system(&ws, cx);
    assert!(!shown(&ws, cx), "switched off with the system's tools");
    start_using(handle, &ws, cx);
    assert!(!entry.is_on());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), off);
    // One that is on shows on.
    let (_handle, ws, entry, _dirs) = ready(cx);
    entry.set(true).unwrap();
    read_system(&ws, cx);
    assert!(shown(&ws, cx));
}

#[gpui_kit::test]
fn the_welcome_opened_again_shows_the_system_and_changes_only_what_was_chosen(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    let home = tempfile::tempdir().unwrap();
    let entry = with_autostart(&ws, cx, home.path());
    entry.set(true).unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_welcome(false, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(ws.autostart.on(), "read from the system");
        assert!(ws.welcome.as_ref().unwrap().autostart_shown(&ws.autostart));
    });
    // Finishing without touching it leaves the system's entry as it is.
    go(handle, &ws, Page::Ready, cx);
    start_using(handle, &ws, cx);
    assert!(entry.is_on());
    // Opened again and switched off: that is applied.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_welcome(false, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    go(handle, &ws, Page::Ready, cx);
    click_autostart(handle, cx);
    start_using(handle, &ws, cx);
    assert!(!entry.is_on());
    // Not a first run, and no entry: no default.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_welcome(false, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!shown(&ws, cx));
}

/// Opens the settings panel from the welcome (the shortcut) and lets the entry's change and the
/// panel's reading reach the system and the workspace.
fn open_settings(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.toggle_settings(window, cx))
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn settings_opened_from_the_last_page_shows_the_default_the_welcome_turned_on(
    cx: &mut TestAppContext,
) {
    // Written as the welcome closes, and that is what the switch shows, not what was there before.
    let (handle, ws, entry, _dirs) = ready(cx);
    open_settings(handle, &ws, cx);
    assert!(entry.is_on());
    assert!(ws.read_with(cx, |ws, _| ws.autostart.on()));
}

#[gpui_kit::test]
fn settings_opened_from_the_last_page_shows_an_entry_the_welcome_turned_off(
    cx: &mut TestAppContext,
) {
    let (handle, ws, entry, _dirs) = ready(cx);
    entry.set(true).unwrap();
    read_system(&ws, cx);
    click_autostart(handle, cx);
    open_settings(handle, &ws, cx);
    assert!(!entry.is_on());
    assert!(!ws.read_with(cx, |ws, _| ws.autostart.on()));
}
