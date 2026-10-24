//! The person's instructions for their agents: which apply to a session's start, what the agent
//! reads, and Settings ▸ Instructions (docs/specs/instructions.md).

use serde_json::{Value, json};

use super::agents::{CLAUDE2, with_agents};
use super::bus::{ask_as, me};
use super::*;
use crate::instructions::Draft;

fn draft(name: &str, text: &str) -> Draft {
    Draft {
        name: name.into(),
        text: text.into(),
        ..Draft::default()
    }
}

fn add(ws: &Entity<Workspace>, cx: &mut TestAppContext, draft: Draft) -> u64 {
    ws.update(cx, |ws, _| ws.standing.add(draft))
}

/// What the hook of `agent` in `terminal` is told as its session starts.
fn start(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    terminal: &str,
    agent: &str,
) -> String {
    let answer = ask_as(
        handle,
        ws,
        cx,
        terminal,
        "hook",
        "templum/instructions",
        json!({ "agent": agent }),
    );
    answer["result"]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{answer}"))
        .to_string()
}

/// Says the first terminal runs `agent`, as a snapshot's look at its process does.
fn runs(ws: &Entity<Workspace>, cx: &mut TestAppContext, agent: Option<&str>) {
    let id = first_id(ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(id).unwrap().seen.agent = agent.map(str::to_string)
    });
}

/// One snapshot period, with the background write finished.
#[cfg(unix)]
fn tick(cx: &mut TestAppContext) {
    cx.executor().advance_clock(crate::session::EVERY);
    cx.run_until_parked();
}

/// The first block's terminal.
fn first_terminal(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> String {
    let id = first_id(ws, cx);
    me(ws, cx, id).0
}

fn order(text: &str) -> Vec<&str> {
    text.lines().filter_map(|l| l.strip_prefix("## ")).collect()
}

#[gpui_kit::test]
fn a_session_gets_the_instructions_that_apply(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, id);
    let w = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let other = ws.update(cx, |ws, _| ws.make_space());
    let own = |name: &str, text: &str, space: &str| Draft {
        workspace: Some(space.into()),
        ..draft(name, text)
    };
    add(&ws, cx, own("A", "Run the tests.", &w));
    add(&ws, cx, draft("B", "Commit in English.\n\nImperative."));
    add(&ws, cx, own("C", "Not here.", &other));
    add(&ws, cx, own("D", "Mine too.", &w));
    let text = start(handle, &ws, cx, &terminal, "claude");
    assert_eq!(order(&text), ["B", "A", "D"], "{text}");
    assert!(
        text.starts_with("The person's standing instructions, written in Templum"),
        "{text}"
    );
    assert!(
        text.contains("## B\n\nCommit in English.\n\nImperative.\n\n## A\n\nRun the tests."),
        "{text}"
    );
    assert!(!text.contains("Not here."), "{text}");
    // Another workspace's block gets its own and the every-workspace ones.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.move_blocks(vec![id], Some(&other), window, cx)
        })
    })
    .unwrap();
    let text = start(handle, &ws, cx, &terminal, "claude");
    assert_eq!(order(&text), ["B", "C"], "{text}");
}

#[gpui_kit::test]
fn an_instruction_for_chosen_agents_reaches_only_them(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    with_agents(
        &ws,
        cx,
        "[agents.mine]\ncommand = \"claude --my-flag\"\nname = \"Mine\"\n",
    );
    let terminal = first_terminal(&ws, cx);
    runs(&ws, cx, Some("claude"));
    let chosen = |agents: &[&str]| Draft {
        agents: Some(agents.iter().map(|a| a.to_string()).collect()),
        ..draft("X", "x")
    };
    let for_codex = add(&ws, cx, chosen(&["codex"]));
    assert_eq!(start(handle, &ws, cx, &terminal, "claude"), "");
    runs(&ws, cx, Some("codex"));
    assert_ne!(start(handle, &ws, cx, &terminal, "codex"), "");
    // One of the person's `[agents]` running `claude` is Claude Code to it.
    ws.update(cx, |ws, _| ws.standing.remove(for_codex));
    add(&ws, cx, chosen(&["mine"]));
    runs(&ws, cx, Some("claude"));
    assert_ne!(start(handle, &ws, cx, &terminal, "claude"), "");
    runs(&ws, cx, Some("codex"));
    assert_eq!(start(handle, &ws, cx, &terminal, "codex"), "");
}

/// An instruction for an account is its block's only; one for the agent reaches its accounts' too.
#[gpui_kit::test]
fn an_accounts_instruction_is_its_own(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    with_agents(&ws, cx, CLAUDE2);
    let id = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, id);
    let account = |ws: &Entity<Workspace>, cx: &mut TestAppContext, seen: Option<&str>| {
        ws.update(cx, |ws, _| {
            let pane = ws.space.terminal_mut(id).unwrap();
            pane.account = Some("claude2".into());
            pane.seen.agent = seen.map(str::to_string);
        })
    };
    let chosen = |agent: &str| Draft {
        agents: Some(vec![agent.into()]),
        ..draft("X", "x")
    };
    let for_claude2 = add(&ws, cx, chosen("claude2"));
    // A plain Claude Code terminal never gets Claude Code 2's.
    runs(&ws, cx, Some("claude"));
    assert_eq!(start(handle, &ws, cx, &terminal, "claude"), "");
    // The account's block does, seen or not yet seen.
    account(&ws, cx, Some("claude2"));
    assert_ne!(start(handle, &ws, cx, &terminal, "claude"), "");
    account(&ws, cx, None);
    assert_ne!(start(handle, &ws, cx, &terminal, "claude"), "");
    // One for Claude Code reaches the account's block too.
    ws.update(cx, |ws, _| ws.standing.remove(for_claude2));
    add(&ws, cx, chosen("claude"));
    account(&ws, cx, Some("claude2"));
    assert_ne!(start(handle, &ws, cx, &terminal, "claude"), "");
}

#[gpui_kit::test]
fn the_hooks_agent_names_a_session_templum_hasnt_seen(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let terminal = first_terminal(&ws, cx);
    let for_agent = |agent: &str| Draft {
        agents: Some(vec![agent.into()]),
        ..draft(agent, agent)
    };
    add(&ws, cx, for_agent("claude"));
    add(&ws, cx, for_agent("codex"));
    // Nothing seen yet: the hook's own agent.
    runs(&ws, cx, None);
    let text = start(handle, &ws, cx, &terminal, "codex");
    assert_eq!(order(&text), ["codex"], "{text}");
    // A stale look (Claude Code ran there a moment ago): the hook knows better.
    runs(&ws, cx, Some("claude"));
    let text = start(handle, &ws, cx, &terminal, "codex");
    assert_eq!(order(&text), ["codex"], "{text}");
    let text = start(handle, &ws, cx, &terminal, "claude");
    assert_eq!(order(&text), ["claude"], "{text}");
}

#[gpui_kit::test]
fn a_gone_workspace_or_agent_applies_nowhere(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let terminal = first_terminal(&ws, cx);
    add(
        &ws,
        cx,
        Draft {
            workspace: Some("gone".into()),
            ..draft("Gone workspace", "x")
        },
    );
    add(
        &ws,
        cx,
        Draft {
            agents: Some(vec!["removed".into()]),
            ..draft("Gone agent", "x")
        },
    );
    runs(&ws, cx, Some("claude"));
    assert_eq!(start(handle, &ws, cx, &terminal, "claude"), "");
    ws.read_with(cx, |ws, _| assert_eq!(ws.standing.instructions.len(), 2));
}

#[gpui_kit::test]
fn none_while_connections_are_off_or_outside_a_block(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let terminal = first_terminal(&ws, cx);
    add(&ws, cx, draft("A", "a"));
    assert_ne!(start(handle, &ws, cx, &terminal, "claude"), "");
    assert_eq!(start(handle, &ws, cx, "999-9", "claude"), "");
    ws.update(cx, |ws, _| ws.config.connections.enabled = false);
    assert_eq!(start(handle, &ws, cx, &terminal, "claude"), "");
}

#[gpui_kit::test]
fn the_text_loses_what_the_editor_doesnt_show(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let terminal = first_terminal(&ws, cx);
    add(
        &ws,
        cx,
        draft(
            "Two\nlines\u{200B}",
            "one\u{200B}\n\u{E0041}two\u{FE0F}\u{E0101}\r\n\tthree\u{202E}\u{FE00}\u{3164}\n\n\n",
        ),
    );
    let text = start(handle, &ws, cx, &terminal, "claude");
    assert!(
        text.ends_with("## Two lines\n\none\ntwo\n\tthree"),
        "{text:?}"
    );
}

#[gpui_kit::test]
fn a_broken_instructions_file_is_never_written_over(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(crate::instructions::path(dir.path()), "{").unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    assert!(dir.path().join("instructions.json.bak").exists());
    assert_eq!(notification_count(handle, cx), 1);
    ws.read_with(cx, |ws, _| assert!(ws.standing.instructions.is_empty()));
    ws.update(cx, |ws, cx| {
        ws.standing.add(draft("A", "a"));
        ws.save_canvas_now(cx);
    });
    assert!(crate::instructions::path(dir.path()).exists(), "a new file");
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_unreadable_instructions_file_is_left_alone(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let path = crate::instructions::path(dir.path());
    let text = r#"{"version":1,"instructions":[{"id":1,"name":"Keep","text":"k"}]}"#;
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    assert_eq!(notification_count(handle, cx), 1);
    ws.update(cx, |ws, cx| {
        ws.standing.add(draft("A", "a"));
        ws.save_canvas_now(cx);
    });
    tick(cx);
    assert!(crate::session::path(dir.path(), crate::session::DEFAULT_ID).exists());
    // Readable again, it is still not written over: this session never read it.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    ws.update(cx, |ws, cx| {
        ws.standing.add(draft("B", "b"));
        ws.save_canvas_now(cx);
    });
    tick(cx);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[gpui_kit::test]
fn the_instructions_are_saved_with_the_canvas(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    ws.update(cx, |ws, cx| {
        ws.standing.add(draft("A", "a"));
        ws.save_canvas_now(cx);
    });
    let text = std::fs::read_to_string(crate::instructions::path(dir.path())).unwrap();
    let saved: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["instructions"][0]["name"], "A", "{text}");
}

/// Saves `draft` as Settings ▸ Instructions does.
fn save(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: Option<u64>,
    draft: &Draft,
) -> Result<u64, &'static str> {
    ws.update(cx, |ws, cx| ws.save_instruction(id, draft, cx))
}

fn open_instructions(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-instructions", cx);
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn saving_an_instruction_adds_it_last_and_applies_at_once(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let terminal = first_terminal(&ws, cx);
    let first = save(&ws, cx, None, &draft("First", "one")).unwrap();
    let second = save(&ws, cx, None, &draft("Second", "two")).unwrap();
    assert!(second > first);
    let text = start(handle, &ws, cx, &terminal, "claude");
    assert_eq!(order(&text), ["First", "Second"], "{text}");
    // Editing one keeps its place and reaches the next start.
    save(&ws, cx, Some(first), &draft("First", "uno")).unwrap();
    let text = start(handle, &ws, cx, &terminal, "claude");
    assert!(text.contains("## First\n\nuno"), "{text}");
    assert_eq!(order(&text), ["First", "Second"], "{text}");
}

#[gpui_kit::test]
fn save_refuses_what_is_missing(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    let chosen_none = Draft {
        agents: Some(Vec::new()),
        ..draft("Name", "text")
    };
    for (bad, why) in [
        (draft("  ", "text"), "An instruction needs a name"),
        (draft("Name", " \n "), "An instruction needs a text"),
        (chosen_none, "Choose at least one agent, or Every agent"),
        (
            draft(&"n".repeat(61), "text"),
            "A name is at most 60 characters",
        ),
    ] {
        assert_eq!(save(&ws, cx, None, &bad), Err(why));
    }
    ws.read_with(cx, |ws, _| assert!(ws.standing.instructions.is_empty()));
    // A name is trimmed, a text kept as typed.
    let id = save(&ws, cx, None, &draft(" Name ", "  indented\n")).unwrap();
    ws.read_with(cx, |ws, _| {
        let saved = ws.standing.get(id).unwrap();
        assert_eq!(
            (saved.name.as_str(), saved.text.as_str()),
            ("Name", "  indented\n")
        );
    });
}

#[gpui_kit::test]
fn the_switch_turns_one_off_and_its_row_says_where_and_for_whom(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let terminal = first_terminal(&ws, cx);
    ws.update(cx, |ws, _| ws.space.name = "api".into());
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let id = save(
        &ws,
        cx,
        None,
        &Draft {
            workspace: Some(space),
            agents: Some(vec!["claude".into(), "codex".into(), "gone".into()]),
            ..draft("Tests", "Run them.")
        },
    )
    .unwrap();
    let every = save(&ws, cx, None, &draft("All", "x")).unwrap();
    ws.read_with(cx, |ws, _| {
        let row = |id| crate::settings::instruction_description(ws, ws.standing.get(id).unwrap());
        assert_eq!(row(id), "api · Claude Code, Codex, a removed agent");
        assert_eq!(row(every), "Every workspace · Every agent");
    });
    ws.update(cx, |ws, _| {
        let mut gone = ws.standing.get(id).unwrap().clone();
        gone.workspace = Some("deleted".into());
        ws.standing.instructions.retain(|i| i.id != id);
        ws.standing.instructions.push(gone);
    });
    ws.read_with(cx, |ws, _| {
        let row = crate::settings::instruction_description(ws, ws.standing.get(id).unwrap());
        assert!(row.starts_with("A deleted workspace · "), "{row}");
    });
    runs(&ws, cx, Some("claude"));
    open_instructions(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("add-instruction").is_some());
        window.click(format!("instruction-on-{every}"), cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert!(!ws.standing.get(every).unwrap().on));
    let text = start(handle, &ws, cx, &terminal, "claude");
    assert_eq!(
        text, "",
        "nothing applies: one is off, one is for a deleted workspace"
    );
    cx.update_window(handle, |_, window, cx| {
        window.click(format!("instruction-on-{every}"), cx);
        window.render_frame(cx);
    })
    .unwrap();
    let text = start(handle, &ws, cx, &terminal, "claude");
    assert_eq!(order(&text), ["All"], "{text}");
}

#[gpui_kit::test]
fn delete_removes_only_that_one(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let keep = save(&ws, cx, None, &draft("Keep", "k")).unwrap();
    let go = save(&ws, cx, None, &draft("Go", "g")).unwrap();
    open_instructions(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click(format!("edit-instruction-{go}"), cx);
        window.render_frame(cx);
    })
    .unwrap();
    // The dialog slides in for 0.25 s first: presses meanwhile are lost.
    std::thread::sleep(std::time::Duration::from_millis(300));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("delete-instruction", cx);
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let ids: Vec<u64> = ws.standing.instructions.iter().map(|i| i.id).collect();
        assert_eq!(ids, [keep]);
    });
    ws.update(cx, |ws, cx| ws.delete_instruction(keep, cx));
    ws.read_with(cx, |ws, _| assert!(ws.standing.instructions.is_empty()));
}

/// The editor's Save reads its controls: the place from the menu, the agents from the boxes.
#[gpui_kit::test]
fn the_editor_saves_where_and_for_whom(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let other = ws.update(cx, |ws, _| ws.make_space());
    let id = save(&ws, cx, None, &draft("Rule", "Do it.")).unwrap();
    open_instructions(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click(format!("edit-instruction-{id}"), cx);
        window.render_frame(cx);
    })
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // Where: the second workspace (every workspace, then the sidebar's order).
        window
            .within("dialog")
            .click("menu-instruction-workspace", cx);
        window.render_frame(cx);
        window.within("popup-menu").click(2usize, cx);
        window.render_frame(cx);
        // Agents: off "Every agent", then Claude Code alone.
        window.within("dialog").click("instruction-every-agent", cx);
        window.render_frame(cx);
        window
            .within("dialog")
            .click("instruction-agent-claude", cx);
        window.render_frame(cx);
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let saved = ws.standing.get(id).unwrap();
        assert_eq!(saved.workspace.as_deref(), Some(other.as_str()));
        assert_eq!(saved.agents.as_deref(), Some(&["claude".to_string()][..]));
        assert_eq!(
            (saved.name.as_str(), saved.text.as_str()),
            ("Rule", "Do it.")
        );
    });
}

/// The page draws the section's note (the element is the note's; the rendered window has no
/// text to read, so its words are checked on `Section::note`).
#[gpui_kit::test]
fn the_section_note_says_when_changes_reach_agents(cx: &mut TestAppContext) {
    let (handle, _ws) = open(cx);
    open_instructions(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find("settings-note");
        window.click("settings-tab-appearance", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("settings-note").is_none(),
            "Appearance has none"
        );
    })
    .unwrap();
    let note = crate::settings::Section::Instructions.note().unwrap();
    assert!(
        note.contains("as a session starts") && note.contains("resume, /clear or compaction"),
        "{note}"
    );
}

/// Marks that carry hidden text after a visible character are dropped; a text of nothing else
/// is blank.
#[gpui_kit::test]
fn a_text_of_invisible_marks_is_blank(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let terminal = first_terminal(&ws, cx);
    let hidden =
        "\u{200B}\u{FEFF}\u{FE0F}\u{E0101}\u{061C}\u{180E}\u{034F}\u{115F}\u{1160}\u{3164}\u{FFA0}";
    assert_eq!(
        save(&ws, cx, None, &draft("Name", hidden)),
        Err("An instruction needs a text")
    );
    // A file edited by hand can hold one: it applies to no one.
    add(&ws, cx, draft("Hidden", hidden));
    assert_eq!(start(handle, &ws, cx, &terminal, "claude"), "");
}
