//! The Git popup on a mirrored workspace (docs/specs/templum-to-templum.md, The Git popup on a
//! remote workspace): every read and act of the popup is an operation of home's `git` tool, sent
//! through `remote_call`; nothing is run or read on this computer, where home's paths may name
//! other folders. A job runs here or at home through [`Workspace::git_op`] and
//! [`Workspace::act_op`], and its `done` closure is the same either way.

use std::time::Duration;

use gpui_kit::{Context, Window};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use templum_api::client::{Answered, Call};
use templum_canvas::BlockId;
use templum_git::ops::{self, Op};
use templum_git::{GitError, Repo};
use templum_mcp::control::Kind;

use super::remote::{Deed, Then};
use super::{GitAt, Workspace};
use crate::git_block::{Found, Git, Tab};

/// The panel's working diff is read again this long after the last read even when the status
/// is the same (a file edited again at home shows its new lines).
pub(super) const HOME_DIFF_AGAIN: Duration = Duration::from_secs(10);

/// Where a job runs.
pub(super) enum Runs {
    Here(Repo),
    /// At home: the arguments every call carries (`workspace`, `folder`).
    Home(Value),
}

/// Fetch, pull, push, sync, publish and Generate may take minutes, and so may what runs git's
/// hooks (pre-commit and commit-msg on a commit, post-checkout on a switch, post-merge on a
/// merge): they go on the client's slow lane.
pub(super) fn slow(op: &Op) -> bool {
    matches!(
        op,
        Op::Fetch
            | Op::Pull { .. }
            | Op::Push
            | Op::PushBranch { .. }
            | Op::Publish { .. }
            | Op::Sync { .. }
            | Op::Message { .. }
            | Op::Commit { .. }
            | Op::Switch { .. }
            | Op::NewBranch { .. }
            | Op::Checkout { .. }
            | Op::Merge { .. }
            | Op::Revert { .. }
            | Op::StashApply { .. }
            | Op::StashPop { .. }
            | Op::Continue
    )
}

/// Home's answer to `op` as the popup's jobs take a git result: its `git`, or a refusal as the
/// error git gave (home's `details` are its stderr, else its sentence).
pub(super) fn heard(op: &str, answered: Answered) -> Result<Value, GitError> {
    match answered {
        Ok(mut answer) => Ok(answer.get_mut("git").map_or(Value::Null, Value::take)),
        Err(refusal) => Err(GitError {
            command: format!("git {op}"),
            code: None,
            stderr: refusal
                .details
                .filter(|d| !d.trim().is_empty())
                .unwrap_or(refusal.sentence),
            missing: false,
        }),
    }
}

/// What home's `git` gave, as the type the job returns. Its strings stay as home gave them: the
/// ones sent back to home (a hunk's lines in a patch, a path) must be what home has, and the
/// panel draws the same raw git text from a local repository.
pub(super) fn parsed<X: DeserializeOwned>(heard: Result<Value, GitError>) -> Result<X, GitError> {
    serde_json::from_value(heard?).map_err(|e| GitError {
        command: "git".into(),
        code: None,
        // serde quotes the value it met: a short line, not home's megabyte.
        stderr: format!(
            "The answer couldn't be read: {}",
            super::bus::clean_to(&e.to_string(), 200)
        ),
        missing: false,
    })
}

/// Home's `find` or `init`: where the repository is. The root is every later call's `folder`,
/// so one that cleaning would change is refused, never altered.
pub(super) fn found(heard: Result<Value, GitError>) -> Result<ops::Found, GitError> {
    let found: ops::Found = parsed(heard)?;
    match &found.root {
        Some(root) if super::bus::clean_to(root, super::remote::LINE_CHARS) != *root => {
            Err(GitError {
                command: "git find".into(),
                code: None,
                stderr: "The repository's folder at home has a name this computer can't show."
                    .into(),
                missing: false,
            })
        }
        _ => Ok(found),
    }
}

/// `path` is `root` or inside it, by either separator (home's paths are home's).
pub(crate) fn lies_in(path: &str, root: &str) -> bool {
    let root = root.strip_suffix(['/', '\\']).unwrap_or(root);
    path.strip_prefix(root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '\\']))
}

/// A failed git command said in a sentence: here, `git_error_message`; on a mirror, home's own
/// first line, or that a login is asked for at home.
pub(crate) fn failure_words(err: &GitError, computer: Option<&str>) -> String {
    match computer {
        None => super::git_error_message(err),
        Some(computer) if err.needs_input() => {
            format!("git on {computer} asks for a login: run it in a terminal there.")
        }
        Some(_) => err.first_line().to_string(),
    }
}

impl Workspace {
    /// Where the job of the Git `at` names runs: here with its `Repo`, or at home with the
    /// arguments every call carries. None while there is no repository (or it changed).
    pub(super) fn git_runs(&mut self, at: &GitAt) -> Option<Runs> {
        let root = match &self.git_at(at)?.found {
            Found::Repo(repo) => return Some(Runs::Here(repo.clone())),
            Found::Home(root) => root.clone(),
            _ => return None,
        };
        self.home_args(&at.space, &root).map(Runs::Home)
    }

    /// The arguments every call of `git` carries on the mirrored workspace `space`: home's
    /// workspace and the folder (as home wrote it).
    pub(super) fn home_args(&self, space: &str, folder: &str) -> Option<Value> {
        let workspace = self
            .space_named(space)?
            .remote
            .as_deref()?
            .home_workspace()?;
        Some(json!({ "workspace": workspace, "folder": folder }))
    }

    /// A read of the popup: `here` on this computer's `Repo` as before, or `op` at home turned
    /// into the same result by `home`. `done` is the same either way.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn git_op<T: Send + 'static>(
        &mut self,
        at: GitAt,
        op: Op,
        here: impl FnOnce(&Repo) -> T + Send + 'static,
        home: fn(Result<Value, GitError>) -> T,
        done: impl FnOnce(&mut Workspace, GitAt, T, &mut Window, &mut Context<Workspace>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.git_runs(&at) {
            Some(Runs::Here(repo)) => self.git_job(at, move || here(&repo), done, window, cx),
            Some(Runs::Home(args)) => self.home_job(at, args, op, false, home, done, window, cx),
            None => {}
        }
    }

    /// `git_op` for an action: while any is asked and not done, the action buttons are disabled.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn act_op<T: Send + 'static>(
        &mut self,
        at: GitAt,
        op: Op,
        here: impl FnOnce(&Repo) -> T + Send + 'static,
        home: fn(Result<Value, GitError>) -> T,
        done: impl FnOnce(&mut Workspace, GitAt, T, &mut Window, &mut Context<Workspace>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.git_runs(&at) {
            Some(Runs::Here(repo)) => self.act_job(at, move || here(&repo), done, window, cx),
            Some(Runs::Home(args)) => self.home_job(at, args, op, true, home, done, window, cx),
            None => {}
        }
    }

    /// `op` at home for the Git `at` names, with the popup's queue and busy rules: the rule
    /// first (an act warns, a read sets the popup's error line; nothing is sent), then the call;
    /// `home` turns its answer into the job's result and `done` takes it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn home_job<T: Send + 'static>(
        &mut self,
        at: GitAt,
        args: Value,
        op: Op,
        act: bool,
        home: fn(Result<Value, GitError>) -> T,
        done: impl FnOnce(&mut Workspace, GitAt, T, &mut Window, &mut Context<Workspace>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_at(&at).is_none() {
            return;
        }
        if let Some(why) = self.why_not_in(&at.space, Deed::Git(op.name())) {
            if act {
                self.warn(why, window, cx);
            } else if let Some(git) = self.git_at(&at) {
                git.error = Some(why);
                cx.notify();
            }
            return;
        }
        if !act {
            return self.home_send(at, args, op, home, done, cx);
        }
        if let Some(git) = self.git_at(&at) {
            git.acting += 1;
        }
        let done = move |ws: &mut Workspace,
                         at: GitAt,
                         result: T,
                         window: &mut Window,
                         cx: &mut Context<Workspace>| {
            if let Some(git) = ws.git_at(&at) {
                git.acting = git.acting.saturating_sub(1);
            }
            done(ws, at, result, window, cx)
        };
        self.home_send(at, args, op, home, done, cx);
    }

    /// The call itself, queued behind the running job as `git_job` queues.
    fn home_send<T: Send + 'static>(
        &mut self,
        at: GitAt,
        args: Value,
        op: Op,
        home: fn(Result<Value, GitError>) -> T,
        done: impl FnOnce(&mut Workspace, GitAt, T, &mut Window, &mut Context<Workspace>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.git_at(&at) else {
            return;
        };
        if git.busy {
            git.queued.push(Box::new(move |ws, _, cx| {
                ws.home_send(at, args, op, home, done, cx)
            }));
            return;
        }
        git.busy = true;
        cx.notify();
        let (name, space, slow) = (op.name(), at.space.clone(), slow(&op));
        let mut args = args;
        if let (Some(args), Ok(Value::Object(op))) =
            (args.as_object_mut(), serde_json::to_value(&op))
        {
            args.extend(op);
        }
        let call = Call {
            tool: "git".into(),
            args,
            slow,
        };
        let then: Then = Box::new(move |_, answered, window, cx| {
            let heard = heard(name, answered);
            #[cfg(test)]
            let said = heard.as_ref().err().map(|e| e.first_line().to_string());
            // Up to 8 MiB of JSON become the job's result in the background, as a local job's
            // do: the frame never waits for it.
            cx.spawn_in(window, async move |ws, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { home(heard) })
                    .await;
                ws.update_in(cx, |ws, window, cx| {
                    #[cfg(test)]
                    ws.said.extend(said);
                    ws.job_done(at, result, done, window, cx)
                })
                .ok();
            })
            .detach();
        });
        self.remote_call(&space, call, then, cx);
    }

    /// The Git `at` names is open to the status the panel's diff was read with: on this computer
    /// always; on a mirror when the status changed, or [`HOME_DIFF_AGAIN`] passed.
    pub(super) fn panel_due(&self, at: &GitAt) -> bool {
        if at.home.is_none() {
            return true;
        }
        let Some(git) = self
            .space_named(&at.space)
            .and_then(|s| s.git())
            .filter(|g| g.generation == at.generation)
        else {
            return false;
        };
        git.home_diff
            .as_ref()
            .is_none_or(|(status, at)| *status != git.status || at.elapsed() >= HOME_DIFF_AGAIN)
    }

    /// What a question about the Git of the workspace shown remembers: which workspace it was
    /// asked on, and whether that one is a mirror.
    pub(super) fn asked_on(&self) -> (String, bool) {
        (self.space.id.clone(), self.on_remote())
    }

    /// A question asked on `asked` is answered: true when the same workspace is shown. When
    /// another is, and either is a mirror, nothing is done (and it is said): an answer must never
    /// reach home's repository, or a local one, by way of the workspace that happens to be shown.
    pub(super) fn answered_on(
        &mut self,
        asked: &(String, bool),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        (self.space.id == asked.0 || !(asked.1 || self.on_remote()))
            || self.still_shown(&asked.0, window, cx)
    }

    /// The Refresh of the header and the folder menu: the panel's diff is read too.
    pub(crate) fn refresh_git_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(git) = self.space.git_mut() {
            git.home_diff = None;
        }
        self.refresh_git(window, cx);
    }

    /// Where the Git popup opens on the mirrored workspace shown: the focused terminal's folder
    /// at home, else the first terminal's (back to front) with one, else the workspace's. Home's
    /// words, one line each.
    pub(crate) fn home_git_folder(&self) -> Option<String> {
        let remote = self.space.remote.as_deref()?;
        let home = remote.home.as_ref()?;
        let folder = |b: &templum_api::model::BlockView| {
            b.folder
                .as_deref()
                .map(|f| super::bus::clean_to(f, super::remote::LINE_CHARS))
                .filter(|f| !f.is_empty())
        };
        let mut terminals: Vec<_> = remote
            .state
            .blocks
            .iter()
            .filter(|b| b.workspace == home.id && b.kind == "terminal")
            .collect();
        terminals.sort_by_key(|b| b.z);
        self.space
            .focused
            .and_then(|id| remote.home_of(id))
            .and_then(|id| terminals.iter().find(|b| b.id == id))
            .and_then(|b| folder(b))
            .or_else(|| terminals.iter().find_map(|b| folder(b)))
            .or_else(|| {
                let own = home.folder.as_deref()?;
                Some(super::bus::clean_to(own, super::remote::LINE_CHARS)).filter(|f| !f.is_empty())
            })
    }

    /// Points the mirrored workspace's Git at `folder` at home, looked up afresh: nothing of the
    /// one before stays, and what its calls still waiting return is dropped. Nothing is
    /// remembered: a mirror's Git lives in memory only.
    pub(crate) fn set_home_git_folder(
        &mut self,
        folder: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A folder of home's is for a mirror's Git: another workspace shown by the time a menu
        // item is clicked keeps its own.
        if !self.on_remote() {
            return;
        }
        let (tab, generation) = self
            .space
            .git()
            .map_or((Tab::Changes, 0), |g| (g.tab, g.generation + 1));
        let mut git = Git::at_home(folder, tab, window, cx);
        git.generation = generation;
        if self.git_open {
            git.focus.focus(window, cx);
        }
        self.space.git = Some(Box::new(git));
        self.look_up_git(window, cx);
        cx.notify();
    }

    /// The folders the folder menu offers on the mirrored workspace shown: home's workspace's and
    /// its terminals' (`home_folders`), then home's recent folders of it, each once, less the one
    /// the Git is at and those inside its root. They are folders, not repositories: a `find` for
    /// each would run at home at every opening.
    pub(crate) fn home_git_folders(&self) -> Vec<String> {
        let git = self.space.git();
        let home = git.and_then(|g| g.home.as_deref());
        let root = git.and_then(|g| match &g.found {
            Found::Home(root) => Some(root.as_str()),
            _ => None,
        });
        let recent = (self.space.remote.as_deref()).map_or(&[][..], |r| &r.recent[..]);
        let mut folders: Vec<String> = Vec::new();
        for path in
            (self.home_folders().into_iter().map(|(path, _)| path)).chain(recent.iter().cloned())
        {
            let shown =
                Some(path.as_str()) == home || root.is_some_and(|root| lies_in(&path, root));
            if !shown && !folders.contains(&path) {
                folders.push(path);
            }
        }
        folders
    }

    /// Open a terminal here, from the folder menu of a mirror: home opens a terminal in `root`.
    pub(crate) fn open_terminal_at_home(
        &mut self,
        root: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = self.new_terminal_rect(window, cx).origin();
        self.remote_add_in(Kind::Terminal, at, Some(root), None, cx);
        self.close_git(window, cx);
    }

    /// The terminals of the mirrored workspace shown whose folder at home is `root` or inside it.
    pub(crate) fn home_terminals_in(&self, root: &str) -> Vec<(BlockId, String)> {
        let Some(remote) = self.space.remote.as_deref() else {
            return Vec::new();
        };
        let folder_of = |id: BlockId| {
            let home = remote.home_of(id)?;
            let block = remote.state.blocks.iter().find(|b| b.id == home)?;
            block.folder.clone()
        };
        self.remote_terminals()
            .into_iter()
            .filter(|(id, _)| folder_of(*id).is_some_and(|folder| lies_in(&folder, root)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::remote::LINE_CHARS;
    use super::*;
    use templum_api::client::Refusal;

    #[test]
    fn home_s_paths_lie_in_its_roots_by_either_separator() {
        assert!(lies_in("/a/b", "/a"));
        assert!(lies_in("/a", "/a"));
        assert!(lies_in("/a/b", "/a/"));
        assert!(lies_in("C:\\x\\y", "C:\\x"));
        assert!(!lies_in("/ab", "/a"));
        assert!(!lies_in("/a", "/a/b"));
    }

    #[test]
    fn only_the_network_generate_and_what_runs_hooks_are_slow() {
        let slow_ones = [
            Op::Fetch,
            Op::Push,
            Op::Pull {
                how: Default::default(),
            },
            Op::Sync { join: None },
            Op::Message { all: false },
            Op::PushBranch { branch: "x".into() },
            Op::Publish {
                remote: "origin".into(),
                branch: "x".into(),
            },
            // Hooks: pre-commit, commit-msg, post-checkout, post-merge.
            Op::Commit {
                summary: "x".into(),
                description: String::new(),
                amend: false,
                all: false,
            },
            Op::Switch {
                branch: "x".into(),
                remote: false,
                leave: None,
            },
            Op::NewBranch {
                name: "x".into(),
                hash: None,
            },
            Op::Checkout { hash: "a".into() },
            Op::Merge { branch: "x".into() },
            Op::Revert { hash: "a".into() },
            Op::StashApply {
                stash: ops::StashRef {
                    index: 0,
                    hash: "a".into(),
                },
            },
            Op::StashPop {
                stash: ops::StashRef {
                    index: 0,
                    hash: "a".into(),
                },
            },
            Op::Continue,
        ];
        assert!(slow_ones.iter().all(slow));
        assert!(!slow(&Op::Look));
        assert!(!slow(&Op::Stage { paths: vec![] }));
        assert!(!slow(&Op::Discard { paths: vec![] }));
    }

    #[test]
    fn home_s_refusal_is_a_git_error_with_its_details() {
        let refused = |details: Option<&str>| {
            heard(
                "push",
                Err(Refusal {
                    sentence: "fatal: x".into(),
                    details: details.map(Into::into),
                    maybe_done: false,
                }),
            )
        };
        let error = |stderr: &str| GitError {
            command: "git push".into(),
            code: None,
            stderr: stderr.into(),
            missing: false,
        };
        assert_eq!(
            refused(Some("hint\nfatal: x")),
            Err(error("hint\nfatal: x"))
        );
        assert_eq!(refused(None), Err(error("fatal: x")));
        assert_eq!(
            heard("look", Ok(json!({"git": {"a": 1}}))),
            Ok(json!({"a": 1}))
        );
        assert_eq!(heard("look", Ok(json!({}))), Ok(Value::Null));
    }

    #[test]
    fn a_failure_on_a_mirror_is_said_in_home_s_words() {
        let login = GitError {
            command: "git push".into(),
            code: Some(128),
            stderr: "fatal: could not read Username for 'https://x': terminal prompts disabled"
                .into(),
            missing: false,
        };
        assert_eq!(
            failure_words(&login, Some("winx-desktop")),
            "git on winx-desktop asks for a login: run it in a terminal there."
        );
        assert_eq!(
            failure_words(&login, None),
            super::super::git_error_message(&login)
        );
        let plain = GitError {
            stderr: "hint: x\nfatal: nope".into(),
            ..login
        };
        assert_eq!(failure_words(&plain, Some("winx-desktop")), "fatal: nope");
    }

    #[test]
    fn a_root_cleaning_would_change_is_refused_never_altered() {
        let root = |root: Option<&str>| found(Ok(json!({"root": root, "main_tree": null})));
        assert_eq!(
            root(Some("/home/ada/my app")).unwrap().root.unwrap(),
            "/home/ada/my app"
        );
        assert_eq!(root(None).unwrap().root, None);
        for odd in [
            "/a/b\u{1b}",
            "/a/b\nc",
            "/a//b  c",
            &"/x".repeat(LINE_CHARS),
        ] {
            let refused = root(Some(odd)).unwrap_err();
            assert!(refused.stderr.contains("can't show"), "{odd:?}");
        }
    }

    #[test]
    fn a_shape_error_is_told_as_a_git_error() {
        let wrong = parsed::<ops::Look>(Ok(json!({"status": 7})));
        let stderr = wrong.unwrap_err().stderr;
        assert!(
            stderr.starts_with("The answer couldn't be read"),
            "{stderr}"
        );
    }
}
