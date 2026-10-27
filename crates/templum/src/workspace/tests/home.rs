//! The Home (docs/specs/home.md): Templum's first screen, in the canvas's place; leaving it for a
//! workspace, its search, its tiles and their presses, its cards, recent folders and its keys.

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
#[cfg(target_os = "macos")]
const CTRL_1: &str = "cmd-1";
#[cfg(not(target_os = "macos"))]
const CTRL_1: &str = "ctrl-1";

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

/// Shows the Home with its command.
fn open_home(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    act(handle, cx, |w, cx| w.press(WORKSPACES, cx));
}

/// The sidebar marks the button `id` as what is shown.
fn marked(handle: AnyWindowHandle, cx: &mut TestAppContext, id: &str) -> bool {
    let id = id.to_string();
    cx.update_window(handle, |_, window, _| {
        window.within(id).try_find("shown-bar").is_some()
    })
    .unwrap()
}

fn focused(handle: AnyWindowHandle, cx: &mut TestAppContext) -> Option<gpui_kit::FocusHandle> {
    cx.update_window(handle, |_, window, cx| window.focused(cx))
        .unwrap()
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
fn templum_starts_on_the_home_with_no_canvas_drawn(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    three(handle, &ws, cx);
    show(handle, &ws, cx, 1);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open(Rect::new(0.0, 0.0, 600.0, 400.0), window, cx)
        })
    })
    .unwrap();
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let (handle, again) = open_welcomed(cx, Some(quiet()), None, Some(data.path().to_path_buf()));
    assert!(up(&again, cx), "the Home, at launch");
    let (id, size) = again.read_with(cx, |ws, _| {
        assert!(ws.welcome.is_none());
        assert_eq!(ws.space.name, "Api", "the one shown last, held under it");
        let (id, pane) = ws.space.terminals().next().expect("its terminal, started");
        assert!(!pane.term.exited());
        (id, pane.term.grid_size())
    });
    act(handle, cx, |w, _| {
        assert!(w.try_find("screen").is_some());
        assert!(w.try_find("workspace").is_none(), "no canvas");
        assert!(
            w.try_find(("block", id.0 as usize)).is_none(),
            "nor its blocks"
        );
        assert!(w.try_find("toolbar").is_none());
        assert!(
            w.try_find("agent-pill").is_none(),
            "the Templum Agent's pill"
        );
        assert!(w.try_find("zoom-level").is_none());
        assert!(
            w.try_find("title-home").is_some(),
            "the title bar says Home"
        );
    });
    assert!(
        marked(handle, cx, "home-button"),
        "the sidebar marks the Home"
    );
    for tile in ["workspace-default", "workspace-w2", "workspace-w3"] {
        assert!(!marked(handle, cx, tile), "and no workspace: {tile}");
    }
    // A place, not a popup: Esc stays on it.
    act(handle, cx, |w, cx| w.press("escape", cx));
    assert!(up(&again, cx), "Esc");
    again.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(id).unwrap();
        assert!(!pane.term.exited(), "its terminal ran on");
        assert_eq!(pane.term.grid_size(), size, "at its size");
    });

    act(handle, cx, |w, cx| w.click("space-card-w2", cx));
    assert!(!up(&again, cx), "its card shows it");
    assert_eq!(shown_name(&again, cx), "Api");
    act(handle, cx, |w, _| {
        assert!(w.try_find("workspace").is_some(), "its canvas");
        assert!(w.try_find(("block", id.0 as usize)).is_some());
        assert!(w.try_find("toolbar").is_some() && w.try_find("zoom-level").is_some());
        assert!(w.try_find("title-home").is_none(), "its name");
    });
    assert!(marked(handle, cx, "workspace-w2") && !marked(handle, cx, "home-button"));
    let canvas = again.read_with(cx, |ws, _| ws.focus_handle.clone());
    assert_eq!(
        focused(handle, cx),
        Some(canvas),
        "the canvas has the keyboard"
    );
}

#[gpui_kit::test]
fn the_api_says_no_workspace_is_shown_on_the_home(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    frame(handle, cx);
    let state = ws.read_with(cx, |ws, cx| ws.api_state(cx));
    assert_eq!(state.app.shown.as_deref(), Some("w3"));
    assert!(state.view.is_some());
    open_home(handle, cx);
    let state = ws.read_with(cx, |ws, cx| ws.api_state(cx));
    assert_eq!(state.app.shown, None, "the Home");
    assert!(state.workspaces.iter().all(|w| !w.shown));
    assert!(state.view.is_none(), "no canvas, no camera");
    // `show` leaves it, for the workspace held under it too.
    show(handle, &ws, cx, 2);
    let state = ws.read_with(cx, |ws, cx| ws.api_state(cx));
    assert!(!up(&ws, cx));
    assert_eq!(state.app.shown.as_deref(), Some("w3"));
    assert!(state.workspaces[2].shown);
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
fn the_button_the_name_and_the_command_show_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    act(handle, cx, |w, cx| w.click("home-button", cx));
    assert!(up(&ws, cx), "the sidebar's Home button");
    act(handle, cx, |w, cx| w.click("home-button", cx));
    assert!(up(&ws, cx), "the button again: still on the Home");
    act(handle, cx, |w, cx| w.click("workspace-w2", cx));
    assert!(!up(&ws, cx), "a sidebar tile shows its workspace");
    assert_eq!(shown_name(&ws, cx), "Api");
    act(handle, cx, |w, cx| w.click("workspace-name", cx));
    assert!(up(&ws, cx), "the name in the title bar");
    act(handle, cx, |w, cx| w.press(CTRL_1, cx));
    assert!(!up(&ws, cx), "Ctrl+1");
    assert_eq!(shown_name(&ws, cx), "Main");
    act(handle, cx, |w, cx| w.press(WORKSPACES, cx));
    assert!(up(&ws, cx), "the command");
    act(handle, cx, |w, cx| w.press(WORKSPACES, cx));
    assert!(up(&ws, cx), "the command again, from the search field");
}

#[gpui_kit::test]
fn next_and_previous_go_from_the_home_to_the_first_and_the_last(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    show(handle, &ws, cx, 1);
    open_home(handle, cx);
    let step = |ws: &Entity<Workspace>, cx: &mut TestAppContext, by: i32| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.cycle_space(by, window, cx))
        })
        .unwrap();
    };
    step(&ws, cx, 1);
    assert_eq!(shown_name(&ws, cx), "Main", "the Home is above the first");
    open_home(handle, cx);
    step(&ws, cx, -1);
    assert_eq!(shown_name(&ws, cx), "Web");
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
    act(handle, cx, |w, cx| {
        assert!(
            w.try_find("space-card-w3").is_some(),
            "Esc empties the search"
        );
        w.input("storefr", cx);
    });
    act(handle, cx, |w, _| {
        assert!(w.try_find("space-card-w3").is_some(), "its folder");
        assert!(w.try_find("space-card-w2").is_none());
    });
}

#[gpui_kit::test]
fn clicking_a_card_shows_its_workspace(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.click("space-card-default", cx));
    assert!(!up(&ws, cx));
    assert_eq!(shown_name(&ws, cx), "Main");
    // The one held under it is shown too.
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.click("space-card-default", cx));
    assert!(!up(&ws, cx), "the workspace shown last");
    assert_eq!(shown_name(&ws, cx), "Main");
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
fn settings_open_over_the_home(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.press(SETTINGS, cx));
    assert!(up(&ws, cx) && panel_open(&ws, cx));
    act(handle, cx, |w, cx| w.press(SETTINGS, cx));
    settle_slide(handle, cx);
    act(handle, cx, |w, cx| w.click("home-settings", cx));
    assert!(up(&ws, cx) && panel_open(&ws, cx), "its gear");
}

#[gpui_kit::test]
fn the_sidebar_opens_every_workspaces_popups_over_the_home_and_a_workspaces_in_it(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    show(handle, &ws, cx, 1);
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.click("agents-button", cx));
    assert!(up(&ws, cx), "Agents, of every workspace, over the Home");
    ws.read_with(cx, |ws, _| assert!(ws.agents_open));
    act(handle, cx, |w, cx| w.click("automations-button", cx));
    assert!(up(&ws, cx), "Automations too");
    ws.read_with(cx, |ws, _| assert!(ws.automations_open && !ws.agents_open));
    // The requests are a workspace's: with none waiting, the one held under the Home's.
    act(handle, cx, |w, cx| w.click("inbox-button", cx));
    assert!(!up(&ws, cx));
    ws.read_with(cx, |ws, _| {
        assert!(ws.inbox_open && !ws.automations_open);
        assert_eq!(ws.space.name, "Api");
    });
    // Back on the Home, the workspace's popups are gone with it.
    open_home(handle, cx);
    ws.read_with(cx, |ws, _| assert!(!ws.inbox_open && !ws.git_open));
    let folder = tempfile::tempdir().unwrap();
    ws.update(cx, |ws, _| {
        ws.space.folder = Some(folder.path().to_path_buf())
    });
    act(handle, cx, |w, cx| w.click("git-button", cx));
    assert!(!up(&ws, cx), "Git, in the workspace held");
    assert_eq!(shown_name(&ws, cx), "Api");
}

#[gpui_kit::test]
fn a_key_on_the_home_never_reaches_the_canvas(cx: &mut TestAppContext) {
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
    assert_eq!(
        focused(handle, cx),
        Some(search),
        "a key goes to the search"
    );
    assert!(up(&ws, cx));
    act(handle, cx, |w, cx| {
        w.focus(&canvas, cx);
        w.press("enter", cx);
    });
    assert!(
        !up(&ws, cx),
        "its keys still run: Enter opens the first card"
    );
}

#[gpui_kit::test]
fn showing_the_workspace_held_leaves_the_home(cx: &mut TestAppContext) {
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

    // The Agents popup, of every workspace, stays open over the Home: closed, it covers no tile.
    open_home(handle, cx);
    act(handle, cx, |w, cx| w.click("agents-button", cx));
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
    act(handle, cx, |w, cx| w.click("agents-button", cx));
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
            "Opened just now"
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

#[gpui_kit::test]
fn reviewing_the_templum_agent_from_the_home_shows_the_workspace_held(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    open_home(handle, cx);
    // The click of the notification that says what it asks: the chat is on a canvas.
    act(handle, cx, |w, cx| {
        ws.update(cx, |ws, cx| ws.review_agent(w, cx))
    });
    assert!(!up(&ws, cx));
    ws.read_with(cx, |ws, _| assert!(ws.templum_agent.open));
}

#[gpui_kit::test]
fn deleting_the_workspace_held_keeps_the_home(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three(handle, &ws, cx);
    open_home(handle, cx);
    // Its card's menu: Delete….
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.delete_space("w3", window, cx))
    })
    .unwrap();
    assert!(up(&ws, cx), "still on the Home");
    assert_eq!(shown_name(&ws, cx), "Api", "the one before, held under it");
    assert_eq!(names(&ws, cx), ["Main", "Api"]);
}
