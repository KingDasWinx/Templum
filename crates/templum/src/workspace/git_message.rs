//! Commit messages from an agent: Generate ✨ beside a Git block's summary, its ⚙ (the model,
//! this repository's instructions) and Settings ▸ Git's Try it. What runs is
//! `templum_git::message`. See docs/specs/git.md, section 7.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::Notification;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, SharedString, Window, div, px};
use serde_json::Value;
use templum_api::client::Call;
use templum_git::message::{self, Failure, Request, Writer};
use templum_git::ops::{self, Op};

use crate::config::{self, Edit};
use crate::prompt;

use super::git_home::{heard, parsed};
use super::remote::{Deed, LINE_CHARS, TEXT_CHARS, Then};
use super::{GitAt, Workspace, show_notification};
use crate::git_block::Found;

/// The table Generate's settings live in.
const TABLE: &str = "git.commit_message";

/// Settings ▸ Git ▸ Try it: running (setting it stops the agent), or what came back.
#[derive(Debug)]
pub(crate) enum TryIt {
    Running(Arc<AtomicBool>),
    Answer(String),
    /// What went wrong, whole: the sentence, the exit code, stderr.
    Failed(String),
}

/// `[git.repositories]`'s key for a repository: its root with `/` on every OS, as
/// `git rev-parse --show-toplevel` prints it.
pub(crate) fn repository_key(root: &Path) -> String {
    root.to_string_lossy().replace('\\', "/")
}

impl Workspace {
    /// Generate ✨: the first time, asks whether the diff may go to the agent; then writes the
    /// message in the background. A click while it runs stops it.
    pub(crate) fn generate_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git() else {
            return;
        };
        // On a mirror home's agent writes it, and nothing here can stop it.
        if git.home.is_some() {
            if git.generating.is_none() {
                self.generate_at_home(window, cx);
            }
            return;
        }
        if let Some(cancel) = &git.generating {
            cancel.store(true, Ordering::Relaxed);
            return;
        }
        let Some(title) = self
            .commit_agent
            .as_ref()
            .filter(|a| a.ready)
            .map(|a| a.title.clone())
        else {
            return;
        };
        if self.config.git.commit_message.confirmed {
            self.generate_now(window, cx);
            return;
        }
        let entity = cx.entity();
        prompt::confirm(
            format!("Send your changes to {title}?"),
            format!(
                "Generate sends the diff of what you commit to {title}, which sends it to its \
                 provider and uses your account. Templum asks this once."
            ),
            "Generate",
            move |window, cx| {
                entity.update(cx, |ws, cx| {
                    ws.edit(
                        Edit::set(TABLE, "confirmed", true),
                        |c| c.git.commit_message.confirmed = true,
                        cx,
                    );
                    ws.generate_now(window, cx);
                })
            },
            window,
            cx,
        );
    }

    /// Generate ✨ on a mirror: home's agent writes the message from home's diff (`message`, on
    /// the slow lane), whatever agent this computer has. Not through the Git's queue: as here, it
    /// only reads and runs beside the Git's commands. Nothing stops it from here.
    fn generate_at_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(why) = self.why_not(Deed::Git("message")) {
            return self.warn(why, window, cx);
        }
        let at = self.here();
        let Some(computer) = at.home.clone() else {
            return;
        };
        let Some(git) = self.space.git() else {
            return;
        };
        let Found::Home(root) = &git.found else {
            return;
        };
        let Some(mut args) = self.home_args(&at.space, root) else {
            return;
        };
        let all = git.commit_all();
        let before = (
            git.summary.read(cx).value(),
            git.description.read(cx).value(),
        );
        if let (Some(args), Ok(Value::Object(op))) = (
            args.as_object_mut(),
            serde_json::to_value(Op::Message { all }),
        ) {
            args.extend(op);
        }
        if let Some(git) = self.space.git_mut() {
            git.generating = Some(Arc::new(AtomicBool::new(false)));
        }
        cx.notify();
        let call = Call {
            tool: "git".into(),
            args,
            slow: true,
        };
        let space = at.space.clone();
        let then: Then = Box::new(move |ws, answered, window, cx| {
            let Some(git) = ws.git_at(&at) else {
                return;
            };
            git.generating = None;
            cx.notify();
            let sent = match answered {
                Ok(answer) => parsed::<ops::Message>(heard("message", Ok(answer))).map_err(|e| {
                    let words = e.first_line().to_string();
                    (words.clone(), words)
                }),
                Err(refusal) => {
                    let details = refusal.details.unwrap_or_else(|| refusal.sentence.clone());
                    Err((refusal.sentence, details))
                }
            };
            match sent {
                // Home's words, as the fields hold them: one line, and a description of text.
                Ok(m) => ws.generated_parts(
                    at,
                    &computer,
                    before,
                    Ok((
                        super::bus::clean_to(&m.summary, LINE_CHARS),
                        super::remote::clean_text(&m.description, TEXT_CHARS),
                    )),
                    m.committed,
                    window,
                    cx,
                ),
                Err((sentence, details)) => {
                    ws.notify_agent_failure(&computer, sentence, details, window, cx)
                }
            }
        });
        self.remote_call(&space, call, then, cx);
    }

    /// The agent that writes this repository's messages, and how: Settings ▸ Git's rules, this
    /// repository's own over them, and the agent chosen there. None when no agent is ready.
    pub(crate) fn commit_writer(&self, root: &Path) -> Option<Writer> {
        let agent = self.commit_agent.as_ref().filter(|a| a.ready)?;
        let settings = self.config.git.commit_message.clone();
        // This repository's own rules win over Settings ▸ Git's.
        let instructions = self
            .config
            .git
            .repositories
            .get(&repository_key(root))
            .and_then(|r| r.commit_instructions.clone())
            .filter(|i| !i.trim().is_empty())
            .unwrap_or(settings.instructions);
        Some(Writer {
            command: agent.command.clone(),
            model: agent.model.clone(),
            env: agent.env.iter().cloned().collect(),
            instructions,
            template: settings
                .prompt
                .unwrap_or_else(|| message::PROMPT.to_string()),
            max_bytes: settings.max_diff_kb as usize * 1024,
            timeout: Duration::from_secs(settings.timeout),
        })
    }

    /// Reads what the commit would take, builds the request and runs the agent, off the UI
    /// thread and beside the block's own commands (it only reads, so they don't wait for it).
    fn generate_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.commit_agent.as_ref().map(|a| a.title.clone()) else {
            return;
        };
        let at = self.here();
        let Some(git) = self.space.git() else {
            return;
        };
        let Some(repo) = git.repo().cloned() else {
            return;
        };
        let Some(writer) = self.commit_writer(&repo.root) else {
            return;
        };
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let all = git.commit_all();
        let before = (
            git.summary.read(cx).value(),
            git.description.read(cx).value(),
        );
        let cancel = Arc::new(AtomicBool::new(false));
        git.generating = Some(cancel.clone());
        cx.notify();
        cx.spawn_in(window, async move |ws, cx| {
            let (answer, committed) = cx
                .background_executor()
                .spawn(async move { message::generate(&repo, &writer, all, &cancel) })
                .await;
            ws.update_in(cx, |ws, window, cx| {
                ws.generated(at, &title, before, answer, committed, window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// The agent's answer: in the fields, unless the summary or the description changed since
    /// the click (what was typed is never replaced) or a commit was made meanwhile (`committed`:
    /// the answer describes changes already committed); a failure says why, with Details; a
    /// cancel says nothing.
    // ponytail: a block closed or pointed at another folder meanwhile drops the answer, and its
    // agent runs on to its timeout; set `generating` in `set_git_folder` and on close if a slow
    // agent's cost matters.
    #[allow(clippy::too_many_arguments)]
    fn generated(
        &mut self,
        at: GitAt,
        agent: &str,
        before: (SharedString, SharedString),
        answer: Result<String, Failure>,
        committed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let parts = answer.and_then(|a| {
            message::split(&a).ok_or(Failure::Empty {
                stderr: String::new(),
            })
        });
        self.generated_parts(at, agent, before, parts, committed, window, cx);
    }

    /// `generated` once the answer is split into a summary and a description: here from the
    /// agent's text, on a mirror from home's.
    #[allow(clippy::too_many_arguments)]
    fn generated_parts(
        &mut self,
        at: GitAt,
        agent: &str,
        before: (SharedString, SharedString),
        parts: Result<(String, String), Failure>,
        committed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.git_at(&at) else {
            return;
        };
        git.generating = None;
        let (summary, description) = (git.summary.clone(), git.description.clone());
        cx.notify();
        match parts {
            Err(Failure::Cancelled) => {}
            Err(failure) => {
                self.notify_agent_failure(agent, failure.to_string(), failure.details(), window, cx)
            }
            Ok((s, d)) => {
                let left_out = if committed {
                    Some(format!(
                        "Left out {agent}'s message: a commit was made while it was writing"
                    ))
                } else if (summary.read(cx).value(), description.read(cx).value()) != before {
                    Some(format!(
                        "Kept your message: it changed while {agent} was writing one"
                    ))
                } else {
                    None
                };
                if let Some(why) = left_out {
                    show_notification(window, cx, Notification::info(why));
                    return;
                }
                summary.update(cx, |field, cx| field.set_value(s, window, cx));
                description.update(cx, |field, cx| field.set_value(d, window, cx));
            }
        }
    }

    /// Generate failed: the sentence, and Details with the exit code and what the agent wrote
    /// to stderr. No Run in a terminal: the command's files are gone by now.
    fn notify_agent_failure(
        &self,
        agent: &str,
        sentence: String,
        details: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        eprintln!("templum: {agent}: {sentence}");
        #[cfg(test)]
        super::told(&sentence);
        let title = SharedString::from(format!("{agent}: commit message"));
        let (text, mono) = (details, self.cell.font.family.clone());
        let notification = Notification::error(sentence)
            .title(title.clone())
            .autohide(false)
            .content(move |_, _, _| {
                let (title, text, mono) = (title.clone(), text.clone(), mono.clone());
                div()
                    .mt(px(6.0))
                    .child(
                        Button::new("generate-details")
                            .label("Details")
                            .small()
                            .on_click(move |_, window, cx| {
                                prompt::details(
                                    title.clone(),
                                    text.clone(),
                                    mono.clone(),
                                    window,
                                    cx,
                                )
                            }),
                    )
                    .into_any_element()
            });
        show_notification(window, cx, notification);
    }

    /// The model Generate asks for; empty: the agent's recommended one (the key goes).
    pub(crate) fn set_commit_model(&mut self, model: &str, cx: &mut Context<Self>) {
        let model = model.trim();
        if model.is_empty() {
            self.edit(
                Edit::remove(TABLE, "model"),
                |c| c.git.commit_message.model = None,
                cx,
            );
        } else {
            let chosen = model.to_string();
            self.edit(
                Edit::set(TABLE, "model", model),
                |c| c.git.commit_message.model = Some(chosen),
                cx,
            );
        }
    }

    /// `[git.repositories."<root>"].commit_instructions`: they win over Settings ▸ Git's in
    /// this repository; empty: the global ones again.
    // ponytail: removing the last key leaves an empty `[git.repositories."…"]` line in the file;
    // drop the table in `save_edits` when someone minds it.
    pub(crate) fn set_repo_instructions(
        &mut self,
        root: &Path,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let key = repository_key(root);
        let table = config::repository_table(&key);
        let text = text.trim().to_string();
        if text.is_empty() {
            self.edit(
                Edit::remove(table, "commit_instructions"),
                |c| {
                    c.git.repositories.remove(&key);
                },
                cx,
            );
        } else {
            self.edit(
                Edit::set(table, "commit_instructions", text.as_str()),
                |c| {
                    c.git
                        .repositories
                        .entry(key)
                        .or_default()
                        .commit_instructions = Some(text)
                },
                cx,
            );
        }
    }

    /// The ⚙'s Instructions for this repository…: a field holding them, Save writes them.
    pub(crate) fn ask_repo_instructions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self
            .space
            .git()
            .and_then(|g| g.repo())
            .map(|r| r.root.clone())
        else {
            return;
        };
        let current = self
            .config
            .git
            .repositories
            .get(&repository_key(&root))
            .and_then(|r| r.commit_instructions.clone())
            .unwrap_or_default();
        let entity = cx.entity();
        prompt::ask_text(
            "Instructions for this repository",
            "Save",
            current,
            move |text, _, cx| {
                entity.update(cx, |ws, cx| ws.set_repo_instructions(&root, &text, cx));
                true
            },
            window,
            cx,
        );
    }

    /// Settings ▸ Git ▸ Agent: Generate runs `id`; the model goes back to its recommended one
    /// (a model belongs to its agent).
    pub(crate) fn set_commit_agent(&mut self, id: &str, cx: &mut Context<Self>) {
        let chosen = id.to_string();
        self.edit(
            Edit::set(TABLE, "agent", id),
            |c| c.git.commit_message.agent = Some(chosen),
            cx,
        );
        self.edit(
            Edit::remove(TABLE, "model"),
            |c| c.git.commit_message.model = None,
            cx,
        );
    }

    /// Settings ▸ Git ▸ Try it: the agent on a small sample change (none of the person's code
    /// is sent), its answer or its whole error shown under the button. It runs even when the
    /// agent isn't ready: its error says why. A click while it runs stops it.
    pub(crate) fn try_message(&mut self, cx: &mut Context<Self>) {
        if let Some(TryIt::Running(cancel)) = &self.message_try {
            cancel.store(true, Ordering::Relaxed);
            return;
        }
        let Some(agent) = self.commit_agent.clone() else {
            return;
        };
        let settings = self.config.git.commit_message.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.message_try = Some(TryIt::Running(cancel.clone()));
        cx.notify();
        let template = settings
            .prompt
            .unwrap_or_else(|| message::PROMPT.to_string());
        let timeout = Duration::from_secs(settings.timeout);
        let instructions = settings.instructions;
        cx.spawn(async move |ws, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move {
                    let files = [message::SAMPLE_FILE.to_string()];
                    let request = message::request(
                        &template,
                        &Request {
                            instructions: &instructions,
                            files: &files,
                            diff: message::SAMPLE_DIFF,
                            recent: &[],
                            max_bytes: usize::MAX,
                        },
                    );
                    message::run(
                        &agent.command,
                        &agent.model,
                        &request,
                        &std::env::temp_dir(),
                        timeout,
                        &cancel,
                        agent.env.as_slice(),
                    )
                })
                .await;
            ws.update(cx, |ws, cx| {
                ws.message_try = match answer {
                    Err(Failure::Cancelled) => None,
                    Ok(answer) => Some(TryIt::Answer(answer.trim().to_string())),
                    Err(failure) => Some(TryIt::Failed(failure.details())),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
