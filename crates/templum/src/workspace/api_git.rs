//! The API's `git` tool at home (docs/specs/api.md): one operation of the Git popup, run in a
//! repository of a workspace. The folder and the repository are found off the UI thread, a
//! limited token's confinement is checked on the repository found (its root and its main tree),
//! and the operations of one repository run in turn, each killed after ten minutes. What an
//! operation does is `templum_git::ops`; this is the door.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::Context;
use serde_json::{Value, json};
use templum_api::{Caller, codes};
use templum_git::ops::{self, Answer, Done, Failure, Found, Home, Op};
use templum_git::trash::Trash;
use templum_git::{GitError, Repo};
use templum_mcp::{Replier, Reply, rpc};

use super::api::{Refusal, answer_line, not_found, prefix_of, refuses_share};
use super::bus::clean_title;
use super::{Space, Workspace};

/// A git call is stopped after this long: one that hangs on a network or a prompt must not hold
/// its repository's queue.
pub(super) const GIT_DEADLINE: Duration = Duration::from_secs(600);

/// The most of git's stderr a refusal carries in `details`.
pub(super) const DETAILS_MOST: usize = 65_536;

/// The next call on a repository, run on the UI thread when the one before it is answered.
pub(super) type GitTurn = Box<dyn FnOnce(&mut Workspace, &mut Context<Workspace>)>;

/// One `git` call on its way: who asked, what, and what home lends it.
struct Call {
    id: Value,
    reply: Replier,
    /// The workspace's id.
    space: String,
    /// The workspace's name, as `clean_title` says it.
    name: String,
    computer: String,
    op: Op,
    trash: Option<Trash>,
    global_author: bool,
}

/// Why a call stopped before git ran an operation.
enum Stop {
    /// Outside the folders of a limited token: 403.
    Outside(String),
    /// A refusal, or a git failure.
    Failed(Failure),
}

/// What was found at the folder.
enum Located {
    Repo(Repo),
    /// No repository there, and the answer needs none (`find`, `init`).
    Answer(Answer),
}

impl Workspace {
    /// `git`: the operation `op` in the repository of `workspace`'s folder (or of `folder`). Ok
    /// with a line is answered now; Ok(None) answers later, once its repository's turn comes.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn api_git(
        &mut self,
        caller: &Caller,
        id: &Value,
        reply: &Replier,
        workspace: String,
        folder: Option<String>,
        op: Op,
        cx: &mut Context<Self>,
    ) -> Result<Option<String>, Refusal> {
        // A mirror's blocks and folders are home's: nothing here reaches them.
        let space = match self.space_of(&workspace, true) {
            Ok(space) => space,
            Err(refused) => {
                return match self.space_called(&workspace).and_then(Space::mirror_words) {
                    Some(_) => Ok(Some(answer_line(id, &refused, None))),
                    None => Err(not_found("workspace", &workspace)),
                };
            }
        };
        let Some((name, home_folder)) = self
            .space_named(&space)
            .map(|s| (clean_title(&s.name), s.folder.clone()))
        else {
            return Err(not_found("workspace", &workspace));
        };
        let refuse = |why: String| Ok(Some(answer_line(id, &Reply::err(why), None)));
        let folder = match folder {
            // A limited token's folder was made real and confined by `api_allows`.
            Some(folder) if caller.workspaces.is_some() => PathBuf::from(folder),
            Some(folder) => {
                let path = crate::launcher::expand(&folder);
                match &home_folder {
                    _ if !path.is_relative() => path,
                    Some(home) => home.join(path),
                    None => return refuse(format!("{folder} isn't a folder on this computer.")),
                }
            }
            None => match home_folder {
                Some(home) => home,
                None => {
                    return refuse(
                        "This workspace has no folder: name one with \"folder\".".to_string(),
                    );
                }
            },
        };
        let roots = caller.workspaces.is_some().then(|| {
            let seen = self.api_scope(cx).visible(caller);
            self.confined_roots(&seen)
        });
        let program = self.config.git.program().to_string();
        let call = Call {
            id: id.clone(),
            reply: reply.clone(),
            space,
            name,
            computer: templum_api::computer(),
            op,
            trash: self.trash.as_ref().map(Trash::for_api),
            global_author: caller.workspaces.is_none(),
        };
        cx.spawn(async move |ws, cx| {
            let (c, p, f, r) = (call.op.clone(), program, folder.clone(), roots);
            let name = call.name.clone();
            let located = cx
                .background_executor()
                .spawn(async move { locate(&p, &f, r.as_deref(), &name, &c) })
                .await;
            match located {
                Ok(Located::Repo(repo)) => {
                    ws.update(cx, |ws, cx| ws.git_queue(repo, call, cx)).ok();
                }
                Ok(Located::Answer(answer)) => {
                    let _ = call.reply.send(git_line(&call.id, &call.space, &answer));
                }
                Err(stop) => {
                    let _ = call.reply.send(stop_line(&call, stop));
                }
            }
        })
        .detach();
        Ok(None)
    }

    /// The repository is found: Generate's writer is taken (home's agent, once the person has
    /// allowed the diff to go to it), and the call waits its turn on the repository.
    fn git_queue(&mut self, repo: Repo, call: Call, cx: &mut Context<Self>) {
        let mut writer = None;
        if matches!(call.op, Op::Message { .. }) {
            let why = if !self.config.git.commit_message.confirmed {
                Some(format!(
                    "Generate a message on {} once first: it asks there whether the diff may go \
                     to the agent's provider.",
                    call.computer
                ))
            } else {
                writer = self.commit_writer(&repo.root);
                writer.is_none().then(|| {
                    format!(
                        "No commit-message agent is ready on {}: choose one in Settings ▸ Git \
                         there.",
                        call.computer
                    )
                })
            };
            if let Some(why) = why {
                let _ = call
                    .reply
                    .send(answer_line(&call.id, &Reply::err(why), None));
                return;
            }
        }
        let root = repo.root.clone();
        let reply = call.reply.clone();
        self.git_turn(
            root.clone(),
            Box::new(move |_, cx| {
                cx.spawn(async move |ws, cx| {
                    let line = cx
                        .background_executor()
                        .spawn(async move {
                            let repo = repo.with_deadline(Instant::now() + GIT_DEADLINE);
                            let home = Home {
                                trash: call.trash.as_ref(),
                                writer: writer.as_ref(),
                                global_author: call.global_author,
                            };
                            match ops::run(&repo, call.op.clone(), &home) {
                                Ok(answer) => git_line(&call.id, &call.space, &answer),
                                Err(failure) => git_refusal(&call.id, &failure, &call.computer),
                            }
                        })
                        .await;
                    let _ = reply.send(line);
                    ws.update(cx, |ws, cx| ws.git_turn_done(&root, cx)).ok();
                })
                .detach();
            }),
            cx,
        );
    }

    /// Runs `turn` now when no call is running on the repository at `root`, else after those
    /// before it: one operation at a time per repository, as the popup's queue does.
    pub(super) fn git_turn(&mut self, root: PathBuf, turn: GitTurn, cx: &mut Context<Self>) {
        match self.api.git.get_mut(&root) {
            Some(queue) => queue.push_back(turn),
            None => {
                self.api.git.insert(root, Default::default());
                turn(self, cx);
            }
        }
    }

    /// The running call on `root` is answered: the next one starts, or the queue goes.
    pub(super) fn git_turn_done(&mut self, root: &Path, cx: &mut Context<Self>) {
        match self
            .api
            .git
            .get_mut(root)
            .and_then(|queue| queue.pop_front())
        {
            Some(next) => next(self, cx),
            None => {
                self.api.git.remove(root);
            }
        }
    }
}

/// The repository of `folder`, off the UI thread. A token limited to some workspaces (`roots`)
/// gets none outside their folders: neither the repository's root nor its main working tree.
fn locate(
    program: &str,
    folder: &Path,
    roots: Option<&[PathBuf]>,
    workspace: &str,
    op: &Op,
) -> Result<Located, Stop> {
    let outside = || {
        Stop::Outside(format!(
            "That repository lies outside {workspace}'s folder."
        ))
    };
    let refused = |why: String| Stop::Failed(Failure::Refused(why));
    if !folder.is_dir() {
        return Err(refused(format!(
            "{} isn't a folder on this computer.",
            folder.display()
        )));
    }
    if roots.is_some_and(|roots| inside(folder, roots).is_none()) {
        return Err(outside());
    }
    let repo = match Repo::discover(program, folder).map_err(git_failed)? {
        Some(repo) => repo,
        None => {
            let found = |repo: Option<&Repo>, text: String| {
                let root = repo.map(|r| r.root.to_string_lossy().into_owned());
                Ok(Located::Answer(Answer {
                    text,
                    git: Done::Found(Found {
                        root,
                        main_tree: None,
                    }),
                }))
            };
            return match op {
                Op::Find => found(None, format!("No repository at {}.", folder.display())),
                Op::Init => {
                    let repo = Repo::init(program, folder).map_err(git_failed)?;
                    let text = format!("Made a repository at {}.", repo.root.display());
                    found(Some(&repo), text)
                }
                _ => Err(refused(format!(
                    "{} isn't in a Git repository: init makes one.",
                    folder.display()
                ))),
            };
        }
    };
    match roots {
        Some(roots) if !repo_inside(&repo, roots) => Err(outside()),
        _ => Ok(Located::Repo(repo)),
    }
}

fn git_failed(e: GitError) -> Stop {
    Stop::Failed(Failure::Git(e))
}

/// `path`'s real path, when it lies inside one of `roots`, links followed. A share of another
/// computer is never asked about: it would be offered the person's credentials.
pub(super) fn inside(path: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    if refuses_share(
        prefix_of(path),
        &roots.iter().map(|r| prefix_of(r)).collect::<Vec<_>>(),
    ) {
        return None;
    }
    dunce::canonicalize(path)
        .ok()
        .filter(|real| roots.iter().any(|root| real.starts_with(root)))
}

/// Both the repository's working tree and its main working tree (a linked worktree's lies
/// elsewhere, and a `.git` file planted in a folder can point it anywhere) are inside `roots`.
/// A bare repository has no working tree: it is never inside.
fn repo_inside(repo: &Repo, roots: &[PathBuf]) -> bool {
    let Ok(Some(main)) = repo.main_tree() else {
        return false;
    };
    inside(&repo.root, roots).is_some() && inside(&main, roots).is_some()
}

/// A `tools/call` answer for a git operation: its sentence, its result and the workspace.
fn git_line(id: &Value, workspace: &str, answer: &Answer) -> String {
    rpc::result(
        id,
        json!({
            "content": [{ "type": "text", "text": answer.text }],
            "git": serde_json::to_value(&answer.git).unwrap_or(Value::Null),
            "target": { "workspace": workspace, "block": null },
        }),
    )
}

/// What a failed call says: the sentence, and git's whole stderr (cut at `DETAILS_MOST`) as
/// `details`.
fn git_refusal(id: &Value, failure: &Failure, computer: &str) -> String {
    let (text, details) = match failure {
        Failure::Git(e) if e.needs_input() => (
            format!("git on {computer} asks for a login: run it in a terminal there."),
            Some(e.stderr.clone()),
        ),
        Failure::Git(e) if e.cancelled() => (
            format!("git on {computer} ran for 10 minutes and was stopped."),
            None,
        ),
        Failure::Git(e) if e.too_large() => (
            format!("The output of git is too large to read from {computer}: ask for less."),
            None,
        ),
        Failure::Git(e) => (e.first_line().to_string(), Some(e.stderr.clone())),
        Failure::Agent(f) => (f.to_string(), Some(f.details())),
        other => (other.sentence(), None),
    };
    let mut result = json!({ "isError": true, "content": [{ "type": "text", "text": text }] });
    if let Some(details) = details.filter(|d| !d.is_empty()) {
        result["details"] = json!(cut(&details, DETAILS_MOST));
    }
    rpc::result(id, result)
}

/// `text` cut to at most `most` bytes, at a character's boundary.
fn cut(text: &str, most: usize) -> &str {
    let mut end = text.len().min(most);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The line that answers a call that stopped before an operation ran.
fn stop_line(call: &Call, stop: Stop) -> String {
    match stop {
        Stop::Outside(why) => rpc::error(&call.id, codes::SCOPE, &why),
        Stop::Failed(failure) => git_refusal(&call.id, &failure, &call.computer),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(stderr: &str, code: Option<i32>) -> Failure {
        Failure::Git(GitError {
            command: "git push".into(),
            code,
            stderr: stderr.into(),
            missing: false,
        })
    }

    fn result_of(line: String) -> Value {
        serde_json::from_str::<Value>(&line).unwrap()["result"].clone()
    }

    #[test]
    fn details_are_cut_at_a_characters_boundary() {
        assert_eq!(cut("abc", 10), "abc");
        // "é" is two bytes: 3 bytes cut inside the second one.
        assert_eq!(cut("éé", 3), "é");
        let long = "é".repeat(DETAILS_MOST);
        let line = git_refusal(&json!(1), &failed(&long, Some(1)), "home");
        let details = result_of(line)["details"].as_str().unwrap().to_string();
        assert_eq!(details.len(), DETAILS_MOST);
    }

    #[test]
    fn a_refusal_says_git_s_first_line_the_login_or_the_deadline() {
        let words = |failure: Failure| {
            let result = result_of(git_refusal(&json!(1), &failure, "home"));
            assert_eq!(result["isError"], true);
            (
                result["content"][0]["text"].as_str().unwrap().to_string(),
                result["details"].as_str().map(str::to_string),
            )
        };
        assert_eq!(
            words(failed("hint: wait\nfatal: bad\n", Some(128))),
            (
                "fatal: bad".to_string(),
                Some("hint: wait\nfatal: bad\n".to_string())
            )
        );
        assert_eq!(
            words(failed(
                "fatal: could not read Username for 'https://x': terminal prompts disabled",
                Some(128)
            ))
            .0,
            "git on home asks for a login: run it in a terminal there."
        );
        // A command killed at the deadline: no stderr to show.
        let killed = failed("Cancelled", None);
        assert_eq!(
            words(killed),
            (
                "git on home ran for 10 minutes and was stopped.".to_string(),
                None
            )
        );
        // A command killed for writing too much.
        assert_eq!(
            words(failed("Too large", None)),
            (
                "The output of git is too large to read from home: ask for less.".to_string(),
                None
            )
        );
        // A refusal of the operation's own has no details.
        assert_eq!(
            words(Failure::Refused("a.txt has nothing staged.".into())),
            ("a.txt has nothing staged.".to_string(), None)
        );
    }
}
