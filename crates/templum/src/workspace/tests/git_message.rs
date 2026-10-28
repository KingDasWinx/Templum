//! Commit messages from an agent: Generate ✨, its ⚙, the agents' commands and Settings ▸ Git.

use super::*;

#[gpui_kit::test]
fn the_chosen_agent_is_ready_and_follows_the_config(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        concat!(
            "[agents.mine]\nname = \"Mine\"\ncommand = \"mine\"\n",
            // git is on every CI runner: a program that is installed.
            "generate = \"git {model}\"\nmodels = [\"fast\", \"smart\"]\n",
            "[git.commit_message]\nagent = \"mine\"\n",
        ),
    );
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    ws.read_with(cx, |ws, _| {
        let agent = ws.commit_agent.as_ref().unwrap();
        assert_eq!(
            (agent.id.as_str(), agent.model.as_str(), agent.ready),
            ("mine", "fast", true)
        );
    });
    ws.update(cx, |ws, cx| {
        ws.edit(
            Edit::set("git.commit_message", "model", "smart"),
            |c| c.git.commit_message.model = Some("smart".into()),
            cx,
        )
    });
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.commit_agent.as_ref().unwrap().model, "smart")
    });
    write_config(dir.path(), "[git.commit_message]\nagent = \"mine\"\n");
    reload(handle, &ws, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.commit_agent, None, "mine is gone: never another agent")
    });
}

#[gpui_kit::test]
fn the_agents_dialog_sets_a_commit_message_command(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-agents", cx);
        window.render_frame(cx);
        window.click("edit-agent-claude", cx);
        window.render_frame(cx);
    })
    .unwrap();
    // The dialog slides in for 0.25 s first: presses meanwhile are lost.
    std::thread::sleep(std::time::Duration::from_millis(300));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.within("dialog").try_find("agent-models").is_some());
        window.within("dialog").click("agent-generate", cx);
        window.press("end", cx);
        window.input(" --verbose", cx);
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let claude = &ws.config.agents["claude"];
        assert_eq!(
            claude.generate.as_deref(),
            Some("claude -p --model {model} --tools= --setting-sources= --verbose")
        );
        assert_eq!(claude.models, None, "the models, unchanged, stay Templum's");
        assert_eq!(claude.command, None);
    });
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
    assert!(
        text.contains(
            "generate = \"claude -p --model {model} --tools= --setting-sources= --verbose\""
        ),
        "{text}"
    );
}

/// An agent's program in `dir`, run as `sh <it>`: it keeps the request it gets (`request.txt`)
/// and its arguments (`args.txt`) there, then runs `then`, a shell line. `sh` reads it rather
/// than running it: another test's fork may still hold it open for writing, and Linux then
/// refuses to run it ("Text file busy").
#[cfg(unix)]
fn fake_agent(dir: &std::path::Path, then: &str) -> std::path::PathBuf {
    let path = dir.join("fake-agent");
    let keep = dir.display();
    std::fs::write(
        &path,
        format!("cat > '{keep}/request.txt'\necho \"$@\" > '{keep}/args.txt'\n{then}\n"),
    )
    .unwrap();
    path
}

/// config.toml with `agent` as the person's agent "fake", chosen for commit messages.
#[cfg(unix)]
fn fake_config(agent: &std::path::Path, confirmed: bool) -> String {
    format!(
        "[agents.fake]\nname = \"Fake\"\ncommand = \"fake\"\ngenerate = \"sh {} {{model}}\"\n\
         models = [\"fast\", \"smart\"]\n\n[git.commit_message]\nagent = \"fake\"\n\
         confirmed = {confirmed}\ninstructions = \"Say why.\"\n",
        agent.display()
    )
}

/// What the fake agent answers by default.
#[cfg(unix)]
const ANSWER: &str = "printf 'Fix the parser\\n\\nIt read past the end.\\n'";

/// A repository with a.txt's change staged, a config folder with the fake agent running
/// `then`, and the Git popup on the repository.
#[cfg(unix)]
fn staged_with_agent(
    cx: &mut TestAppContext,
    then: &str,
    confirmed: bool,
) -> (
    AnyWindowHandle,
    Entity<Workspace>,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let repo = committed_repo();
    std::fs::write(repo.path().join("a.txt"), "two\n").unwrap();
    git_run(repo.path(), &["add", "a.txt"]);
    let config = tempfile::tempdir().unwrap();
    let agent = fake_agent(config.path(), then);
    write_config(config.path(), &fake_config(&agent, confirmed));
    let (handle, ws) = open_with(cx, Some(config.path().to_path_buf()));
    open_git(handle, &ws, cx, repo.path());
    (handle, ws, repo, config)
}

/// The summary and the description as they are.
#[cfg(unix)]
fn fields(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> (String, String) {
    let (summary, description) = git_of(ws, cx, |g| (g.summary.clone(), g.description.clone()));
    ws.read_with(cx, |_, cx| {
        (
            summary.read(cx).value().to_string(),
            description.read(cx).value().to_string(),
        )
    })
}

#[cfg(unix)]
fn click_generate(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("git-generate", cx);
        window.render_frame(cx);
    })
    .unwrap();
}

#[cfg(unix)]
#[gpui_kit::test]
fn generate_writes_the_message_from_what_is_staged(cx: &mut TestAppContext) {
    let (handle, ws, repo, config) = staged_with_agent(cx, ANSWER, true);
    click_generate(handle, cx);
    git_of(&ws, cx, |g| assert!(g.generating.is_some(), "spins"));
    settle(handle, cx);
    assert_eq!(
        fields(&ws, cx),
        (
            "Fix the parser".to_string(),
            "It read past the end.".to_string()
        )
    );
    git_of(&ws, cx, |g| assert!(g.generating.is_none()));
    let request = std::fs::read_to_string(config.path().join("request.txt")).unwrap();
    for part in ["Say why.", "M a.txt", "+two", "First"] {
        assert!(request.contains(part), "{part} in {request}");
    }
    assert_eq!(
        std::fs::read_to_string(config.path().join("args.txt")).unwrap(),
        "fast\n",
        "the recommended model"
    );
    assert_eq!(
        git_run(repo.path(), &["log", "-1", "--format=%s"]),
        "First\n",
        "nothing committed"
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn generate_shows_only_with_something_to_describe(cx: &mut TestAppContext) {
    let repo = committed_repo();
    let config = tempfile::tempdir().unwrap();
    let agent = fake_agent(config.path(), ANSWER);
    write_config(config.path(), &fake_config(&agent, true));
    let (handle, ws) = open_with(cx, Some(config.path().to_path_buf()));
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("git-generate").is_none(), "nothing changed");
    })
    .unwrap();
    // Nothing staged, a new file: Commit all changes, which Generate describes too.
    std::fs::write(repo.path().join("new.txt"), "brand new\n").unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.refresh_git(window, cx))
    })
    .unwrap();
    settle(handle, cx);
    click_generate(handle, cx);
    settle(handle, cx);
    let request = std::fs::read_to_string(config.path().join("request.txt")).unwrap();
    assert!(
        request.contains("A new.txt") && request.contains("+brand new"),
        "{request}"
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn generate_asks_once_before_sending_the_diff(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let (handle, ws, _repo, config) = staged_with_agent(cx, ANSWER, false);
    click_generate(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx), "asks first");
        window.within("dialog").click("cancel", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    assert!(
        !config.path().join("request.txt").exists(),
        "declined: nothing sent"
    );
    click_generate(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx), "asked again");
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    assert_eq!(fields(&ws, cx).0, "Fix the parser");
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(config.path().join(crate::config::FILE)).unwrap();
    assert!(text.contains("confirmed = true"), "{text}");
    std::fs::remove_file(config.path().join("request.txt")).unwrap();
    click_generate(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(!window.has_active_dialog(cx), "asked once")
    })
    .unwrap();
    settle(handle, cx);
    assert!(config.path().join("request.txt").exists(), "sent at once");
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_failing_agent_leaves_the_fields_and_says_why(cx: &mut TestAppContext) {
    let (handle, ws, _repo, _config) = staged_with_agent(cx, "echo oops >&2; exit 3", true);
    let summary = git_of(&ws, cx, |g| g.summary.clone());
    cx.update_window(handle, |_, window, cx| {
        summary.update(cx, |s, cx| s.set_value("Mine", window, cx));
    })
    .unwrap();
    click_generate(handle, cx);
    settle(handle, cx);
    assert_eq!(notification_count(handle, cx), 1, "says why, with Details");
    assert_eq!(fields(&ws, cx).0, "Mine", "left as it was");
    git_of(&ws, cx, |g| assert!(g.generating.is_none()));
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_agent_past_its_timeout_is_stopped(cx: &mut TestAppContext) {
    let (handle, ws, _repo, _config) = staged_with_agent(cx, "sleep 30", true);
    // Below the settings' 5 s floor, for a quick test.
    ws.update(cx, |ws, _| ws.config.git.commit_message.timeout = 1);
    let started = std::time::Instant::now();
    click_generate(handle, cx);
    settle(handle, cx);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "stopped, not waited for"
    );
    assert_eq!(notification_count(handle, cx), 1);
    assert_eq!(told_so_far(), ["The agent didn't answer within 1 s"]);
    assert_eq!(fields(&ws, cx), (String::new(), String::new()));
    git_of(&ws, cx, |g| assert!(g.generating.is_none()));
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_second_click_cancels_and_a_message_changed_meanwhile_is_kept(cx: &mut TestAppContext) {
    let (handle, ws, _repo, _config) = staged_with_agent(cx, ANSWER, true);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("git-generate", cx);
        window.render_frame(cx);
        window.click("git-generate", cx);
    })
    .unwrap();
    settle(handle, cx);
    assert_eq!(notification_count(handle, cx), 0, "a cancel says nothing");
    assert_eq!(fields(&ws, cx), (String::new(), String::new()));
    git_of(&ws, cx, |g| assert!(g.generating.is_none()));
    // Typed while the agent writes: what was typed stays.
    let summary = git_of(&ws, cx, |g| g.summary.clone());
    cx.update_window(handle, |_, window, cx| {
        window.click("git-generate", cx);
        summary.update(cx, |s, cx| s.set_value("Mine", window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    assert_eq!(fields(&ws, cx).0, "Mine");
    assert_eq!(notification_count(handle, cx), 1, "Kept your message");
}

#[cfg(unix)]
#[gpui_kit::test]
fn the_generate_menu_sets_the_model_and_this_repositorys_instructions(cx: &mut TestAppContext) {
    // The question opens at once: what is typed reaches its field.
    no_motion(cx);
    let (handle, ws, _repo, config) = staged_with_agent(cx, ANSWER, true);
    let choose = |cx: &mut TestAppContext, item: usize| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("git-generate-menu", cx);
            window.render_frame(cx);
            // fast, smart, —, Instructions for this repository…: a separator counts.
            window.within("popup-menu").click(item, cx);
            window.render_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
    };
    choose(cx, 1);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.commit_agent.as_ref().unwrap().model, "smart")
    });
    choose(cx, 3);
    cx.update_window(handle, |_, window, cx| {
        window.input("Prefix the summary with the ticket id.", cx);
        window.press("enter", cx);
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    let root = git_of(&ws, cx, |g| g.repo().unwrap().root.clone());
    let key = crate::workspace::git_message::repository_key(&root);
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.config.git.repositories[&key]
                .commit_instructions
                .as_deref(),
            Some("Prefix the summary with the ticket id.")
        );
    });
    click_generate(handle, cx);
    settle(handle, cx);
    let request = std::fs::read_to_string(config.path().join("request.txt")).unwrap();
    assert!(
        request.contains("Prefix the summary with the ticket id.") && !request.contains("Say why."),
        "this repository's rules, not the global ones: {request}"
    );
    assert_eq!(
        std::fs::read_to_string(config.path().join("args.txt")).unwrap(),
        "smart\n"
    );
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(config.path().join(crate::config::FILE)).unwrap();
    assert!(
        text.contains("model = \"smart\"") && text.contains("[git.repositories."),
        "{text}"
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn settings_git_chooses_the_agent_and_tries_it(cx: &mut TestAppContext) {
    use crate::workspace::TryIt;
    let config = tempfile::tempdir().unwrap();
    let agent = fake_agent(
        config.path(),
        "printf 'Fix the greeting\\n\\nIt said Helo.\\n'",
    );
    write_config(
        config.path(),
        &format!(
            "[agents.fake]\nname = \"Fake\"\ncommand = \"fake\"\ngenerate = \"sh {} {{model}}\"\n\
             models = [\"fast\", \"smart\"]\n",
            agent.display()
        ),
    );
    let (handle, ws) = open_with(cx, Some(config.path().to_path_buf()));
    // None chosen: the first installed one with a command (Claude Code, Codex, then Fake).
    let first = ["claude", "codex"]
        .into_iter()
        .find(|p| templum_terminal::installed(p))
        .unwrap_or("fake");
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.commit_agent.as_ref().map(|a| a.id.as_str()),
            Some(first),
            "none chosen: the first installed one with a command"
        )
    });
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-git", cx);
        window.render_frame(cx);
        window
            .within("field-git.commit_message.agent")
            .click("menu-git.commit_message.agent", cx);
        window.render_frame(cx);
        // Claude Code, Codex, Fake.
        window.within("popup-menu").click(2usize, cx);
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let agent = ws.commit_agent.as_ref().unwrap();
        assert_eq!(
            (agent.id.as_str(), agent.model.as_str(), agent.ready),
            ("fake", "fast", true)
        );
    });
    cx.update_window(handle, |_, window, cx| {
        // Try it is the last row.
        window.scroll(
            "settings-scroll",
            ScrollDelta::Pixels(point(px(0.), px(-5000.))),
            cx,
        );
        window.render_frame(cx);
        window.click("git-try-message", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    ws.read_with(cx, |ws, _| match &ws.message_try {
        Some(TryIt::Answer(answer)) => assert_eq!(answer, "Fix the greeting\n\nIt said Helo."),
        _ => panic!("no answer"),
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("git-try-result").is_some(),
            "shown under it"
        );
    })
    .unwrap();
    let request = std::fs::read_to_string(config.path().join("request.txt")).unwrap();
    assert!(
        request.contains("M src/greet.rs") && request.contains("Hello, {name}!"),
        "the sample, its braces as written: {request}"
    );
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(config.path().join(crate::config::FILE)).unwrap();
    assert!(
        text.contains("[git.commit_message]") && text.contains("agent = \"fake\""),
        "{text}"
    );
}

#[gpui_kit::test]
fn try_it_shows_the_whole_error(cx: &mut TestAppContext) {
    use crate::workspace::TryIt;
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        concat!(
            "[agents.mine]\nname = \"Mine\"\ncommand = \"mine\"\n",
            // git refuses an option it doesn't know, on every OS.
            "generate = \"git {model}\"\nmodels = [\"--templum-no-such-option\"]\n",
            "[git.commit_message]\nagent = \"mine\"\n",
        ),
    );
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    ws.update(cx, |ws, cx| ws.try_message(cx));
    settle(handle, cx);
    ws.read_with(cx, |ws, _| match &ws.message_try {
        Some(TryIt::Failed(text)) => {
            assert!(text.contains("failed (exit code"), "{text}");
            assert!(
                text.contains("templum-no-such-option"),
                "git's own words: {text}"
            );
        }
        _ => panic!("no failure"),
    });
}

#[gpui_kit::test]
fn the_git_section_writes_the_rules_and_the_prompt_back_at_its_default_goes(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-git", cx);
        window.render_frame(cx);
        for row in ["agent", "model", "other_model", "timeout", "max_diff_kb"] {
            let id = gpui_kit::ElementId::Name(format!("field-git.commit_message.{row}").into());
            assert!(window.try_find(id).is_some(), "{row}");
        }
    })
    .unwrap();
    // What the text areas call on each change, as the shell program's tests do.
    ws.update(cx, |ws, cx| {
        crate::settings::set_commit_prompt(ws, "Only {diff}", cx);
        crate::settings::set_commit_instructions(ws, "Imperative.", cx);
    });
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        let m = &ws.config.git.commit_message;
        assert_eq!(m.prompt.as_deref(), Some("Only {diff}"));
        assert_eq!(m.instructions, "Imperative.");
    });
    ws.update(cx, |ws, cx| {
        crate::settings::set_commit_prompt(ws, templum_git::message::PROMPT, cx)
    });
    frame(handle, cx);
    ws.update(cx, |ws, _| ws.save());
    let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
    assert!(
        !text.contains("prompt") && text.contains("instructions = \"Imperative.\""),
        "{text}"
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_commit_made_while_the_agent_writes_leaves_its_answer_out(cx: &mut TestAppContext) {
    // The agent's answer would describe changes already committed (here from a terminal; from
    // the block, the fields were cleared too).
    let (handle, ws, repo, _config) = staged_with_agent(
        cx,
        &format!("git commit -q -m 'From a terminal'\n{ANSWER}"),
        true,
    );
    click_generate(handle, cx);
    settle(handle, cx);
    assert_eq!(
        git_run(repo.path(), &["log", "-1", "--format=%s"]),
        "From a terminal\n"
    );
    assert_eq!(fields(&ws, cx), (String::new(), String::new()));
    assert_eq!(notification_count(handle, cx), 1, "says why");
    git_of(&ws, cx, |g| assert!(g.generating.is_none()));
}

#[cfg(unix)]
#[gpui_kit::test]
fn generate_can_be_stopped_once_the_changes_are_committed(cx: &mut TestAppContext) {
    let repo = committed_repo();
    let config = tempfile::tempdir().unwrap();
    let agent = fake_agent(config.path(), ANSWER);
    write_config(config.path(), &fake_config(&agent, true));
    let (handle, ws) = open_with(cx, Some(config.path().to_path_buf()));
    open_git(handle, &ws, cx, repo.path());
    // Writing, with nothing left to commit (it was committed meanwhile): the spinner stays.
    ws.update(cx, |ws, _| {
        ws.space.git_mut().unwrap().generating = Some(std::sync::Arc::new(
            std::sync::atomic::AtomicBool::new(false),
        ));
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("git-generate").is_some());
    })
    .unwrap();
}
