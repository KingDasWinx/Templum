//! Agents' session usage and plan limits (docs/specs/usage.md).

use super::*;
use crate::workspace::usage::{CLAUDE_EVERY, CLAUDE_MIN};

/// The Claude Code and Codex Templum types get its server and hooks; Claude Code its status line
/// too (docs/specs/connections.md, What Templum types into each agent).
#[gpui_kit::test]
fn the_agents_templum_opens_get_its_server_hooks_and_status_line(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    ws.update(cx, |ws, _| {
        let agent = |id: &str| ws.agents.iter().find(|a| a.id == id).cloned();
        let (claude, codex, copilot) = (agent("claude"), agent("codex"), agent("copilot"));
        let qwen = agent("qwen");
        let pi = agent("pi");
        let typed = |name: &str| {
            templum_usage::claude::typed_path(
                &data.path().join(name).to_string_lossy(),
                cfg!(windows),
            )
            .unwrap()
        };
        let (settings, mcp) = (typed("claude-statusline.json"), typed("mcp.json"));
        assert_eq!(
            ws.typed_in(None, claude.as_ref(), "claude --continue"),
            format!("claude --continue --settings {settings} --mcp-config {mcp}")
        );
        let written = std::fs::read_to_string(data.path().join("claude-statusline.json")).unwrap();
        assert!(
            written.contains("--claude-statusline") && written.contains(" hook claude"),
            "{written}"
        );
        let server: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(data.path().join("mcp.json")).unwrap())
                .unwrap();
        assert_eq!(
            server["mcpServers"]["templum"]["args"],
            serde_json::json!(["mcp"])
        );
        #[cfg(unix)]
        assert!(
            ws.typed_in(None, codex.as_ref(), "codex resume 0d5e")
                .starts_with("codex resume 0d5e -c 'mcp_servers.templum.command="),
            "{}",
            ws.typed_in(None, codex.as_ref(), "codex resume 0d5e")
        );
        #[cfg(unix)]
        {
            let line = ws.typed_in(None, codex.as_ref(), "codex");
            let stop = r#"-c 'hooks.Stop=[{hooks=[{type="command",command="templum hook codex",timeout=5}]}]'"#;
            assert!(line.contains(stop) && !line.contains("notify"), "{line}");
            // No `<data>/bin` on the terminal's PATH: the server, not hooks running a `templum`
            // it can't find.
            let env = ws.bus_env.clone();
            ws.bus_env.retain(|(k, _)| k != "PATH");
            let line = ws.typed_in(None, codex.as_ref(), "codex");
            assert!(
                line.contains("mcp_servers.templum") && !line.contains("hooks."),
                "{line}"
            );
            ws.bus_env = env;
        }
        assert_eq!(ws.typed_in(None, None, "claude"), "claude");
        // Copilot CLI: its server, its instructions and its plugin of hooks, in every shell.
        let words = templum_usage::copilot::args(data.path(), cfg!(windows))
            .unwrap()
            .iter()
            .map(|w| templum_usage::claude::typed_path(w, cfg!(windows)).unwrap())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(words.contains("--plugin-dir="), "{words}");
        // `(` is typed in double quotes in every shell.
        assert!(
            words.contains(" \"--allow-tool=templum(reply)\" "),
            "{words}"
        );
        assert_eq!(
            ws.typed_in(None, copilot.as_ref(), "copilot --session-id 0d5e"),
            format!("copilot --session-id 0d5e {words}")
        );
        // Qwen Code: its server, its one `--mcp-config` last.
        let qwen_words = templum_usage::qwen::args(data.path(), cfg!(windows)).unwrap();
        let qwen_mcp = templum_usage::claude::typed_path(&qwen_words[0], cfg!(windows)).unwrap();
        assert_eq!(
            ws.typed_in(None, qwen.as_ref(), "qwen --resume 0d5e"),
            format!("qwen --resume 0d5e {qwen_mcp}")
        );
        // Pi: its extension, `-e` and the file.
        let pi_words = templum_usage::pi::args(data.path(), cfg!(windows)).unwrap();
        let file = templum_usage::claude::typed_path(&pi_words[1], cfg!(windows)).unwrap();
        assert_eq!(
            ws.typed_in(None, pi.as_ref(), "pi --session-id 0d5e"),
            format!("pi --session-id 0d5e -e {file}")
        );
        // Without `<data>/bin` on the terminal's PATH, the files' program can't be found.
        let env = ws.bus_env.clone();
        ws.bus_env.retain(|(k, _)| k != "PATH");
        assert_eq!(ws.typed_in(None, pi.as_ref(), "pi"), "pi");
        assert_eq!(ws.typed_in(None, copilot.as_ref(), "copilot"), "copilot");
        assert_eq!(ws.typed_in(None, qwen.as_ref(), "qwen"), "qwen");
        ws.bus_env = env;
        // Windows PowerShell 5.1 passes `args=["mcp"]` on as `args=[mcp]`; pwsh 7 doesn't.
        let shell = ws.config.shell.program.clone();
        ws.config.shell.program = Some("powershell".into());
        assert_eq!(ws.typed_in(None, codex.as_ref(), "codex"), "codex");
        // Windows names its programs in any case.
        for program in ["PowerShell.exe", "CMD.EXE"] {
            ws.config.shell.program = Some(program.into());
            assert_eq!(ws.typed_in(None, codex.as_ref(), "codex"), "codex", "{program}");
        }
        ws.config.shell.program = Some("WSL.exe".into());
        assert_eq!(ws.typed_in(None, claude.as_ref(), "claude"), "claude");
        assert_eq!(ws.typed_in(None, copilot.as_ref(), "copilot"), "copilot");
        ws.config.shell.program = Some("powershell".into());
        assert!(ws.typed_in(None, copilot.as_ref(), "copilot").contains("--plugin-dir="));
        ws.config.shell.program = Some("pwsh".into());
        assert!(
            ws.typed_in(None, codex.as_ref(), "codex")
                .contains("mcp_servers.templum")
        );
        ws.config.shell.program = shell;
        // Connections off: the status line only, and nothing for Codex.
        ws.config.connections.enabled = false;
        assert_eq!(
            ws.typed_in(None, claude.as_ref(), "claude"),
            format!("claude --settings {settings}")
        );
        assert_eq!(ws.typed_in(None, codex.as_ref(), "codex"), "codex");
        assert_eq!(ws.typed_in(None, copilot.as_ref(), "copilot"), "copilot");
        assert_eq!(ws.typed_in(None, qwen.as_ref(), "qwen"), "qwen");
        ws.config.usage.claude_limits = false;
        assert_eq!(ws.typed_in(None, claude.as_ref(), "claude"), "claude");
        // Connections without the status line: the hooks still need the settings file.
        ws.config.connections.enabled = true;
        assert_eq!(
            ws.typed_in(None, claude.as_ref(), "claude"),
            format!("claude --settings {settings} --mcp-config {mcp}")
        );
        // A bus that couldn't start: no server, no hooks.
        let bus = ws.bus.take();
        assert_eq!(ws.typed_in(None, claude.as_ref(), "claude"), "claude");
        ws.bus = bus;
        // Without a data folder, nothing would read what the relay writes.
        ws.data_dir = None;
        assert_eq!(ws.typed_in(None, claude.as_ref(), "claude"), "claude");
    });
}

/// Every new terminal carries what Templum types after `claude` and `codex`, as raw words, for
/// the ones a person types there; empty when there is nothing to give (docs/specs/connections.md,
/// Agents typed in a terminal).
#[gpui_kit::test]
fn new_terminals_carry_what_templum_types_after_claude_and_codex(cx: &mut TestAppContext) {
    let parent = tempfile::tempdir().unwrap();
    let data = parent.path().join("Templum Data");
    std::fs::create_dir(&data).unwrap();
    let (_, ws) = open_in(cx, None, Some(data.clone()));
    ws.update(cx, |ws, _| {
        let word = |ws: &Workspace, key: &str| {
            let env = ws.agent_env(None);
            env.into_iter().find(|(k, _)| k == key).unwrap().1
        };
        let p = |name: &str| {
            let path = data.join(name).to_string_lossy().into_owned();
            if cfg!(windows) {
                path.replace('\\', "/")
            } else {
                path
            }
        };
        let claude = format!(
            "--settings\n{}\n--mcp-config={}",
            p("claude-statusline.json"),
            p("mcp.json")
        );
        assert_eq!(word(ws, "TEMPLUM_CLAUDE_ARGS"), claude);
        #[cfg(unix)]
        {
            let exe = crate::workspace::usage::program().unwrap();
            let codex = templum_mcp::codex_args(&exe.to_string_lossy(), false, true).unwrap();
            assert_eq!(word(ws, "TEMPLUM_CODEX_ARGS"), codex.join("\n"));
        }
        let copilot = templum_usage::copilot::args(&data, cfg!(windows))
            .unwrap()
            .join("\n");
        assert!(copilot.ends_with(&p("copilot/plugin")), "{copilot}");
        // The bare word, one per line: no quotes in the variable.
        assert!(
            copilot.lines().any(|w| w == "--allow-tool=templum(reply)"),
            "{copilot}"
        );
        assert_eq!(word(ws, "TEMPLUM_COPILOT_ARGS"), copilot);
        let opencode = templum_usage::opencode::config(&data, cfg!(windows)).unwrap();
        assert_eq!(word(ws, "TEMPLUM_OPENCODE_CONFIG"), opencode);
        assert_eq!(word(ws, "TEMPLUM_KILO_CONFIG"), opencode);
        let qwen = templum_usage::qwen::args(&data, cfg!(windows)).unwrap();
        assert_eq!(word(ws, "TEMPLUM_QWEN_ARGS"), qwen.join("\n"));
        let system = templum_usage::qwen::system_file();
        let settings = match templum_usage::qwen::settings(&data, cfg!(windows), &system) {
            Some(file) => file.to_string_lossy().into_owned(),
            None => String::new(),
        };
        assert_eq!(word(ws, "TEMPLUM_QWEN_SETTINGS"), settings);
        let pi = templum_usage::pi::args(&data, cfg!(windows)).unwrap();
        assert_eq!(word(ws, "TEMPLUM_PI_ARGS"), pi.join("\n"));
        ws.config.connections.enabled = false;
        assert_eq!(word(ws, "TEMPLUM_PI_ARGS"), "");
        assert_eq!(word(ws, "TEMPLUM_CODEX_ARGS"), "");
        assert_eq!(word(ws, "TEMPLUM_COPILOT_ARGS"), "");
        assert_eq!(word(ws, "TEMPLUM_OPENCODE_CONFIG"), "");
        assert_eq!(word(ws, "TEMPLUM_KILO_CONFIG"), "");
        assert_eq!(word(ws, "TEMPLUM_QWEN_ARGS"), "");
        assert_eq!(word(ws, "TEMPLUM_QWEN_SETTINGS"), "");
        assert_eq!(
            word(ws, "TEMPLUM_CLAUDE_ARGS"),
            format!("--settings\n{}", p("claude-statusline.json"))
        );
        ws.config.usage.claude_limits = false;
        assert_eq!(word(ws, "TEMPLUM_CLAUDE_ARGS"), "");
        assert_eq!(word(ws, "TEMPLUM_CODEX_ARGS"), "");
        ws.config.connections.enabled = true;
        ws.config.usage.claude_limits = true;
        for program in ["powershell", "CMD.EXE"] {
            ws.config.shell.program = Some(program.into());
            assert_eq!(word(ws, "TEMPLUM_CODEX_ARGS"), "", "{program}");
            assert_eq!(word(ws, "TEMPLUM_CLAUDE_ARGS"), claude, "{program}");
            assert_eq!(word(ws, "TEMPLUM_COPILOT_ARGS"), copilot, "{program}");
        }
        ws.config.shell.program = Some("WSL.exe".into());
        assert_eq!(word(ws, "TEMPLUM_CLAUDE_ARGS"), "");
        assert_eq!(word(ws, "TEMPLUM_CODEX_ARGS"), "");
        assert_eq!(word(ws, "TEMPLUM_COPILOT_ARGS"), "");
        ws.config.shell.program = None;
        // Without `<data>/bin` on the terminal's PATH, the files' program can't be found.
        ws.bus_env.retain(|(k, _)| k != "PATH");
        assert_eq!(word(ws, "TEMPLUM_COPILOT_ARGS"), "");
        assert_eq!(word(ws, "TEMPLUM_OPENCODE_CONFIG"), "");
    });
    // Without a data folder: both, empty, so none comes from a Templum this one was started from.
    let (_, bare) = open_in(cx, None, None);
    bare.update(cx, |ws, _| {
        assert_eq!(
            ws.agent_env(None),
            [
                ("TEMPLUM_CLAUDE_ARGS".to_string(), String::new()),
                ("TEMPLUM_CODEX_ARGS".to_string(), String::new()),
                ("TEMPLUM_COPILOT_ARGS".to_string(), String::new()),
                ("TEMPLUM_OPENCODE_CONFIG".to_string(), String::new()),
                ("TEMPLUM_KILO_CONFIG".to_string(), String::new()),
                ("TEMPLUM_QWEN_ARGS".to_string(), String::new()),
                ("TEMPLUM_QWEN_SETTINGS".to_string(), String::new()),
                ("TEMPLUM_PI_ARGS".to_string(), String::new()),
                ("TEMPLUM_ACCOUNTS".to_string(), String::new()),
                ("TEMPLUM_HOOKED".to_string(), String::new()),
            ]
        );
    });
}

/// Every new terminal carries its accounts, for the shell to define a function each
/// (docs/specs/accounts.md, Typed in a terminal); empty with none.
#[gpui_kit::test]
fn a_new_terminal_gets_the_accounts(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let accounts = |ws: &Workspace| {
        let env = ws.agent_env(None);
        env.into_iter()
            .find(|(k, _)| k == "TEMPLUM_ACCOUNTS")
            .unwrap()
            .1
    };
    ws.read_with(cx, |ws, _| assert_eq!(accounts(ws), ""));
    super::agents::with_agents(&ws, cx, "[agents]\nclaudinho = { of = \"claude\" }\n");
    ws.read_with(cx, |ws, _| {
        assert!(
            accounts(ws).starts_with("claudinho\tclaude\t"),
            "{}",
            accounts(ws)
        );
    });
}

/// A program path a shell would expand (`$`) can't be typed into the hooks: the server isn't
/// given either, or the agent would be connected with a dot that never moves.
#[test]
fn claude_code_gets_the_server_only_with_the_hooks() {
    use crate::workspace::usage::claude_flags;
    let data = tempfile::tempdir().unwrap();
    let flags = claude_flags(
        data.path(),
        std::path::Path::new("/opt/templum"),
        cfg!(windows),
        false,
        true,
    );
    assert!(
        flags.contains(" --settings ") && flags.contains(" --mcp-config "),
        "{flags}"
    );
    assert_eq!(
        claude_flags(
            data.path(),
            std::path::Path::new("/opt/$x/templum"),
            cfg!(windows),
            false,
            true
        ),
        ""
    );
}

/// A data folder a shell would expand (`$`): the agent starts as typed, without a broken line.
#[cfg(unix)]
#[gpui_kit::test]
fn an_untypeable_data_folder_gives_no_connections(cx: &mut TestAppContext) {
    let parent = tempfile::tempdir().unwrap();
    let data = parent.path().join("a$b");
    std::fs::create_dir(&data).unwrap();
    let (_, ws) = open_in(cx, None, Some(data));
    ws.update(cx, |ws, _| {
        let claude = ws.agents.iter().find(|a| a.id == "claude").cloned();
        assert_eq!(ws.typed_in(None, claude.as_ref(), "claude"), "claude");
    });
}

/// A limit nearing its end, read while Templum is in the background: one system notification,
/// not one per read.
#[cfg(unix)]
#[gpui_kit::test]
fn a_limit_near_its_end_is_told_once(cx: &mut TestAppContext) {
    use templum_usage::{Limits, Window};
    let (handle, ws) = open(cx);
    cx.update(|cx| cx.set_app_identity("test.Templum", "Templum"));
    super::terminals::set_active(handle, false, cx);
    let now = crate::notes::now();
    let limits = vec![(
        "claude".to_string(),
        Limits {
            windows: vec![Window {
                minutes: 300,
                used_percent: 92.0,
                resets_at: now + 2 * 3600,
                scope: None,
            }],
            at: now,
        },
    )];
    for _ in 0..2 {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.usage_done(Vec::new(), (Vec::new(), limits.clone()), window, cx)
            });
            window.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
    let shown = cx.shown_system_notifications();
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(
        shown[0].title.as_ref(),
        "Claude Code: 92% of the 5-hour limit used"
    );
    assert!(shown[0].body.starts_with("Resets "), "{:?}", shown[0].body);
    assert_eq!(ws.read_with(cx, |ws, _| ws.limits.clone()), limits);
}

/// The title bar's meter shows only with a limit known; it opens the usage, whose sessions show
/// their terminals. A terminal with usage has its chip.
#[gpui_kit::test]
fn the_meter_opens_the_usage_and_a_session_shows_its_terminal(cx: &mut TestAppContext) {
    use templum_usage::{Limits, Session, Window};
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("usage-meter").is_none(), "no limit known");
        window.press(NEW, cx);
    })
    .unwrap();
    let now = crate::notes::now();
    ws.update(cx, |ws, _| {
        ws.limits = vec![(
            "claude".into(),
            Limits {
                windows: vec![Window {
                    minutes: 300,
                    used_percent: 73.0,
                    resets_at: now + 3 * 3600,
                    scope: None,
                }],
                at: now,
            },
        )];
        let pane = ws.space.terminal_mut(first).unwrap();
        pane.seen.agent = Some("claude".into());
        pane.usage = Some(Session {
            context_percent: Some(42.0),
            cost_usd: Some(3.1),
            ..Session::default()
        });
        assert_ne!(ws.space.focused, Some(first));
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("usage-chip", first.0 as usize)).is_some());
        window.click("usage-meter", cx);
        window.render_frame(cx);
        assert!(window.try_find("usage-popover").is_some());
        window.click(("usage-session", 0_usize), cx);
        window.render_frame(cx);
        assert!(window.try_find("usage-popover").is_none());
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.focused, Some(first)));
}

fn limits_of(agent: &str, used: f32) -> (String, templum_usage::Limits) {
    let now = crate::notes::now();
    (
        agent.to_string(),
        templum_usage::Limits {
            windows: vec![templum_usage::Window {
                minutes: 300,
                used_percent: used,
                resets_at: now + 3 * 3600,
                scope: None,
            }],
            at: now,
        },
    )
}

/// With several agents' limits known, the meter shows them in turn; one hidden in the settings
/// never shows there.
#[gpui_kit::test]
fn the_meter_takes_the_agents_in_turn_but_not_a_hidden_one(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        ws.limits = vec![limits_of("claude", 27.0), limits_of("codex", 91.0)];
    });
    let shown = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            (
                window.try_find("usage-meter-claude").is_some(),
                window.try_find("usage-meter-codex").is_some(),
            )
        })
        .unwrap()
    };
    assert_eq!(shown(cx), (true, false), "Claude first");
    // A read after TURN_SECS turns it, unless the pointer is over the meter.
    let read = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.meter_turned = 0;
                let limits = ws.limits.clone();
                ws.usage_done(Vec::new(), (Vec::new(), limits), window, cx)
            })
        })
        .unwrap()
    };
    read(&ws, cx);
    assert_eq!(shown(cx), (false, true), "then Codex");
    ws.update(cx, |ws, _| ws.meter_hovered = true);
    read(&ws, cx);
    assert_eq!(shown(cx), (false, true), "held while hovered");
    ws.update(cx, |ws, _| {
        ws.meter_hovered = false;
        ws.config.usage.hide = vec!["codex".into()];
    });
    for _ in 0..3 {
        read(&ws, cx);
        assert_eq!(shown(cx), (true, false), "Codex hidden");
    }
}

/// Settings ▸ Usage hides an agent's limits from the title bar, and ↺ shows them again.
#[gpui_kit::test]
fn an_agent_is_taken_off_the_title_bar_and_back(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-usage", cx);
        window.render_frame(cx);
        // The section's last row, below the fold of a small window.
        window.scroll(
            "settings-scroll",
            ScrollDelta::Pixels(point(px(0.), px(-5000.))),
            cx,
        );
        window.render_frame(cx);
        window.click("field-usage.hide.codex", cx);
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
    assert!(text.contains(r#"hide = ["codex"]"#), "{text}");
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.config.usage.hide.clone()),
        ["codex"]
    );
    cx.update_window(handle, |_, window, cx| {
        window.click("reset-usage.hide.codex", cx);
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
    assert!(!text.contains("hide"), "{text}");
}

/// Templum asks Claude Code for the plan's limits only with a data folder, when the user
/// wants them, and runs the program Claude Code's command names.
#[gpui_kit::test]
fn claude_code_is_asked_for_its_limits_when_wanted(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let program = data.path().join("my-claude");
    std::fs::write(&program, "").unwrap();
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    ws.update(cx, |ws, _| {
        let claude = ws.agents.iter_mut().find(|a| a.id == "claude").unwrap();
        claude.command = format!("{} --model opus", program.display());
        let probe = data.path().join("usage").join("probe");
        assert_eq!(
            ws.claude_poll_jobs(),
            [("claude".to_string(), program.clone(), probe.clone(), None)]
        );
        ws.config.usage.claude_limits = false;
        assert_eq!(ws.claude_poll_jobs(), []);
        ws.config.usage.claude_limits = true;
        ws.data_dir = None;
        assert_eq!(ws.claude_poll_jobs(), []);
    });
}

/// A workspace on a data folder with Claude Code (a made-up program) and its account `claude2`,
/// which never ran and has a login (its `.claude.json`).
fn two_claudes(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Workspace>, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let program = data.path().join("my-claude");
    std::fs::write(&program, "").unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    super::agents::with_agents(
        &ws,
        cx,
        &format!(
            "[agents.claude]\ncommand = '{}'\n{}",
            program.display(),
            super::agents::CLAUDE2
        ),
    );
    let folder = data.path().join("accounts").join("claude2");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(".claude.json"), "{}").unwrap();
    (handle, ws, data)
}

/// Each Claude Code account with a login is asked too, on its folder, whether it ran since
/// Templum started or not; one without a login has nothing to ask for.
#[gpui_kit::test]
fn an_account_with_a_login_is_asked_for_its_limits_without_having_run(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let program = data.path().join("my-claude");
    std::fs::write(&program, "").unwrap();
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    super::agents::with_agents(
        &ws,
        cx,
        &format!(
            "[agents.claude]\ncommand = '{}'\n{}",
            program.display(),
            super::agents::CLAUDE2
        ),
    );
    let account = data.path().join("accounts").join("claude2");
    ws.update(cx, |ws, _| {
        assert_eq!(ws.claude_poll_jobs().len(), 1, "no folder");
        std::fs::create_dir_all(&account).unwrap();
        assert_eq!(ws.claude_poll_jobs().len(), 1, "no login");
        std::fs::write(account.join(".claude.json"), "{}").unwrap();
        let jobs = ws.claude_poll_jobs();
        assert_eq!(
            jobs[1],
            (
                "claude2".to_string(),
                program.clone(),
                data.path().join("usage").join("probe"),
                Some(account)
            )
        );
    });
}

/// `/usage` is asked for an account at most once a minute, and every five when nothing else asks;
/// a trigger asks for its account only.
#[gpui_kit::test]
fn an_account_is_asked_at_most_once_a_minute(cx: &mut TestAppContext) {
    let (_, ws, _data) = two_claudes(cx);
    ws.update(cx, |ws, _| {
        let ids = |jobs: Vec<(String, _, _, _)>| -> Vec<String> {
            jobs.into_iter().map(|(id, ..)| id).collect()
        };
        let t = 10_000;
        assert_eq!(
            ids(ws.claude_due(t, None, CLAUDE_EVERY)),
            ["claude", "claude2"]
        );
        assert_eq!(
            ws.claude_due(t + 30, None, CLAUDE_MIN),
            [],
            "within a minute"
        );
        assert_eq!(
            ws.claude_due(t + 299, None, CLAUDE_EVERY),
            [],
            "nothing asked meanwhile"
        );
        assert_eq!(
            ids(ws.claude_due(t + CLAUDE_MIN, Some("claude2"), CLAUDE_MIN)),
            ["claude2"],
            "a trigger asks its account only"
        );
        assert_eq!(
            ws.claude_due(t + CLAUDE_MIN + 59, Some("claude2"), CLAUDE_MIN),
            [],
            "and not again within a minute"
        );
        assert_eq!(ws.claude_due(t + CLAUDE_MIN, Some("codex"), CLAUDE_MIN), []);
        assert_eq!(
            ids(ws.claude_due(t + CLAUDE_EVERY, None, CLAUDE_EVERY)),
            ["claude"],
            "five minutes on, the one not asked since"
        );
    });
}

/// The meter turning to an account, the popover opening and a Claude Code turn's end ask for its
/// limits; hovering the meter (no turn) and agents other than Claude Code's don't.
#[gpui_kit::test]
fn the_meter_the_popover_and_a_turns_end_ask_for_limits(cx: &mut TestAppContext) {
    let (handle, ws, _data) = two_claudes(cx);
    let asked = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.update(cx, |ws, _| {
            ws.claude_queue.clear();
            let mut ids: Vec<String> = ws.claude_asked.drain().map(|(id, _)| id).collect();
            ids.sort();
            ids
        })
    };
    ws.update(cx, |ws, _| {
        ws.limits = vec![limits_of("claude", 27.0), limits_of("claude2", 50.0)];
    });
    // The meter turns to the second.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.meter_turned = 0;
            let limits = ws.limits.clone();
            ws.usage_done(Vec::new(), (Vec::new(), limits), window, cx)
        })
    })
    .unwrap();
    assert_eq!(asked(&ws, cx), ["claude2"], "the meter turned");
    // Held under the pointer, it doesn't turn, and asks nothing.
    ws.update(cx, |ws, _| ws.meter_hovered = true);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.meter_turned = 0;
            ws.claude_asked.clear();
            let limits = ws.limits.clone();
            ws.usage_done(Vec::new(), (Vec::new(), limits), window, cx)
        })
    })
    .unwrap();
    assert_eq!(asked(&ws, cx), Vec::<String>::new(), "the meter was held");
    ws.update(cx, |ws, _| ws.meter_hovered = false);
    ws.update(cx, |ws, cx| ws.toggle_usage(cx));
    assert_eq!(asked(&ws, cx), ["claude", "claude2"], "the popover opened");
    ws.update(cx, |ws, cx| ws.toggle_usage(cx));
    assert_eq!(asked(&ws, cx), Vec::<String>::new(), "and closed");
    // A turn's end in a terminal of the account.
    let first = first_id(&ws, cx);
    let terminal = ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(first).unwrap();
        pane.seen.agent = Some("claude2".into());
        pane.term.id().to_string()
    });
    let stop = serde_json::json!({ "hook_event_name": "Stop", "last_assistant_message": "Done." });
    let said = templum_mcp::hooks::claude(&stop).expect("an event");
    let said = serde_json::to_value(said).unwrap();
    super::bus::event(handle, &ws, cx, &terminal, said.clone());
    assert_eq!(asked(&ws, cx), ["claude2"], "a turn ended");
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(first).unwrap().seen.agent = Some("codex".into());
    });
    super::bus::event(handle, &ws, cx, &terminal, said);
    assert_eq!(asked(&ws, cx), Vec::<String>::new(), "Codex has no probe");
}

/// With one agent in the meter, its limits are asked again every minute; with several, only the
/// five-minute look asks.
#[gpui_kit::test]
fn the_one_agent_in_the_meter_is_asked_every_minute(cx: &mut TestAppContext) {
    let (_, ws, _data) = two_claudes(cx);
    let now = crate::notes::now();
    let stamp = |ws: &mut Workspace| {
        ws.claude_asked = [("claude".into(), now - 61), ("claude2".into(), now - 61)].into();
    };
    let asked_at = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.update(cx, |ws, cx| {
            ws.ask_claude_due(cx);
            (
                ws.claude_asked["claude"] > now - 61,
                ws.claude_asked["claude2"] > now - 61,
            )
        })
    };
    ws.update(cx, |ws, _| {
        ws.limits = vec![limits_of("claude", 27.0)];
        stamp(ws);
    });
    assert_eq!(asked_at(&ws, cx), (true, false), "the one shown");
    ws.update(cx, |ws, _| {
        ws.limits = vec![limits_of("claude", 27.0), limits_of("claude2", 50.0)];
        stamp(ws);
    });
    assert_eq!(asked_at(&ws, cx), (false, false), "two shown");
}

/// The probes run one at a time, the one a trigger asks for first; an account waits or runs once,
/// and is asked for when its probe starts, not when it queued.
#[gpui_kit::test]
fn probes_run_one_at_a_time_the_asked_one_first(cx: &mut TestAppContext) {
    let (_, ws, _data) = two_claudes(cx);
    let ids = |ws: &Workspace| -> Vec<String> {
        ws.claude_queue.iter().map(|(id, ..)| id.clone()).collect()
    };
    ws.update(cx, |ws, cx| {
        let t = crate::notes::now();
        ws.ask_claude(Some("claude"), CLAUDE_MIN, cx);
        ws.ask_claude(Some("claude2"), CLAUDE_MIN, cx);
        assert_eq!(ids(ws), ["claude2", "claude"], "the last asked first");
        // Queued, an account isn't queued again.
        ws.claude_asked.clear();
        ws.ask_claude(None, CLAUDE_MIN, cx);
        assert_eq!(ids(ws), ["claude2", "claude"]);
        assert_eq!(ws.claude_next(t + 90).unwrap().0, "claude2");
        assert_eq!(ws.claude_asked["claude2"], t + 90, "asked as it starts");
        assert!(ws.claude_next(t + 90).is_none(), "one runs");
        // Nor while it runs, nor within a minute of its start.
        assert_eq!(ws.claude_due(t + 100, Some("claude2"), CLAUDE_MIN), []);
        ws.probe_done("claude2".into(), Some(limits_of("claude2", 1.0).1), cx);
        assert_eq!(ws.claude_due(t + 120, Some("claude2"), CLAUDE_MIN), []);
        assert_eq!(
            ws.claude_due(t + 90 + CLAUDE_MIN, Some("claude2"), CLAUDE_MIN)
                .len(),
            1
        );
        assert_eq!(ws.claude_next(t + 91).unwrap().0, "claude");
        assert!(ws.claude_next(t + 91).is_none());
    });
}

/// A probe that finds nothing (an API key has no plan, an offline machine no answer) isn't asked
/// again by a trigger for five minutes, as one that finds limits is after one.
#[gpui_kit::test]
fn a_probe_that_finds_nothing_is_not_asked_again_by_triggers(cx: &mut TestAppContext) {
    let (_, ws, _data) = two_claudes(cx);
    ws.update(cx, |ws, cx| {
        let t = crate::notes::now();
        let ask = |ws: &mut Workspace, id: &str, after: u64| {
            ws.claude_due(t + after, Some(id), CLAUDE_MIN).len()
        };
        ws.probe_done("claude".into(), None, cx);
        ws.claude_asked.insert("claude2".into(), t);
        assert_eq!(ask(ws, "claude", CLAUDE_MIN + 5), 0, "nothing found");
        assert_eq!(
            ask(ws, "claude", CLAUDE_EVERY - 5),
            0,
            "within five minutes"
        );
        assert_eq!(ask(ws, "claude", CLAUDE_EVERY + 5), 1, "then it may");
        assert_eq!(
            ask(ws, "claude2", CLAUDE_MIN + 5),
            1,
            "found limits: a minute"
        );
        assert_eq!(
            ws.claude_due(t + 2 * CLAUDE_EVERY, None, CLAUDE_EVERY)
                .len(),
            1,
            "the look"
        );
    });
}

/// A waiting probe of an account that was deleted, or of any when the limits were turned off, is
/// dropped: running Claude Code on its folder would make the folder again.
#[gpui_kit::test]
fn a_queued_probe_is_dropped_with_its_account(cx: &mut TestAppContext) {
    let (_, ws, _data) = two_claudes(cx);
    ws.update(cx, |ws, cx| {
        ws.ask_claude(None, CLAUDE_EVERY, cx);
        ws.agents.retain(|a| a.id != "claude2");
        assert_eq!(ws.claude_next(1).unwrap().0, "claude");
        ws.probe_done("claude".into(), None, cx);
        assert!(ws.claude_next(1).is_none(), "claude2 is gone");
        ws.ask_claude(None, CLAUDE_EVERY, cx);
        ws.config.usage.claude_limits = false;
        assert!(ws.claude_next(1).is_none(), "turned off");
    });
}

/// What `/usage` finds for an account that is gone, or not asked any more, goes with it; a result
/// that arrives after, or one older than the last, is ignored.
#[gpui_kit::test]
fn what_a_probe_found_goes_with_its_account(cx: &mut TestAppContext) {
    let (handle, ws, _data) = two_claudes(cx);
    let (_, found) = limits_of("claude2", 61.0);
    let read = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.usage_done(Vec::new(), (Vec::new(), Vec::new()), window, cx)
            })
        })
        .unwrap()
    };
    ws.update(cx, |ws, cx| {
        ws.claude_polled("claude2".into(), found.clone(), cx)
    });
    ws.update(cx, |ws, cx| {
        let older = templum_usage::Limits {
            at: found.at - 1,
            ..found.clone()
        };
        ws.claude_polled("claude2".into(), older, cx);
        assert_eq!(ws.claude_polled["claude2"], found, "not the older");
        ws.claude_polled("claude9".into(), found.clone(), cx);
        assert!(!ws.claude_polled.contains_key("claude9"), "no such account");
    });
    read(&ws, cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.limits.len(), 1));
    // Turned off: its row goes.
    ws.update(cx, |ws, _| ws.config.usage.claude_limits = false);
    read(&ws, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.limits, []);
        assert!(ws.claude_polled.is_empty());
    });
    // Deleted: the same.
    ws.update(cx, |ws, cx| {
        ws.config.usage.claude_limits = true;
        ws.claude_polled("claude2".into(), found.clone(), cx);
        ws.agents.retain(|a| a.id != "claude2");
    });
    read(&ws, cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.limits, []));
}

/// What `/usage` finds is in the meter's limits at once, and stays through the next read.
#[gpui_kit::test]
fn a_fresh_probe_shows_without_waiting_for_the_next_read(cx: &mut TestAppContext) {
    let (handle, ws, _data) = two_claudes(cx);
    let (_, found) = limits_of("claude2", 61.0);
    ws.update(cx, |ws, cx| {
        ws.claude_polled("claude2".into(), found.clone(), cx)
    });
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.limits, [("claude2".to_string(), found.clone())]);
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.usage_done(Vec::new(), (Vec::new(), Vec::new()), window, cx)
        })
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.limits, [("claude2".to_string(), found)])
    });
}

/// Each account is its own row of the meter, named by its name, and its own switch.
#[gpui_kit::test]
fn each_account_has_its_row_in_the_meter(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    super::agents::with_agents(&ws, cx, super::agents::CLAUDE2);
    let now = crate::notes::now();
    let limits = templum_usage::Limits {
        windows: vec![templum_usage::Window {
            minutes: 300,
            used_percent: 40.0,
            resets_at: now + 3600,
            scope: None,
        }],
        at: now,
    };
    ws.update(cx, |ws, _| {
        ws.limits = vec![
            ("claude".into(), limits.clone()),
            ("claude2".into(), limits),
        ];
        let shown: Vec<String> = crate::usage::meter_agents(ws, now)
            .into_iter()
            .map(|(id, _)| ws.agent_title(id))
            .collect();
        assert_eq!(shown, ["Claude Code", "Claude Code 2"]);
        ws.config.usage.hide = vec!["claude2".into()];
        let shown: Vec<&str> = crate::usage::meter_agents(ws, now)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(shown, ["claude"]);
    });
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-usage", cx);
        window.render_frame(cx);
        assert!(window.try_find("field-usage.hide.claude2").is_some());
    })
    .unwrap();
}

#[cfg(unix)]
#[test]
fn gemini_cli_links_the_extension_or_says_why_not() {
    use crate::workspace::usage::gemini_run;
    let data = tempfile::tempdir().unwrap();
    // `true` and `false` stand in for gemini: whatever they are given, they succeed or fail.
    assert_eq!(
        gemini_run(Some("true".into()), data.path(), true, None),
        Ok(())
    );
    assert!(
        data.path()
            .join("gemini")
            .join("gemini-extension.json")
            .exists(),
        "the extension is written first"
    );
    let why = gemini_run(Some("false".into()), data.path(), false, None).unwrap_err();
    assert!(why.starts_with("Gemini CLI failed"), "{why}");
    let why = gemini_run(None, data.path(), true, None).unwrap_err();
    assert!(why.contains("didn't find Gemini CLI"), "{why}");
}

/// A Gemini CLI opened without Templum's extension says so once per run, never on a restore
/// (docs/specs/connections.md, Gemini CLI without its extension).
#[gpui_kit::test]
fn a_gemini_cli_without_the_extension_is_told_once(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let open = |id: &str, cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                let entry = ws.agents.iter().find(|a| a.id == id).cloned().unwrap();
                let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
                ws.start_agent(Some(&entry), None, "", folder.path(), rect, window, cx)
                    .unwrap();
            })
        })
        .unwrap();
    };
    let before = notification_count(handle, cx);
    open("claude", cx);
    assert_eq!(notification_count(handle, cx), before, "Claude Code");
    // A restored Gemini CLI says nothing.
    let item: crate::session::Item = serde_json::from_value(serde_json::json!({
        "kind": "terminal",
        "rect": { "x": 0.0, "y": 400.0, "w": 400.0, "h": 300.0 },
        "folder": folder.path(),
        "agent": "gemini",
    }))
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.add_saved(&item, window, cx).unwrap());
    })
    .unwrap();
    assert_eq!(notification_count(handle, cx), before, "restored");
    // Connections off: nothing to install it for.
    ws.update(cx, |ws, _| ws.config.connections.enabled = false);
    open("gemini", cx);
    assert_eq!(notification_count(handle, cx), before, "off");
    ws.update(cx, |ws, _| ws.config.connections.enabled = true);
    open("gemini", cx);
    assert_eq!(notification_count(handle, cx), before + 1, "once");
    open("gemini", cx);
    assert_eq!(notification_count(handle, cx), before + 1, "not twice");
}

/// The OpenCode and Kilo Code Templum opens get their variable in their shell's environment,
/// unless the person has their own (docs/specs/connections.md, OpenCode and Kilo Code).
#[gpui_kit::test]
fn blocks_templum_opens_get_their_agents_variable(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    ws.update(cx, |ws, _| {
        let agent = |id: &str| ws.agents.iter().find(|a| a.id == id).cloned();
        let (opencode, kilo, claude) = (agent("opencode"), agent("kilo"), agent("claude"));
        let unset = |_: &str| false;
        let config = templum_usage::opencode::config(data.path(), cfg!(windows)).unwrap();
        assert_eq!(
            ws.agent_vars(opencode.as_ref(), None, &unset),
            [("OPENCODE_CONFIG_CONTENT".to_string(), config.clone())]
        );
        let wsl = Some("C:/Windows/System32/wsl.exe");
        assert_eq!(
            ws.agent_vars(opencode.as_ref(), wsl, &unset),
            [],
            "nothing crosses into WSL"
        );
        assert_eq!(
            ws.agent_vars(kilo.as_ref(), None, &unset),
            [("KILO_CONFIG_CONTENT".to_string(), config)]
        );
        assert_eq!(ws.agent_vars(claude.as_ref(), None, &unset), []);
        // Qwen Code: its system settings file moved to Templum's, unless its own exists.
        let qwen = agent("qwen");
        let system = templum_usage::qwen::system_file();
        let want: Vec<(String, String)> =
            templum_usage::qwen::settings(data.path(), cfg!(windows), &system)
                .map(|file| {
                    let file = file.to_string_lossy().into_owned();
                    ("QWEN_CODE_SYSTEM_SETTINGS_PATH".to_string(), file)
                })
                .into_iter()
                .collect();
        assert_eq!(ws.agent_vars(qwen.as_ref(), None, &unset), want);
        let theirs = |name: &str| name == "QWEN_CODE_SYSTEM_SETTINGS_PATH";
        assert_eq!(ws.agent_vars(qwen.as_ref(), None, &theirs), []);
        assert_eq!(ws.agent_vars(None, None, &unset), []);
        // The person's own wins.
        let theirs = |name: &str| name == "OPENCODE_CONFIG_CONTENT";
        assert_eq!(ws.agent_vars(opencode.as_ref(), None, &theirs), []);
        assert_eq!(ws.agent_vars(kilo.as_ref(), None, &theirs).len(), 1);
        // Without `<data>/bin` on the terminal's PATH, the plugin's program can't be found.
        let env = ws.bus_env.clone();
        ws.bus_env.retain(|(k, _)| k != "PATH");
        assert_eq!(ws.agent_vars(opencode.as_ref(), None, &unset), []);
        ws.bus_env = env;
        ws.config.connections.enabled = false;
        assert_eq!(ws.agent_vars(opencode.as_ref(), None, &unset), []);
        // Every terminal gets an empty `TEMPLUM_HOOKED`: not a Templum's it was started from.
        let hooked = ws
            .agent_env(None)
            .into_iter()
            .find(|(k, _)| k == "TEMPLUM_HOOKED");
        assert_eq!(hooked, Some(("TEMPLUM_HOOKED".to_string(), String::new())));
    });
}

/// The words for agents typed in a terminal follow its own shell, not the default
/// (docs/specs/shells.md, Agents typed in a chosen shell).
#[gpui_kit::test]
fn a_terminals_agent_words_follow_its_own_shell(cx: &mut TestAppContext) {
    let parent = tempfile::tempdir().unwrap();
    let data = parent.path().join("Templum Data");
    std::fs::create_dir(&data).unwrap();
    let (_, ws) = open_in(cx, None, Some(data));
    ws.update(cx, |ws, _| {
        let word = |ws: &Workspace, shell: Option<&str>, key: &str| {
            ws.agent_env(shell)
                .into_iter()
                .find(|(k, _)| k == key)
                .unwrap()
                .1
        };
        ws.config.shell.program = None;
        assert_ne!(word(ws, None, "TEMPLUM_CLAUDE_ARGS"), "");
        // Forward slashes: a file name on every OS.
        let wsl = "C:/Windows/System32/wsl.exe";
        assert_eq!(word(ws, Some(wsl), "TEMPLUM_CLAUDE_ARGS"), "");
        #[cfg(unix)]
        {
            ws.config.shell.program = Some("powershell".into());
            assert_eq!(word(ws, None, "TEMPLUM_CODEX_ARGS"), "");
            assert_ne!(word(ws, Some("/bin/bash"), "TEMPLUM_CODEX_ARGS"), "");
        }
    });
}

/// A Gemini CLI account with its own extensions, without Templum's, is told; one sharing them
/// with a folder that has it isn't.
#[gpui_kit::test]
fn a_gemini_account_without_the_extension_is_told(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    super::agents::with_agents(
        &ws,
        cx,
        concat!(
            "[agents.gemini2]\nof = \"gemini\"\n",
            "[agents.gemini3]\nof = \"gemini\"\nshare = { extensions = \"gemini2\" }\n",
        ),
    );
    super::agents::launch(handle, &ws, cx, "gemini2", folder.path());
    ws.read_with(cx, |ws, _| assert!(ws.gemini_told.contains("gemini2")));
    let extensions = data
        .path()
        .join("accounts/gemini2/.gemini/extensions/templum");
    std::fs::create_dir_all(&extensions).unwrap();
    super::agents::launch(handle, &ws, cx, "gemini3", folder.path());
    ws.read_with(cx, |ws, _| assert!(!ws.gemini_told.contains("gemini3")));
}
