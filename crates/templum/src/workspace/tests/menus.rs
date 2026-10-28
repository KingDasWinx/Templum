//! The title bar, the toolbar and the canvas menus.

use super::*;
use templum_terminal::ShellOption;

#[gpui_kit::test]
fn title_bar_buttons_reset_zoom_and_open_terminals(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(ZOOM_IN, cx);
        window.render_frame(cx);
        window.click("zoom-level", cx);
        window.render_frame(cx);
        window.click("tool-terminal", cx);
        window.render_frame(cx);
        // The shells' menu: Default.
        window.within("popup-menu").click(0usize, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.canvas.camera.zoom(), 1.0);
        assert_eq!(ws.space.canvas.blocks().len(), 2);
    });
}

#[gpui_kit::test]
fn the_toolbar_adds_blocks_and_the_title_bar_lost_its_add_buttons(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find("toolbar");
        assert!(
            window.try_find("new-terminal").is_none(),
            "no + in the title bar"
        );
        assert!(window.try_find("new-kanban").is_none());
        window.click("tool-terminal", cx);
        window.render_frame(cx);
        window.within("popup-menu").click(0usize, cx);
    })
    .unwrap();
    // The choice closes the menu once the update ends; the next clicks reach the toolbar.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("tool-note", cx);
        window.render_frame(cx);
        window.click("tool-kanban", cx);
        window.render_frame(cx);
        window.find("tool-editor");
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.terminals().count(), 2);
        let kinds = |f: fn(&crate::workspace::Body) -> bool| {
            ws.space.blocks.values().filter(|b| f(b)).count()
        };
        assert_eq!(kinds(|b| b.note().is_some()), 1);
        assert_eq!(kinds(|b| b.kanban().is_some()), 1);
        assert_eq!(
            ws.tool,
            crate::workspace::Tool::Select,
            "block tools don't stay on"
        );
    });
}

/// A made-up list (docs/specs/shells.md, The menus): a shell named as the default, left out, then
/// one that runs the tests' quiet program. The menus show Default (zsh), —, Quiet.
fn made_up_shells(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> ShellOption {
    let (program, args) = quiet();
    let quiet_shell = ShellOption {
        label: "Quiet".into(),
        program,
        args,
    };
    ws.update(cx, |ws, _| {
        ws.config.shell.program = None;
        ws.default_shell = "zsh".into();
        ws.shells = vec![
            ShellOption {
                label: "zsh".into(),
                program: "/nonexistent/zsh".into(),
                args: vec![],
            },
            quiet_shell.clone(),
        ];
    });
    quiet_shell
}

#[gpui_kit::test]
fn the_toolbar_terminal_lists_the_shells_and_opens_the_one_chosen(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let quiet_shell = made_up_shells(&ws, cx);
    let terminals =
        |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.space.terminals().count());
    let before = terminals(cx);
    let open_menu = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("tool-terminal", cx);
            window.render_frame(cx);
        })
        .unwrap()
    };
    // Esc closes it: no terminal opens.
    open_menu(cx);
    assert!(
        ws.read_with(cx, |ws, _| ws.menu.is_some()),
        "the menu opens"
    );
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(ws.read_with(cx, |ws, _| ws.menu.is_none()));
    assert_eq!(terminals(cx), before);
    // Default (zsh), —, Quiet: the third item is the shell.
    open_menu(cx);
    cx.update_window(handle, |_, window, cx| {
        window.within("popup-menu").click(2usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.terminals().count(), before + 1);
        let top = ws.space.canvas.top().unwrap();
        assert_eq!(ws.space.terminal(top).unwrap().shell, Some(quiet_shell));
        assert!(ws.menu.is_none(), "a choice closes it");
    });
    // Default: a terminal with no shell of its own.
    open_menu(cx);
    cx.update_window(handle, |_, window, cx| {
        window.within("popup-menu").click(0usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.terminals().count(), before + 2);
        let top = ws.space.canvas.top().unwrap();
        assert_eq!(ws.space.terminal(top).unwrap().shell, None);
    });
}

/// A list read before the login shell's PATH arrived is read again by the next menu.
#[gpui_kit::test]
fn a_menu_reads_an_early_shell_list_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        ws.shells = Vec::new();
        ws.shells_early = true;
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("tool-terminal", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.shells, templum_terminal::available_shells());
        assert!(!ws.shells_early);
    });
}

#[gpui_kit::test]
fn add_terminal_lists_the_shells_and_opens_the_one_chosen_where_clicked(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let quiet_shell = made_up_shells(&ws, cx);
    let world = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let o = ws.read(cx).origin;
            let at = (f32::from(o.x) + 20.0, f32::from(o.y) + 600.0);
            right_click(window, cx, at);
            window.render_frame(cx);
            let ws = ws.read(cx);
            ws.space
                .canvas
                .camera
                .to_world(ws.local(point(px(at.0), px(at.1))))
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        // AI agent ▸, Add ▸ Terminal ▸ (Default, —, Quiet).
        window.within("popup-menu").click(1usize, cx);
        window.render_frame(cx);
        window
            .within("submenu")
            .within("popup-menu")
            .click(0usize, cx);
        window.render_frame(cx);
        click_innermost_menu_item(window, cx, 2);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let top = ws.space.canvas.top().unwrap();
        assert_eq!(ws.space.canvas.get(top).unwrap().rect.origin(), world);
        assert_eq!(ws.space.terminal(top).unwrap().shell, Some(quiet_shell));
        assert!(ws.menu.is_none(), "a choice closes it");
    });
}

#[gpui_kit::test]
fn default_names_the_shell_new_terminals_run(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    ws.update(cx, |ws, _| {
        ws.default_shell = "zsh".into();
        ws.shells = vec![ShellOption {
            label: "PowerShell 7".into(),
            program: "C:/pwsh/pwsh.exe".into(),
            args: vec!["-NoLogo".into()],
        }];
        ws.config.shell.program = None;
        assert_eq!(ws.default_shell_name(), "zsh");
        ws.config.shell.program = Some("C:/pwsh/pwsh.exe".into());
        ws.config.shell.args = vec!["-NoLogo".into()];
        assert_eq!(ws.default_shell_name(), "PowerShell 7");
        ws.config.shell.args.clear();
        assert_eq!(
            ws.default_shell_name(),
            "pwsh",
            "other arguments: its file name"
        );
        ws.config.shell.program = Some("/opt/fish/bin/fish".into());
        assert_eq!(ws.default_shell_name(), "fish");
    });
}

#[gpui_kit::test]
fn the_toolbar_agent_button_lists_the_agents(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("tool-agent", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.menu.is_some(), "the agents menu is open")
    });
}

#[gpui_kit::test]
fn the_canvas_menu_adds_blocks_where_it_was_opened(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    // Left of the startup terminal, then right of it: empty canvas both times.
    for (item, at) in [(0usize, (20.0, 600.0)), (1, (1100.0, 20.0))] {
        let world = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let o = ws.read(cx).origin;
                let at = (f32::from(o.x) + at.0, f32::from(o.y) + at.1);
                right_click(window, cx, at);
                window.render_frame(cx);
                assert!(ws.read(cx).menu.is_some(), "the menu opened");
                let ws = ws.read(cx);
                ws.space
                    .canvas
                    .camera
                    .to_world(ws.local(point(px(at.0), px(at.1))))
            })
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            // AI agent ▸, Add ▸ Terminal ▸ Default, or Kanban board.
            window.within("popup-menu").click(1usize, cx);
            window.render_frame(cx);
            window
                .within("submenu")
                .within("popup-menu")
                .click(item, cx);
            window.render_frame(cx);
            if item == 0 {
                click_innermost_menu_item(window, cx, 0);
                window.render_frame(cx);
            }
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            let top = ws.space.canvas.top().unwrap();
            assert_eq!(ws.space.canvas.get(top).unwrap().rect.origin(), world);
            assert_eq!(ws.space.kanban(top).is_some(), item == 1);
            assert!(ws.menu.is_none(), "a choice closes it");
        });
    }
}

#[gpui_kit::test]
fn the_canvas_menu_opens_an_agent_in_a_recent_folder(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let folder = tempfile::tempdir().unwrap();
    ws.update(cx, |ws, _| {
        ws.space.recent = vec![folder.path().to_path_buf()];
        // One agent, installed everywhere: the menu's first, whatever this computer has.
        ws.agents = vec![crate::agents::AgentEntry {
            id: "mine".into(),
            title: "Mine".into(),
            command: quiet().0,
            resume: None,
            install: None,
            builtin: None,
            changed: false,
            show: true,
            generate: None,
            models: Vec::new(),
            shell: None,
            account: None,
        }];
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let o = ws.read(cx).origin;
        right_click(window, cx, (f32::from(o.x) + 20.0, f32::from(o.y) + 600.0));
        window.render_frame(cx);
        // AI agent ▸ its first agent.
        window.within("popup-menu").click(0usize, cx);
        window.render_frame(cx);
        window
            .within("submenu")
            .within("popup-menu")
            .click(0usize, cx);
        window.render_frame(cx);
        assert!(
            window.try_find("choose-folder").is_some(),
            "the folder dialog"
        );
    })
    .unwrap();
    // The dialog slides in for 0.25 s first: presses meanwhile are lost.
    std::thread::sleep(std::time::Duration::from_millis(300));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("recent-0", cx);
        window.render_frame(cx);
        assert!(window.try_find("choose-folder").is_none(), "it closed");
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.terminals().count(), 2, "the agent's terminal");
        assert_eq!(ws.space.recent, [folder.path()]);
    });
}

#[gpui_kit::test]
fn the_innermost_menu_opens_and_acts_on_what_was_clicked(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let (block, board, card) = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.add_kanban_at(Point::new(900.0, 0.0), window, cx);
                let block = ws.space.canvas.top().unwrap();
                ws.new_board(block, cx);
                let board = ws.space.kanban(block).unwrap().board.unwrap();
                let todo = ws.boards.board(board).unwrap().columns[0].id;
                let card = ws.boards.add_card(board, todo).unwrap();
                ws.space.appearing.clear();
                (block, board, card)
            })
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let card_el = gpui_kit::ElementId::Name(format!("card-{}-{card}", block.0).into());
        window.right_click(card_el, cx);
        window.render_frame(cx);
        // Open, Send to ▸, —, Delete card: the card's menu, not its block's (All boards…, —,
        // Close). It asks first.
        window.within("popup-menu").click(3usize, cx);
    })
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
        let ws = ws.read(cx);
        assert!(ws.boards.card(board, card).is_none(), "the card went");
        assert!(ws.space.kanban(block).is_some(), "its block stayed");
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("body", terminal.0 as usize), cx);
        window.render_frame(cx);
        // A second right-click replaces the menu.
        window.right_click(("body", terminal.0 as usize), cx);
        window.render_frame(cx);
        // Copy, Paste, —, Fullscreen, Connect to ▸, Move to workspace ▸, Close terminal.
        window.within("popup-menu").click(6usize, cx);
        window.render_frame(cx);
        assert!(
            !ws.read(cx).space.terminal(terminal).is_some(),
            "the terminal closed"
        );
    })
    .unwrap();
}

/// The header's ⋯ opens the menu the right-click opens, for whoever doesn't know the right-click.
#[gpui_kit::test]
fn the_headers_more_button_opens_the_blocks_menu(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let n = terminal.0 as usize;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find(("more", n)).is_none(),
            "hidden until hovered"
        );
        window.hover(("block", n), cx);
        window.render_frame(cx);
        window.click(("more", n), cx);
        window.render_frame(cx);
        assert!(ws.read(cx).menu.is_some(), "the menu opened");
        // Copy, Paste, —, Fullscreen, Move to workspace ▸, Close terminal: the terminal's, as
        // the right-click's (no Connect to ▸ with no other block).
        window.within("popup-menu").click(5usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(terminal).is_none(), "the terminal closed");
        assert!(ws.menu.is_none(), "a choice closes it");
    });
}

#[test]
fn a_terminals_label_names_its_agent_title_and_folder_once() {
    use crate::workspace::blocks::label;
    let folder = std::path::Path::new("/code/Templum");
    assert_eq!(
        label(Some("Claude Code"), "✳ Fix tests", Some(folder)),
        "Claude Code · ✳ Fix tests · Templum"
    );
    assert_eq!(label(None, "Templum", Some(folder)), "Templum");
    assert_eq!(label(None, "", None), "");
}

#[gpui_kit::test]
fn send_to_lists_the_terminals_running_an_agent_first(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let second = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_shell(None, Some(Point::new(900.0, 0.0)), window, cx);
                let id = ws.space.canvas.top().unwrap();
                ws.space.terminal_mut(id).unwrap().seen.agent = Some("claude".into());
                id
            })
        })
        .unwrap();
    ws.read_with(cx, |ws, _| {
        let list = ws.terminal_list(None);
        assert_eq!(
            list.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [second, first]
        );
        assert!(list[0].1.starts_with("Claude Code"), "{list:?}");
        // Only those working in a folder.
        let nowhere = std::path::Path::new("/nowhere");
        assert!(ws.terminal_list(Some(nowhere)).is_empty());
    });
}

// A mirror's menus: what home can do, and the rule's words for what it can't.

use super::remote::{apply, live_terminals, sent, terminals_of_home, viewer};
use crate::menu::Target;
use serde_json::json;

#[gpui_kit::test]
fn a_remote_s_menus_open_with_what_works_there(cx: &mut TestAppContext) {
    use std::sync::Arc;
    let (handle, ws, _dir) = live_terminals(cx, &["b1"]);
    let mut state = terminals_of_home(&["b1"]);
    state.blocks.push(Arc::new(templum_api::model::BlockView {
        id: "k1".into(),
        workspace: "w1".into(),
        kind: "kanban".into(),
        title: "Sprint".into(),
        z: 1,
        rect: templum_api::model::Rect {
            x: 500.0,
            y: 0.0,
            w: 400.0,
            h: 300.0,
        },
        ..Default::default()
    }));
    apply(handle, &ws, cx, state);
    let (v1, k1) = (viewer(&ws, cx, "b1"), viewer(&ws, cx, "k1"));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let at = point(px(100.0), px(100.0));
            ws.open_menu(Target::Canvas(Point::new(0.0, 0.0)), at, window, cx);
            assert!(ws.menu.is_some(), "the canvas menu");
            assert!(ws.said.is_empty());
            ws.menu = None;
            ws.open_block_menu(v1, at, window, cx);
            assert!(ws.menu.is_some(), "a terminal's menu");
            ws.menu = None;
            ws.open_block_menu(k1, at, window, cx);
            assert!(ws.menu.is_some(), "a card of home's has a menu");
            ws.menu = None;
            ws.open_menu(Target::Ink(0), at, window, cx);
            assert!(ws.menu.is_none());
            assert_eq!(
                ws.said.last().map(String::as_str),
                Some(crate::workspace::remote::NOT_YET)
            );
        })
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_canvas_menu_on_a_remote_adds_at_home_where_clicked(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_terminals(cx, &["b1"]);
    let adds = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        sent(ws, cx)
            .into_iter()
            .filter(|(tool, _)| tool == "add_block")
            .map(|(_, args)| args)
            .collect::<Vec<_>>()
    };
    let at = (20.0, 600.0);
    let world = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let o = ws.read(cx).origin;
            let at = (f32::from(o.x) + at.0, f32::from(o.y) + at.1);
            right_click(window, cx, at);
            window.render_frame(cx);
            assert!(ws.read(cx).menu.is_some(), "the menu opened");
            let ws = ws.read(cx);
            ws.space
                .canvas
                .camera
                .to_world(ws.local(point(px(at.0), px(at.1))))
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        // AI agent ▸, Add ▸ Terminal, Kanban board, Note, Checklist, Editor.
        window.within("popup-menu").click(1usize, cx);
        window.render_frame(cx);
        window
            .within("submenu")
            .within("popup-menu")
            .click(1usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        adds(&ws, cx),
        [json!({"workspace": "w1", "kind": "kanban", "x": world.x, "y": world.y})]
    );
    // Note is home's too.
    cx.update_window(handle, |_, window, cx| {
        right_click(
            window,
            cx,
            (
                f32::from(ws.read(cx).origin.x) + 20.0,
                f32::from(ws.read(cx).origin.y) + 600.0,
            ),
        );
        window.render_frame(cx);
        window.within("popup-menu").click(1usize, cx);
        window.render_frame(cx);
        window
            .within("submenu")
            .within("popup-menu")
            .click(2usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(adds(&ws, cx).len(), 2);
    assert_eq!(adds(&ws, cx)[1]["kind"], "note");
    // Terminal ▸ lists home's shells: Default is home's default.
    cx.update_window(handle, |_, window, cx| {
        right_click(
            window,
            cx,
            (
                f32::from(ws.read(cx).origin.x) + 20.0,
                f32::from(ws.read(cx).origin.y) + 600.0,
            ),
        );
        window.render_frame(cx);
        window.within("popup-menu").click(1usize, cx);
        window.render_frame(cx);
        window
            .within("submenu")
            .within("popup-menu")
            .click(0usize, cx);
        window.render_frame(cx);
        click_innermost_menu_item(window, cx, 0);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(adds(&ws, cx).len(), 3);
    assert_eq!(adds(&ws, cx)[2]["kind"], json!("terminal"));
    assert!(adds(&ws, cx)[2].get("shell").is_none(), "home's default");
}

#[gpui_kit::test]
fn hand_a_task_is_in_a_remote_agent_s_menu(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    use templum_api::model::AgentView;
    let (handle, ws, _dir) = super::remote::offline(cx);
    super::remote::show_remote(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        super::remote::live(ws, templum_api::store::Scope::Control);
        ws.space.remote.as_mut().unwrap().entry.home = Some("w1".into());
    });
    let agent = AgentView {
        agent: "Claude Code".into(),
        state: "idle".into(),
        ..AgentView::default()
    };
    apply(
        handle,
        &ws,
        cx,
        super::remote::terminal_of_home(Some(agent)),
    );
    let v = viewer(&ws, cx, "abcde");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_block_menu(v, point(px(100.0), px(100.0)), window, cx)
        });
        window.render_frame(cx);
        // Copy, Paste, Hand a task….
        window.within("popup-menu").click(2usize, cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
    })
    .unwrap();
    // A terminal without an agent has no such item.
    let (handle, ws, _dir) = live_terminals(cx, &["b1"]);
    let v = viewer(&ws, cx, "b1");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_block_menu(v, point(px(100.0), px(100.0)), window, cx)
        });
        window.render_frame(cx);
        window.within("popup-menu").click(2usize, cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_shortcut_a_remote_can_t_do_says_why_once(cx: &mut TestAppContext) {
    use crate::keys::Command;
    let (handle, ws, _dir) = live_terminals(cx, &["b1"]);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.run(Command::NewNote, window, cx);
            ws.run(Command::NewNote, window, cx);
            ws.run(Command::NewEditor, window, cx);
            ws.run(Command::NewEditor, window, cx);
            ws.run(Command::NewTerminal, window, cx);
        })
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(
            !ws.said.iter().any(|s| s.contains("read-only")),
            "a note is home's now: nothing is said of it"
        );
        let editors = ws
            .said
            .iter()
            .filter(|s| s.as_str() == crate::workspace::remote::NOT_YET)
            .count();
        assert_eq!(editors, 1);
    });
    let adds: Vec<_> = sent(&ws, cx)
        .into_iter()
        .filter(|(tool, args)| tool == "add_block" && args["kind"] == json!("terminal"))
        .collect();
    assert_eq!(adds.len(), 1, "a terminal is home's to add");
    let notes = sent(&ws, cx)
        .into_iter()
        .filter(|(tool, args)| tool == "add_block" && args["kind"] == json!("note"))
        .count();
    assert_eq!(notes, 2, "so is a note, once for each key");
}
