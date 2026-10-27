//! The Home (docs/specs/home.md): opening it at launch and closing it, its search, its tiles and
//! their presses, its cards, recent folders and its keys.

use std::time::Instant;

use gpui_kit::Focusable as _;

use super::agents_view::agent;
use super::inbox::asks;
use super::*;
use crate::home::Wait;
use crate::space_dialog::Look;
use crate::workspace::AgentState;
use crate::workspace::agents_view::Show;

#[cfg(target_os = "macos")]
const WORKSPACES: &str = "cmd-shift-o";
#[cfg(not(target_os = "macos"))]
const WORKSPACES: &str = "ctrl-shift-o";

fn up(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> bool {
    ws.read_with(cx, |ws, _| ws.home_up())
}

/// Three workspaces, "Main", "Api" and "Web", the last one shown.
fn three(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            for (id, name) in [("w2", "Api"), ("w3", "Web")] {
                ws.new_space(window, cx);
                ws.edit_space(id, name, Look::default(), None, window, cx)
                    .unwrap();
            }
        })
    })
    .unwrap();
}

/// Does `act` in the window, then draws a frame.
fn act(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    act: impl FnOnce(&mut Window, &mut gpui_kit::App),
) {
    cx.update_window(handle, |_, window, cx| {
        act(window, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Opens the Home with its command, and lets it come up (on the real clock).
fn open_home(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    act(handle, cx, |w, cx| w.press(WORKSPACES, cx));
    settle_slide(handle, cx);
}

/// Shows workspace `at`.
fn show(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, at: usize) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(at, window, cx))
    })
    .unwrap();
}

/// Its count of `wait`, as the tile says it.
fn count(ws: &Entity<Workspace>, cx: &mut TestAppContext, wait: Wait) -> usize {
    ws.read_with(cx, |ws, _| {
        let at = Wait::ALL.iter().position(|w| *w == wait).unwrap();
        ws.waiting(&ws.agent_rows())[at]
    })
}

fn shown_name(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> String {
    ws.read_with(cx, |ws, _| ws.space.name.clone())
}

#[gpui_kit::test]
fn templum_starts_on_the_home_over_the_workspace_shown_last(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    three(handle, &ws, cx);
    show(handle, &ws, cx, 1);
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let (handle, again) = open_welcomed(cx, Some(quiet()), None, Some(data.path().to_path_buf()));
    assert!(up(&again, cx), "the Home, at launch");
    again.read_with(cx, |ws, _| {
        assert!(ws.welcome.is_none());
        assert_eq!(ws.space.name, "Api", "the one shown last, behind it");
        let home = ws.screen.as_ref().unwrap();
        assert_eq!(home.shown(Instant::now()), 1.0, "up at once");
    });
    act(handle, cx, |w, cx| w.press("escape", cx));
    assert!(!up(&again, cx), "Esc returns to it");
    assert_eq!(shown_name(&again, cx), "Api");
    let canvas = again.read_with(cx, |ws, _| ws.focus_handle.clone());
    let focused = cx
        .update_window(handle, |_, window, cx| window.focused(cx))
        .unwrap();
    assert_eq!(focused, Some(canvas), "the canvas has the keyboard");
}

#[gpui_kit::test]
fn a_first_run_or_no_data_folder_starts_without_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_welcomed(cx, Some(quiet()), None, Some(data.path().to_path_buf()));
    ws.read_with(cx, |ws, _| assert!(ws.welcome.is_some()));
    assert!(!up(&ws, cx), "never over the welcome");
    act(handle, cx, |w, cx| w.press("escape", cx));
    assert!(
        !up(&ws, cx),
        "the welcome's closing opens the first terminal, not the Home"
    );
    let (_, bench) = open_welcomed(cx, Some(quiet()), None, None);
    assert!(!up(&bench, cx), "--bench");
}

#[gpui_kit::test]
fn the_button_the_name_and_the_command_open_and_close_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    act(handle, cx, |w, cx| w.click("home-button", cx));
    assert!(up(&ws, cx), "the sidebar's Home button");
    settle_slide(handle, cx);
    act(handle, cx, |w, cx| w.click("home-button", cx));
    assert!(!up(&ws, cx), "the button again");
    act(handle, cx, |w, cx| w.click("workspace-name", cx));
    assert!(up(&ws, cx), "the name in the title bar");
    act(handle, cx, |w, cx| w.press("escape", cx));
    assert!(!up(&ws, cx), "Esc");
    act(handle, cx, |w, cx| w.press(WORKSPACES, cx));
    assert!(up(&ws, cx), "the command");
    act(handle, cx, |w, cx| w.press(WORKSPACES, cx));
    assert!(!up(&ws, cx), "the command again, from the search field");
}

#[gpui_kit::test]
fn the_search_finds_a_workspace_by_name_or_folder(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    let folder = tempfile::tempdir().unwrap();
    let web = folder.path().join("storefront");
    std::fs::create_dir(&web).unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.edit_space("w3", "Web", Look::default(), Some(web.clone()), window, cx)
        })
    })
    .unwrap()
    .unwrap();
    open_home(handle, cx);
    act(handle, cx, |w, cx| {
        assert!(w.try_find("space-card-default").is_some());
        w.input("AP", cx);
    });
    act(handle, cx, |w, cx| {
        assert!(w.try_find("space-card-w2").is_some(), "Api");
        assert!(w.try_find("space-card-default").is_none());
        assert!(w.try_find("space-card-w3").is_none());
        w.press("escape", cx);
    });
    settle_slide(handle, cx);
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.input("storefr", cx));
    act(handle, cx, |w, _| {
        assert!(w.try_find("space-card-w3").is_some(), "its folder");
        assert!(w.try_find("space-card-w2").is_none());
    });
}

#[gpui_kit::test]
fn clicking_a_card_switches_and_closes_the_home(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.click("space-card-default", cx));
    assert!(!up(&ws, cx));
    assert_eq!(shown_name(&ws, cx), "Main");
    // The one behind it returns too.
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.click("space-card-default", cx));
    assert!(!up(&ws, cx), "the workspace shown");
}

#[gpui_kit::test]
fn down_and_enter_in_the_search_open_the_highlighted_card(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.press("enter", cx));
    assert_eq!(shown_name(&ws, cx), "Main", "the first card");
    assert!(!up(&ws, cx));
    open_home(handle, cx);
    // Four cards to a row: Down goes to the last of the three, Up back to the first.
    act(handle, cx, |w, cx| w.press("down", cx));
    act(handle, cx, |w, cx| w.press("up", cx));
    act(handle, cx, |w, cx| w.press("down", cx));
    act(handle, cx, |w, cx| w.press("enter", cx));
    assert_eq!(shown_name(&ws, cx), "Web");
    // Typing narrows the cards; Enter opens the first left.
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.input("api", cx));
    act(handle, cx, |w, cx| w.press("enter", cx));
    assert_eq!(shown_name(&ws, cx), "Api");
}

fn names(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<String> {
    ws.read_with(cx, |ws, _| {
        ws.order().iter().map(|s| s.name.clone()).collect()
    })
}

#[gpui_kit::test]
fn dropping_a_card_on_another_takes_its_place(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    three(handle, &ws, cx);
    open_home(handle, cx);
    act(handle, cx, |w, cx| {
        w.drag_to("space-card-w3", "space-card-default", cx)
    });
    assert_eq!(names(&ws, cx), ["Web", "Main", "Api"]);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.active, 0);
        assert_eq!(ws.space.name, "Web", "still shown");
    });
    let crate::session::Loaded::Saved(index) = crate::session::load_index(data.path()) else {
        panic!("no index")
    };
    let ids: Vec<&str> = index.workspaces.iter().map(|w| w.id.as_str()).collect();
    assert_eq!((ids, index.active), (vec!["w3", "default", "w2"], 0));
}

#[gpui_kit::test]
fn dropping_a_sidebar_tile_reorders_and_keeps_the_active_one(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    act(handle, cx, |w, cx| {
        w.drag_to("workspace-default", "workspace-w3", cx)
    });
    assert_eq!(names(&ws, cx), ["Api", "Web", "Main"]);
    ws.read_with(cx, |ws, _| {
        assert_eq!((ws.active, ws.space.name.as_str()), (1, "Web"));
    });
}

#[gpui_kit::test]
fn settings_opened_from_the_home_close_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.press(SETTINGS, cx));
    assert!(!up(&ws, cx), "the panel would open under it");
    assert!(panel_open(&ws, cx));
    act(handle, cx, |w, cx| w.press(SETTINGS, cx));
    settle_slide(handle, cx);
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.click("home-settings", cx));
    assert!(!up(&ws, cx) && panel_open(&ws, cx), "its gear");
}

#[gpui_kit::test]
fn a_popup_from_the_sidebar_closes_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.click("agents-button", cx));
    assert!(!up(&ws, cx));
    ws.read_with(cx, |ws, _| assert!(ws.agents_open));
    // Over the popup, the Home gives it back on Esc; another popup's button closes both.
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.click("inbox-button", cx));
    assert!(!up(&ws, cx));
    ws.read_with(cx, |ws, _| assert!(ws.inbox_open && !ws.agents_open));
}

#[gpui_kit::test]
fn closing_the_home_gives_the_keyboard_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let before = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.new_note(window, cx));
            window.render_frame(cx);
            window.focused(cx)
        })
        .unwrap();
    let canvas = ws.read_with(cx, |ws, _| ws.focus_handle.clone());
    assert!(
        before.is_some() && before.as_ref() != Some(&canvas),
        "the note's field"
    );
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.press("escape", cx));
    let after = cx
        .update_window(handle, |_, window, cx| window.focused(cx))
        .unwrap();
    assert_eq!(after, before);
}

#[gpui_kit::test]
fn no_key_reaches_the_canvas_under_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    open_home(handle, cx);
    // The keyboard left the search (a dialog that closed gave it to the canvas).
    let (canvas, search) = ws.read_with(cx, |ws, cx| {
        let search = ws.screen.as_ref().unwrap().search.read(cx).focus_handle(cx);
        (ws.focus_handle.clone(), search)
    });
    act(handle, cx, |w, cx| {
        w.focus(&canvas, cx);
        w.press("x", cx);
    });
    let focused = cx
        .update_window(handle, |_, window, cx| window.focused(cx))
        .unwrap();
    assert_eq!(focused, Some(search), "a key goes to the search");
    assert!(up(&ws, cx));
    act(handle, cx, |w, cx| {
        w.focus(&canvas, cx);
        w.press("escape", cx);
    });
    assert!(!up(&ws, cx), "its keys still run");
}

#[gpui_kit::test]
fn showing_the_workspace_shown_closes_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    open_home(handle, cx);
    let active = ws.read_with(cx, |ws, _| ws.active);
    // The API's `show`, a notification's click: the person asked for the canvas.
    show(handle, &ws, cx, active);
    assert!(!up(&ws, cx));
}

#[gpui_kit::test]
fn the_tiles_count_what_waits_and_open_it_where_it_is(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    // An agent in "Api" needs the person; one in "Web" ended its turn unseen.
    show(handle, &ws, cx, 1);
    let (needs, needs_terminal) = agent(handle, &ws, cx, 0);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(needs).unwrap().state =
            Some(AgentState::NeedsYou(Some("Allow Bash?".into())))
    });
    show(handle, &ws, cx, 2);
    let (done, done_terminal) = agent(handle, &ws, cx, 0);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(done).unwrap().state = Some(AgentState::Working)
    });
    frame(handle, cx);
    show(handle, &ws, cx, 0);
    ws.update(cx, |ws, _| {
        ws.others[1].terminal_mut(done).unwrap().state = Some(AgentState::Idle)
    });
    open_home(handle, cx);
    assert_eq!(count(&ws, cx, Wait::NeedsYou), 1);
    assert_eq!(count(&ws, cx, Wait::Done), 1);
    assert_eq!(count(&ws, cx, Wait::Requests), 0);
    assert_eq!(count(&ws, cx, Wait::Automations), 0);
    // At zero a tile takes no press.
    act(handle, cx, |w, cx| w.click("home-requests", cx));
    assert!(up(&ws, cx), "nothing waits there");

    act(handle, cx, |w, cx| w.click("home-needs-you", cx));
    assert!(!up(&ws, cx));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.name, "Api", "its workspace");
        assert!(ws.agents_open && ws.agents_show == Show::NeedsYou);
        assert_eq!(
            ws.agents_selected().map(|r| r.terminal),
            Some(needs_terminal.clone())
        );
    });

    open_home(handle, cx);
    act(handle, cx, |w, cx| w.click("home-done", cx));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.name, "Web");
        assert!(ws.agents_open && ws.agents_show == Show::Done);
        assert_eq!(
            ws.agents_selected().map(|r| r.terminal),
            Some(done_terminal)
        );
    });

    // A request of "Api"'s agent.
    let _answers = asks(handle, &ws, cx, &needs_terminal, 1);
    let key = ws.read_with(cx, |ws, _| ws.waits.last().expect("a wait").key);
    open_home(handle, cx);
    assert_eq!(count(&ws, cx, Wait::Requests), 1);
    act(handle, cx, |w, cx| w.click("home-requests", cx));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.name, "Api");
        assert!(ws.inbox_open && !up_now(ws));
        assert_eq!(ws.inbox_selected().map(|w| w.key), Some(key));
    });

    // An automation of "Web" that went wrong unseen.
    let id = ws.update(cx, |ws, _| {
        let id = ws.automations.add(crate::automations::Automation {
            workspace: Some("w3".into()),
            ..super::automations::notify("Hi")
        });
        ws.automation_runs.entry(id).or_default().unseen = true;
        id
    });
    open_home(handle, cx);
    assert_eq!(count(&ws, cx, Wait::Automations), 1);
    act(handle, cx, |w, cx| w.click("home-automations", cx));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.name, "Web");
        assert!(ws.automations_open && !up_now(ws));
        assert_eq!(ws.automation_selected(), Some(id));
    });
    assert_eq!(count(&ws, cx, Wait::Automations), 0, "seen");
}

fn up_now(ws: &Workspace) -> bool {
    ws.home_up()
}

#[gpui_kit::test]
fn a_card_says_what_its_workspace_holds(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    show(handle, &ws, cx, 1);
    let (needs, _) = agent(handle, &ws, cx, 0);
    agent(handle, &ws, cx, 1);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(needs).unwrap().state = Some(AgentState::NeedsYou(None))
    });
    show(handle, &ws, cx, 0);
    open_home(handle, cx);
    ws.read_with(cx, |ws, _| {
        let api = ws.order()[1];
        let counts = ws.counts(api);
        assert_eq!(counts.line(), "2 terminals");
        assert_eq!(counts.running().as_deref(), Some("2 Claude Code running"));
        assert_eq!(counts.needs_words().as_deref(), Some("1 needs you"));
        assert!(api.opened.is_some(), "opened, a moment ago");
        assert_eq!(ws.counts(ws.order()[2]).line(), "Empty");
    });
    act(handle, cx, |w, _| {
        assert!(w.try_find("space-card-agents-w2").is_some());
        assert!(w.try_find("space-card-agents-w3").is_none(), "no agent");
        assert!(
            w.try_find("space-card-opened-default").is_some(),
            "Open now"
        );
    });
    // Its ⋯ opens the sidebar tile's menu.
    act(handle, cx, |w, cx| w.click("space-card-menu-w2", cx));
    ws.read_with(cx, |ws, _| assert!(ws.menu.is_some()));
    assert!(up(&ws, cx), "the menu is over the Home");
}

#[gpui_kit::test]
fn opened_is_kept_in_the_index(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    three(handle, &ws, cx);
    show(handle, &ws, cx, 1);
    let crate::session::Loaded::Saved(index) = crate::session::load_index(data.path()) else {
        panic!("no index")
    };
    assert!(index.workspaces.iter().all(|w| w.opened.is_some()));
    let api = ws.read_with(cx, |ws, _| ws.space.opened);
    assert_eq!(index.workspaces[1].opened, api);
}

#[gpui_kit::test]
fn a_recent_folder_or_open_folder_starts_a_workspace_on_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("daemon-quest");
    std::fs::create_dir(&folder).unwrap();
    ws.update(cx, |ws, _| ws.space.recent = vec![folder.clone()]);
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.click(("home-recent", 0usize), cx));
    assert!(!up(&ws, cx));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.others.len(), 1, "a new workspace");
        assert_eq!(ws.space.name, "daemon-quest");
        assert_eq!(ws.space.folder.as_deref(), Some(folder.as_path()));
        assert!(ws.recent_folders().is_empty(), "now a workspace's own");
    });
    // Open folder… on it again shows that workspace.
    show(handle, &ws, cx, 0);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_folder_space(&folder, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.others.len(), 1, "no second one");
        assert_eq!(ws.space.name, "daemon-quest");
    });
}
