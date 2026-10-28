//! One repository and the `git` program that runs on it. Every method runs one command to
//! completion on the calling thread (the UI runs them on its background executor), never waits
//! for input, and gives back typed values or a [`GitError`] with the command and its stderr.

use std::collections::HashSet;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::blame::BlameLine;
use crate::diff::{self, FileDiff, parse_diff};
use crate::parse::{self, Branch, Change, Commit, CommitFile, Entry, Operation, Stash, Status};

/// A command that failed: which one, how, and what git said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitError {
    /// "git push origin main", as a shell would take it.
    pub command: String,
    /// The exit code; None when it couldn't run or was killed by a signal.
    pub code: Option<i32>,
    pub stderr: String,
    /// The program isn't installed (or isn't on PATH).
    pub missing: bool,
}

impl GitError {
    /// The line that says why: what would have asked for a key or a password, a rejected ref,
    /// else git's fatal or error line (its hints come first), else the first line.
    pub fn first_line(&self) -> &str {
        let lines = || self.stderr.lines().map(str::trim).filter(|l| !l.is_empty());
        lines()
            .find(|l| parse::needs_input(l))
            .or_else(|| lines().find(|l| l.starts_with("! [")))
            .or_else(|| lines().find(|l| l.starts_with("fatal:") || l.starts_with("error:")))
            .or_else(|| lines().next())
            .unwrap_or("")
    }

    /// It was cancelled ([`Repo::with_cancel`]), not refused.
    pub fn cancelled(&self) -> bool {
        self.code.is_none() && !self.missing && self.stderr == CANCELLED
    }

    /// Its output passed [`API_OUTPUT_MOST`] and it was killed.
    pub fn too_large(&self) -> bool {
        self.code.is_none() && !self.missing && self.stderr == TOO_LARGE
    }

    /// It failed because git would have asked for a password or a key.
    pub fn needs_input(&self) -> bool {
        parse::needs_input(&self.stderr)
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.command, self.first_line())
    }
}

impl std::error::Error for GitError {}

/// A diff over this isn't shown: the view would stall the window (the editor opens files up to
/// 4 MB editable, too).
pub const DIFF_LIMIT: usize = 4 * 1024 * 1024;

/// A worktree `add_worktree` made, against the working tree it goes back to (`worktree_state`).
#[derive(Clone, Debug, PartialEq)]
pub struct WorktreeState {
    /// Its folder.
    pub path: PathBuf,
    /// The branch checked out in it: its own (`name`) as `add_worktree` made it, another one an
    /// agent checked out there, or None on a detached HEAD.
    pub branch: Option<String>,
    /// The branch of the working tree it goes back to.
    pub into: String,
    /// Its files with changes, untracked ones included.
    pub changed: usize,
    /// Its HEAD's commits `into` doesn't have.
    pub unmerged: u32,
    /// The files with changes not committed where it goes back, tracked ones only.
    pub into_changed: usize,
    /// A merge, rebase or cherry-pick under way where it goes back.
    pub busy: bool,
    /// The files its HEAD has that a merge would write where it goes back over files git ignores
    /// there, which git does without asking (a `.env` its branch tracks).
    pub overwrites: Vec<String>,
    /// Its files git ignores, which removing it deletes (a `.env`), but for what a build or a test
    /// run makes again (`node_modules`, `target`, `.pytest_cache`); a folder an ignore pattern
    /// names as `dir/`.
    pub ignored: Vec<String>,
}

/// The folders a build, a package manager or a test run fills again: a worktree's ignored files in
/// them aren't work [`WorktreeState::ignored`] tells of; nor a `*.egg-info` folder, nor the files
/// of [`BUILT_FILES`].
const BUILT: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "dist",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".angular",
    ".turbo",
    ".parcel-cache",
    ".venv",
    "venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".nox",
    ".hypothesis",
    "coverage",
    "htmlcov",
    ".nyc_output",
    ".gradle",
    ".dart_tool",
    ".cache",
];

/// The logs and caches a package manager or a tool leaves.
const BUILT_FILES: &[&str] = &[
    "npm-debug.log",
    "yarn-error.log",
    "yarn-debug.log",
    "pnpm-debug.log",
    ".eslintcache",
    ".coverage",
];

#[derive(Clone, Debug)]
pub struct Repo {
    program: String,
    /// Set from elsewhere: the running command is killed.
    cancel: Option<Arc<AtomicBool>>,
    /// Past it the running command is killed ([`Repo::with_deadline`]).
    deadline: Option<Instant>,
    /// The working tree's top folder.
    pub root: PathBuf,
    /// Where HEAD, MERGE_HEAD and the like live (a worktree's own, for a linked worktree).
    pub git_dir: PathBuf,
}

/// "git commit -m 'it'\''s done'": the command as a POSIX shell would take it, argument for
/// argument (what "Run in a terminal" types). bash, zsh and fish read it alike.
fn shown(program: &str, args: &[&str]) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    std::iter::once(program)
        .chain(args.iter().copied())
        .map(|a| {
            if !a.is_empty() && a.chars().all(safe) {
                a.to_string()
            } else {
                format!("'{}'", a.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs `program args` in `dir`: no terminal, no prompt, no editor, English messages.
fn run_in(
    program: &str,
    dir: &Path,
    args: &[&str],
    cancel: Option<&AtomicBool>,
    deadline: Option<Instant>,
) -> Result<Vec<u8>, GitError> {
    let command = shown(program, args);
    let mut c = Command::new(program);
    c.args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("LC_ALL", "C")
        // Reads never take index.lock: a `git add` in a terminal at that moment would fail.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // No console window flashing behind the app.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let couldnt = |e: io::Error| GitError {
        missing: e.kind() == io::ErrorKind::NotFound,
        command: command.clone(),
        code: None,
        stderr: format!("Couldn't run {program}: {e}"),
    };
    let out = if cancel.is_none() && deadline.is_none() {
        retry_busy(|| c.output()).map_err(couldnt)?
    } else {
        // A call with a deadline is the API's: nobody is there to answer, and what git starts
        // (ssh, a hook, a credential helper) dies with it.
        let unattended = deadline.is_some();
        if unattended {
            quiet(&mut c);
        }
        // An API caller can ask for a diff of any size: past a cap the command is killed.
        let most = unattended.then_some(API_OUTPUT_MOST);
        match until_cancelled(&mut c, cancel.unwrap_or(&NEVER), deadline, unattended, most)
            .map_err(|e| {
                if e.kind() == io::ErrorKind::FileTooLarge {
                    GitError {
                        command: command.clone(),
                        code: None,
                        stderr: TOO_LARGE.to_string(),
                        missing: false,
                    }
                } else {
                    couldnt(e)
                }
            })? {
            Some(out) => out,
            None => {
                return Err(GitError {
                    command,
                    code: None,
                    stderr: CANCELLED.to_string(),
                    missing: false,
                });
            }
        }
    };
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(GitError {
            command,
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            missing: false,
        })
    }
}

/// `c` never waits on a person: ssh asks nothing (its askpass fails at once, as `BatchMode`
/// would, without replacing the user's `GIT_SSH_COMMAND` or `core.sshCommand`), nor does a
/// credential manager. A setting the user made for these stays.
fn quiet(c: &mut Command) {
    // Windows has no console here to ask on (CREATE_NO_WINDOW).
    if cfg!(unix) && std::env::var_os("SSH_ASKPASS_REQUIRE").is_none() {
        c.env("SSH_ASKPASS_REQUIRE", "force")
            .env("SSH_ASKPASS", "false");
    }
    if std::env::var_os("GCM_INTERACTIVE").is_none() {
        c.env("GCM_INTERACTIVE", "never");
    }
}

/// Kills `child` and, with `group`, everything it started.
fn kill_tree(child: &mut Child, group: bool) {
    if group {
        let pid = child.id().to_string();
        #[cfg(unix)]
        let mut kill = Command::new("kill");
        #[cfg(unix)]
        kill.args(["-KILL", "--", &format!("-{pid}")]);
        #[cfg(windows)]
        let mut kill = {
            use std::os::windows::process::CommandExt as _;
            let mut kill = Command::new("taskkill");
            kill.args(["/T", "/F", "/PID", &pid])
                .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            kill
        };
        let _ = kill
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// What a cancelled command's error says.
const CANCELLED: &str = "Cancelled";

/// What a command of the API may write to stdout or stderr: 64 MiB, 8 times what an answer
/// weighs. It is read whole into memory, then parsed and copied (about 4 times its size); past
/// it the command is killed.
pub const API_OUTPUT_MOST: usize = 64 * 1024 * 1024;

/// What the error of a command killed for passing [`API_OUTPUT_MOST`] says.
const TOO_LARGE: &str = "Too large";

/// The cancel flag of a command that only has a deadline.
static NEVER: AtomicBool = AtomicBool::new(false);

/// Why a command was stopped: it wrote too much, or it was cancelled or ran past its deadline.
fn stopped(over: &AtomicBool) -> io::Result<Option<Output>> {
    if over.load(Ordering::Relaxed) {
        Err(io::Error::new(io::ErrorKind::FileTooLarge, TOO_LARGE))
    } else {
        Ok(None)
    }
}

/// Runs `run` again a few times while the program is busy: Linux refuses to run a file that
/// another thread's fork still holds open for writing, as when a wrapper was written a moment ago.
fn retry_busy<T>(mut run: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut tries = 0;
    loop {
        match run() {
            Err(e) if e.kind() == io::ErrorKind::ExecutableFileBusy && tries < 5 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20 * tries));
            }
            done => return done,
        }
    }
}

/// Runs `c` to the end, or kills it once `cancel` is set or `deadline` has passed (None then).
/// Its output is read on threads meanwhile, so a chatty command never blocks on a full pipe.
/// With `group` it runs in a group of its own and the kill takes all of it (ssh, a hook).
/// With `most`, a command writing more than that to stdout or stderr is killed and the error is
/// [`io::ErrorKind::FileTooLarge`].
pub(crate) fn until_cancelled(
    c: &mut Command,
    cancel: &AtomicBool,
    deadline: Option<Instant>,
    group: bool,
    most: Option<usize>,
) -> io::Result<Option<Output>> {
    use std::io::Read;
    #[cfg(unix)]
    if group {
        use std::os::unix::process::CommandExt as _;
        c.process_group(0);
    }
    let mut child = retry_busy(|| c.spawn())?;
    let over = Arc::new(AtomicBool::new(false));
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        let over = over.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(pipe) = pipe {
                // One byte past the most says it was passed.
                let room = most.map_or(u64::MAX, |m| m as u64 + 1);
                let _ = pipe.take(room).read_to_end(&mut bytes);
                if most.is_some_and(|m| bytes.len() > m) {
                    over.store(true, Ordering::Relaxed);
                }
            }
            bytes
        })
    };
    let stdout = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stop = || {
        cancel.load(Ordering::Relaxed)
            || over.load(Ordering::Relaxed)
            || deadline.is_some_and(|d| Instant::now() >= d)
    };
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if stop() {
            kill_tree(&mut child, group);
            return stopped(&over);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // A helper it left running may hold its output open: waited for no longer than it.
    while !(stdout.is_finished() && stderr.is_finished()) {
        if stop() {
            kill_tree(&mut child, group);
            return stopped(&over);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if over.load(Ordering::Relaxed) {
        return stopped(&over);
    }
    Ok(Some(Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    }))
}

impl Repo {
    /// "git version 2.55.0", or why `program` can't run.
    pub fn version(program: &str) -> Result<String, GitError> {
        let out = run_in(program, Path::new("."), &["--version"], None, None)?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }

    /// The repository holding `folder`: Ok(None) when there is none.
    pub fn discover(program: &str, folder: &Path) -> Result<Option<Repo>, GitError> {
        let out = match run_in(
            program,
            folder,
            &["rev-parse", "--show-toplevel", "--absolute-git-dir"],
            None,
            None,
        ) {
            Ok(out) => out,
            Err(e) if e.stderr.contains("not a git repository") => return Ok(None),
            Err(e) => return Err(e),
        };
        let text = String::from_utf8_lossy(&out);
        let mut lines = text.lines();
        let (Some(root), Some(git_dir)) = (lines.next(), lines.next()) else {
            return Ok(None);
        };
        Ok(Some(Repo {
            program: program.to_string(),
            cancel: None,
            deadline: None,
            root: PathBuf::from(root),
            git_dir: PathBuf::from(git_dir),
        }))
    }

    /// `git init` in `folder`.
    pub fn init(program: &str, folder: &Path) -> Result<Repo, GitError> {
        run_in(program, folder, &["init", "-q"], None, None)?;
        Self::discover(program, folder)?.ok_or_else(|| GitError {
            command: shown(program, &["init", "-q"]),
            code: None,
            stderr: "git init made no repository".to_string(),
            missing: false,
        })
    }

    /// `git clone -- url into`, then the repository there. `into`'s parent is made when
    /// missing. A clone that fails or is cancelled leaves no folder behind when there was none
    /// before, and an empty folder empty (git can't clean up after a kill).
    pub fn clone_into(
        program: &str,
        url: &str,
        into: &Path,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<Repo, GitError> {
        let dest = into.to_string_lossy().into_owned();
        let command = shown(program, &["clone", "--", url, &dest]);
        let parent = into.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| GitError {
            command: command.clone(),
            code: None,
            stderr: format!("Couldn't make {}: {e}", parent.display()),
            missing: false,
        })?;
        let existed = into.exists();
        let was_empty = std::fs::read_dir(into).is_ok_and(|mut d| d.next().is_none());
        if let Err(e) = run_in(
            program,
            parent,
            &["clone", "-q", "--", url, &dest],
            cancel.as_deref(),
            None,
        ) {
            if !existed {
                let _ = std::fs::remove_dir_all(into);
            } else if was_empty {
                // An empty folder chosen as it is: empty again, with no half-made `.git`.
                for entry in std::fs::read_dir(into).into_iter().flatten().flatten() {
                    let path = entry.path();
                    let _ = if path.is_dir() && !path.is_symlink() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                }
            }
            return Err(e);
        }
        Self::discover(program, into)?.ok_or(GitError {
            command,
            code: None,
            stderr: "git clone made no repository".to_string(),
            missing: false,
        })
    }

    /// This repository, its commands killed once `cancel` is set (fetch, pull and push can
    /// hang on a network or on ssh asking on a terminal).
    pub fn with_cancel(&self, cancel: Arc<AtomicBool>) -> Repo {
        Repo {
            cancel: Some(cancel),
            ..self.clone()
        }
    }

    /// This repository, its commands killed past `at` (what the API's `git` tool allows one
    /// call). A killed command is a cancelled one: [`GitError::cancelled`].
    pub fn with_deadline(&self, at: Instant) -> Repo {
        Repo {
            deadline: Some(at),
            ..self.clone()
        }
    }

    /// Runs `git args` in the root; Ok is stdout.
    pub fn run(&self, args: &[&str]) -> Result<Vec<u8>, GitError> {
        run_in(
            &self.program,
            &self.root,
            args,
            self.cancel.as_deref(),
            self.deadline,
        )
    }

    /// `run` for a command with paths a caller wrote: they are files, never patterns (`[ab].txt`
    /// names that file, not a.txt and b.txt). Not for every command: `git stash -u` and others
    /// pass pathspec magic of their own.
    fn run_paths(&self, args: &[&str]) -> Result<Vec<u8>, GitError> {
        let mut all = vec!["--literal-pathspecs"];
        all.extend_from_slice(args);
        self.run(&all)
    }

    fn text(&self, args: &[&str]) -> Result<String, GitError> {
        Ok(String::from_utf8_lossy(&self.run(args)?).into_owned())
    }

    pub fn status(&self) -> Result<Status, GitError> {
        let out = self.run(&["status", "--porcelain=v2", "--branch", "-z", "-uall"])?;
        Ok(parse::parse_status(&out))
    }

    /// A merge, rebase, cherry-pick or revert in progress, read from the git dir's markers.
    pub fn operation(&self) -> Option<Operation> {
        let has = |name: &str| self.git_dir.join(name).exists();
        if has("MERGE_HEAD") {
            Some(Operation::Merge)
        } else if has("rebase-merge") || has("rebase-apply") {
            Some(Operation::Rebase)
        } else if has("CHERRY_PICK_HEAD") {
            Some(Operation::CherryPick)
        } else if has("REVERT_HEAD") {
            Some(Operation::Revert)
        } else {
            None
        }
    }

    /// `user.name` and `user.email`, from any config level; None when either is unset.
    pub fn author(&self) -> Result<Option<(String, String)>, GitError> {
        let get = |key: &str| -> Result<Option<String>, GitError> {
            match self.text(&["config", "--get", key]) {
                Ok(v) if !v.trim().is_empty() => Ok(Some(v.trim().to_string())),
                Ok(_) => Ok(None),
                // Exit 1: the key isn't set.
                Err(e) if e.code == Some(1) => Ok(None),
                Err(e) => Err(e),
            }
        };
        Ok(match (get("user.name")?, get("user.email")?) {
            (Some(name), Some(email)) => Some((name, email)),
            _ => None,
        })
    }

    /// Writes both to the global config, or only to this repository's.
    pub fn set_author(&self, name: &str, email: &str, global: bool) -> Result<(), GitError> {
        let scope = if global { "--global" } else { "--local" };
        self.run(&["config", scope, "user.name", name])?;
        self.run(&["config", scope, "user.email", email])?;
        Ok(())
    }

    /// `git add -A -- paths`: new, changed and deleted files alike.
    pub fn stage(&self, paths: &[&str]) -> Result<(), GitError> {
        let mut args = vec!["add", "-A", "--"];
        args.extend_from_slice(paths);
        self.run_paths(&args).map(drop)
    }

    /// `git reset -q -- paths`: works before the first commit, where `restore --staged` doesn't.
    pub fn unstage(&self, paths: &[&str]) -> Result<(), GitError> {
        let mut args = vec!["reset", "-q", "--"];
        args.extend_from_slice(paths);
        self.run_paths(&args).map(drop)
    }

    /// Throws away unstaged changes: tracked files go back to the index, untracked ones are
    /// deleted.
    pub fn discard(&self, entries: &[&Entry]) -> Result<(), GitError> {
        let tracked: Vec<&str> = entries
            .iter()
            .filter(|e| e.unstaged != Some(Change::Untracked))
            .map(|e| e.path.as_str())
            .collect();
        if !tracked.is_empty() {
            let mut args = vec!["restore", "--worktree", "--"];
            args.extend_from_slice(&tracked);
            self.run_paths(&args)?;
        }
        for e in entries
            .iter()
            .filter(|e| e.unstaged == Some(Change::Untracked))
        {
            // -uall lists files, never folders.
            std::fs::remove_file(self.root.join(&e.path)).map_err(|err| GitError {
                command: shown("rm", &["--", &e.path]),
                code: None,
                stderr: err.to_string(),
                missing: false,
            })?;
        }
        Ok(())
    }

    /// Appends `path` to the root's .gitignore (made if missing) as a pattern for that path
    /// alone: anchored at the root, its wildcards escaped. The file's other bytes stay as they
    /// are; one that can't be read is left alone.
    pub fn ignore(&self, path: &str) -> io::Result<()> {
        let file = self.root.join(".gitignore");
        let mut bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            bytes.push(b'\n');
        }
        let mut line = String::from("/");
        for c in path.chars() {
            if "\\*?[".contains(c) {
                line.push('\\');
            }
            line.push(c);
        }
        // A trailing space is dropped unless escaped.
        if line.ends_with(' ') {
            line.insert(line.len() - 1, '\\');
        }
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        std::fs::write(file, bytes)
    }

    /// A worktree for a new branch `name`, from HEAD, at `.worktrees/<name>` in the repository's
    /// main working tree (also when this is a linked worktree). `.worktrees/` goes once into the
    /// repository's `info/exclude`, so `git status` never lists it. Its folder.
    pub fn add_worktree(&self, name: &str) -> Result<PathBuf, GitError> {
        let refused = |why: String| GitError {
            command: shown(&self.program, &["worktree", "add", "-b", name]),
            code: None,
            stderr: format!("fatal: {why}"),
            missing: false,
        };
        if !worktree_name_ok(name) {
            return Err(refused(format!(
                "'{name}' can't name a worktree: use letters, digits, '-', '_', '.' and '/'"
            )));
        }
        let Some(main) = self.main_tree()? else {
            return Err(refused(
                "a bare repository has no working tree to branch from".into(),
            ));
        };
        let common = self.text(&["rev-parse", "--git-common-dir"])?;
        let exclude = self.root.join(common.trim()).join("info").join("exclude");
        add_line(&exclude, ".worktrees/")
            .map_err(|e| refused(format!("couldn't write {}: {e}", exclude.display())))?;
        let path = main.join(".worktrees").join(name);
        let dest = path.to_string_lossy().into_owned();
        self.run(&["worktree", "add", "-q", "-b", name, &dest])?;
        Ok(path)
    }

    /// The repository's main working tree: the first of `git worktree list`; None for a bare
    /// repository, which has none.
    pub fn main_tree(&self) -> Result<Option<PathBuf>, GitError> {
        let list = self.text(&["worktree", "list", "--porcelain"])?;
        let mut first = list.split("\n\n").next().unwrap_or_default().lines();
        let main = first
            .next()
            .and_then(|l| l.strip_prefix("worktree "))
            .map(PathBuf::from);
        Ok(main.filter(|_| !first.any(|l| l == "bare")))
    }

    /// The worktree `add_worktree(name)` made (`.worktrees/<name>` of the main working tree), as
    /// it stands against this working tree, where it goes back: None when there is no such
    /// worktree, or `name` couldn't name one.
    pub fn worktree_state(&self, name: &str) -> Result<Option<WorktreeState>, GitError> {
        if !worktree_name_ok(name) {
            return Ok(None);
        }
        let Some(main) = self.main_tree()? else {
            return Ok(None);
        };
        let path = main.join(".worktrees").join(name);
        let same = |p: &Path| match (p.canonicalize(), path.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => p == path,
        };
        let list = self.text(&["worktree", "list", "--porcelain"])?;
        let Some(entry) = list.split("\n\n").find(|entry| {
            entry
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("worktree "))
                .is_some_and(|p| same(Path::new(p)))
        }) else {
            return Ok(None);
        };
        let branch = entry
            .lines()
            .find_map(|l| l.strip_prefix("branch refs/heads/"))
            .map(str::to_string);
        let Some(head) = entry.lines().find_map(|l| l.strip_prefix("HEAD ")) else {
            return Ok(None);
        };
        let Some(tree) = Repo::discover(&self.program, &path)? else {
            return Ok(None);
        };
        let lines = |text: String| text.lines().filter(|l| !l.is_empty()).count();
        let changed = lines(tree.text(&["status", "--porcelain", "-uall"])?);
        let into = self
            .text(&["rev-parse", "--abbrev-ref", "HEAD"])?
            .trim()
            .to_string();
        let unmerged = self
            .text(&["rev-list", "--count", &format!("HEAD..{head}")])?
            .trim()
            .parse()
            .unwrap_or(0);
        let into_changed = lines(self.text(&["diff", "--name-only", "HEAD"])?);
        let theirs = self.ignored()?;
        let overwrites = self
            .text(&["diff", "--name-only", "--diff-filter=A", "HEAD", head])?
            .lines()
            .filter(|file| {
                theirs
                    .iter()
                    .any(|i| file == i || (i.ends_with('/') && file.starts_with(i.as_str())))
            })
            .map(str::to_string)
            .collect();
        // What a build makes again goes without a word: a folder of `BUILT`, and what is inside one
        // when it ignores itself with a `*` (pytest's, ruff's, a virtualenv's), git then listing its
        // `.gitignore` and the rest instead of the folder. Not what is in a tracked `build/ci/`.
        let all = tree.ignored()?;
        let built_dir = |dir: &str| {
            let name = dir.rsplit('/').next().unwrap_or(dir);
            BUILT.contains(&name) || name.ends_with(".egg-info")
        };
        let built = |entry: &&String| {
            let whole = match entry.strip_suffix('/') {
                Some(dir) => built_dir(dir),
                None => BUILT_FILES.contains(&entry.rsplit('/').next().unwrap_or(entry)),
            };
            whole
                || entry.match_indices('/').any(|(at, _)| {
                    let dir = &entry[..at];
                    built_dir(dir)
                        && all
                            .iter()
                            .any(|i| i.strip_prefix(dir) == Some("/.gitignore"))
                })
        };
        let ignored = all.iter().filter(|e| !built(e)).cloned().collect();
        Ok(Some(WorktreeState {
            path,
            branch,
            into,
            changed,
            unmerged,
            into_changed,
            busy: self.operation().is_some(),
            overwrites,
            ignored,
        }))
    }

    /// The files git ignores in its working tree, a folder an ignore pattern names as `dir/`.
    fn ignored(&self) -> Result<Vec<String>, GitError> {
        let listed = self.text(&["status", "--porcelain", "--ignored=matching"])?;
        Ok(listed
            .lines()
            .filter_map(|l| l.strip_prefix("!! "))
            .map(str::to_string)
            .collect())
    }

    /// Removes the worktree `add_worktree(name)` made, then its branch: with `force`, its changes
    /// and its commits not merged are lost; without, git refuses to lose them. Refused while it
    /// isn't on its own branch: a detached HEAD's commits, or another branch, would go with it.
    pub fn remove_worktree(&self, name: &str, force: bool) -> Result<(), GitError> {
        let refused = |why: String| GitError {
            command: shown(&self.program, &["worktree", "remove", name]),
            code: None,
            stderr: format!("fatal: {why}"),
            missing: false,
        };
        let state = self
            .worktree_state(name)?
            .ok_or_else(|| refused(format!("there's no worktree {name}")))?;
        if state.branch.as_deref() != Some(name) {
            return Err(refused(format!(
                "the worktree {name} isn't on its branch {name}"
            )));
        }
        let path = state.path.to_string_lossy().into_owned();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&path);
        self.run(&args)?;
        self.delete_branch(name, force)
    }

    /// Merges the branch `branch` (never a tag of that name) into this working tree's branch:
    /// Ok(Err(files)) when it would conflict in `files`, the merge then aborted, so nothing is left
    /// half done.
    pub fn merge_from(&self, branch: &str) -> Result<Result<(), Vec<String>>, GitError> {
        // A tag of that name would be taken for it: then its full name, with git's words for it.
        let tag = format!("refs/tags/{branch}");
        let (full, words) = (
            format!("refs/heads/{branch}"),
            format!("Merge branch '{branch}'"),
        );
        let args = match self.run(&["show-ref", "--verify", "--quiet", &tag]) {
            Ok(_) => vec!["merge", "--no-edit", "-m", &words, &full],
            Err(_) => vec!["merge", "--no-edit", branch],
        };
        let Err(failed) = self.run(&args) else {
            return Ok(Ok(()));
        };
        let conflicted = self.text(&["diff", "--name-only", "--diff-filter=U"])?;
        let files: Vec<String> = conflicted.lines().map(str::to_string).collect();
        if self.operation() == Some(Operation::Merge) {
            self.run(&["merge", "--abort"])?;
        }
        if files.is_empty() {
            return Err(failed);
        }
        Ok(Err(files))
    }

    /// `git commit -m message` (`--amend` with `amend`): the new commit's short hash.
    pub fn commit(&self, message: &str, amend: bool) -> Result<String, GitError> {
        let mut args = vec!["commit", "-q", "-m", message];
        if amend {
            args.push("--amend");
        }
        self.run(&args)?;
        Ok(self
            .text(&["rev-parse", "--short", "HEAD"])?
            .trim()
            .to_string())
    }

    /// HEAD's full message, for Amend.
    pub fn last_message(&self) -> Result<String, GitError> {
        Ok(self
            .text(&["log", "-1", "--format=%B"])?
            .trim_end()
            .to_string())
    }

    /// `git add --all`: every change, new and deleted files included (Commit all changes).
    pub fn stage_all(&self) -> Result<(), GitError> {
        self.run(&["add", "--all"]).map(drop)
    }

    /// The commit `short` is on the current branch's upstream: pushed, so it can't be taken
    /// back for others. False with no upstream, one gone from the server (its branch deleted
    /// and pruned), and on a detached HEAD.
    pub fn on_upstream(&self, short: &str) -> Result<bool, GitError> {
        if self.run(&["rev-parse", "--verify", "-q", "@{u}"]).is_err() {
            return Ok(false);
        }
        match self.run(&["merge-base", "--is-ancestor", short, "@{u}"]) {
            Ok(_) => Ok(true),
            Err(e) if e.code == Some(1) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// HEAD's full hash (an error before the first commit).
    pub fn head(&self) -> Result<String, GitError> {
        Ok(self
            .text(&["rev-parse", "--verify", "HEAD"])?
            .trim()
            .to_string())
    }

    /// Takes the last commit back, its changes left staged, and gives its message (for the
    /// fields); None, with nothing done, when HEAD isn't `short` any more: a commit made since is
    /// never the one undone. HEAD goes back to `replaced` after an Amend (the commit it replaced,
    /// read before the amend), else to the parent; the repository's first commit has none, so
    /// the branch goes. `update-ref` moves it only from the commit checked here (a commit made
    /// in between is never lost) and leaves the index, as a soft reset does.
    pub fn undo_commit(
        &self,
        short: &str,
        replaced: Option<&str>,
    ) -> Result<Option<String>, GitError> {
        let Ok(head) = self.head() else {
            return Ok(None);
        };
        if short.is_empty() || !head.starts_with(short) {
            return Ok(None);
        }
        let message = self
            .text(&["log", "-1", "--format=%B", &head])?
            .trim_end()
            .to_string();
        let target = match replaced {
            Some(r) => Some(self.amended(&head, r)?),
            None => self
                .text(&["rev-parse", "--verify", "-q", &format!("{head}~1")])
                .ok()
                .map(|t| t.trim().to_string()),
        };
        match target {
            Some(t) => self.run(&[
                "update-ref",
                "-m",
                "templum: undo commit",
                "HEAD",
                &t,
                &head,
            ])?,
            None => self.run(&["update-ref", "-d", "HEAD", &head])?,
        };
        Ok(Some(message))
    }

    /// The commit that the amend which made `head` replaced, as HEAD's reflog tells it: the
    /// entry before the newest "commit (amend)" that moved HEAD to `head`.
    fn amended_from(&self, head: &str) -> Option<String> {
        let log = self
            .text(&["log", "-g", "-n1000", "--format=%H %gs", "HEAD"])
            .ok()?;
        let mut entries = log.lines();
        entries.find(|l| {
            l.strip_prefix(head)
                .is_some_and(|r| r.starts_with(" commit (amend)"))
        })?;
        let before = entries.next()?;
        Some(before.split(' ').next()?.to_string())
    }

    /// The full hash of `replaced` when HEAD is its amend: another commit with the same parents,
    /// as `git commit --amend` leaves it. Any other commit would move the branch somewhere else.
    fn amended(&self, head: &str, replaced: &str) -> Result<String, GitError> {
        let line = |rev: &str| -> Result<Vec<String>, GitError> {
            let out = self.text(&["rev-list", "--parents", "-n1", &format!("{rev}^{{commit}}")])?;
            Ok(out.split_whitespace().map(str::to_string).collect())
        };
        let (old, new) = (line(replaced)?, line(head)?);
        match (old.split_first(), new.split_first()) {
            // Two root commits "have the same parents" without being an amend of each other:
            // HEAD's reflog says which commit the amend of `head` replaced.
            (Some((hash, a)), Some((_, b)))
                if hash != head
                    && a == b
                    && (!b.is_empty() || self.amended_from(head).as_deref() == Some(hash)) =>
            {
                Ok(hash.clone())
            }
            _ => Err(GitError {
                command: shown(&self.program, &["rev-list", "--parents", replaced]),
                code: None,
                stderr: format!("fatal: {replaced} isn't the commit that {head} amended."),
                missing: false,
            }),
        }
    }

    /// `git apply` of `patch` (a `hunk_patch`): on the index with `cached`, the other way with
    /// `reverse`. Written to a temporary file, since `run` gives git no stdin. A patch that no
    /// longer applies (the file changed since it was read) is git's error.
    pub fn apply_patch(&self, patch: &str, cached: bool, reverse: bool) -> Result<(), GitError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let file = std::env::temp_dir().join(format!(
            "templum-{}-{}.patch",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&file, patch).map_err(|e| GitError {
            command: shown(&self.program, &["apply"]),
            code: None,
            stderr: format!("Couldn't write the patch: {e}"),
            missing: false,
        })?;
        let path = file.to_string_lossy().into_owned();
        // nowarn: a user's apply.whitespace=error would refuse a hunk with a trailing space.
        let mut args = vec!["apply", "--whitespace=nowarn"];
        if cached {
            args.push("--cached");
        }
        if reverse {
            args.push("--reverse");
        }
        args.push(&path);
        let result = self.run(&args).map(drop);
        let _ = std::fs::remove_file(&file);
        result
    }
}
/// The working tree's diff: the unstaged changes, untracked files included (built with a new
/// file's headers, after the tracked ones, in the status's order), and the staged ones.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingDiff {
    pub unstaged: Vec<FileDiff>,
    pub staged: Vec<FileDiff>,
}

/// What one read of the working diff takes of the untracked files, at most: past either, a
/// file is listed unread (`FileDiff::unread`) until it is asked for in `whole`.
const UNTRACKED_FILES: usize = 200;
const UNTRACKED_BYTES: u64 = DIFF_LIMIT as u64;
/// What the files asked for in `whole` take of one read in all, tracked (what git's re-read
/// writes) or untracked, 4 MiB at most each: the API can name a thousand.
const ASKED_BYTES: u64 = 8 * DIFF_LIMIT as u64;

/// What one read of the working diff takes of the conflicted files in all, regions kept as
/// lines (two or three times a file's size): past it, a file is listed unread.
const CONFLICT_BYTES: u64 = 4 * DIFF_LIMIT as u64;

/// More context lines than any file has: git shows the file whole.
const WHOLE: &str = "-U999999999";

/// A working file's size as git counts it: a link's own, not its target's.
fn working_size(path: &Path) -> u64 {
    std::fs::symlink_metadata(path).map_or(0, |m| m.len())
}

/// A working file's bytes as git diffs them: a regular file's, a link's own target text. None
/// for anything else (a device, a pipe: reading one never ends) or when it can't be read.
pub(crate) fn working_bytes(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.is_file() {
        std::fs::read(path).ok()
    } else if meta.is_symlink() {
        let target = std::fs::read_link(path).ok()?;
        Some(target.to_string_lossy().into_owned().into_bytes())
    } else {
        None
    }
}

impl Repo {
    /// `git diff` and `git diff --cached`, parsed (`-M`; `-w` with `ignore_whitespace`), each
    /// file's working size and line count counted. The files in `whole` (`(staged, path)`) are
    /// read again with every line as context, for the panel to show them whole.
    pub fn working_diff(
        &self,
        status: &Status,
        ignore_whitespace: bool,
        whole: &[(bool, String)],
    ) -> Result<WorkingDiff, GitError> {
        // Conflicted files first, region by region (git's own diff of them is a combined one,
        // which the parser skips).
        let mut conflict_bytes = 0u64;
        let mut unstaged: Vec<FileDiff> = status
            .conflicts()
            .iter()
            .filter_map(|e| {
                let full = self.root.join(&e.path);
                // Markers are in a regular file only (a link holds none).
                let meta = std::fs::symlink_metadata(&full).ok()?;
                if !meta.is_file() || meta.len() > DIFF_LIMIT as u64 {
                    return None;
                }
                if conflict_bytes + meta.len() > CONFLICT_BYTES {
                    let mut file = FileDiff::empty();
                    file.old_path = Some(e.path.clone());
                    file.new_path = Some(e.path.clone());
                    file.status = Change::Conflict;
                    file.unread = true;
                    return Some(file);
                }
                conflict_bytes += meta.len();
                crate::conflict::conflict_diff(&e.path, &std::fs::read(full).ok()?)
            })
            .collect();
        let mut asked_bytes = 0u64;
        unstaged.extend(self.side_diff(false, ignore_whitespace, whole, &mut asked_bytes)?);
        // An agent can make thousands of new files (a build, an unpacked folder): one read
        // takes the first ones, the others are listed unread until asked for.
        let (mut read, mut bytes) = (0, 0u64);
        for e in status
            .entries
            .iter()
            .filter(|e| e.unstaged == Some(Change::Untracked))
        {
            let size = working_size(&self.root.join(&e.path));
            let asked = whole.iter().any(|(staged, p)| !staged && *p == e.path)
                && asked_bytes + size <= ASKED_BYTES;
            // Over DIFF_LIMIT it is never read (Too large to show): it costs nothing.
            let huge = size > DIFF_LIMIT as u64;
            if huge || asked || (read < UNTRACKED_FILES && bytes + size <= UNTRACKED_BYTES) {
                if asked {
                    asked_bytes += size;
                } else if !huge {
                    read += 1;
                    bytes += size;
                }
                unstaged.push(self.untracked_diff(&e.path));
            } else {
                let mut file = FileDiff::empty();
                file.new_path = Some(e.path.clone());
                file.status = Change::Added;
                file.unread = true;
                unstaged.push(file);
            }
        }
        let staged = self.side_diff(true, ignore_whitespace, whole, &mut asked_bytes)?;
        let mut all = WorkingDiff { unstaged, staged };
        for file in all.unstaged.iter_mut().chain(all.staged.iter_mut()) {
            self.count(file);
        }
        Ok(all)
    }

    /// What every diff read passes: names as themselves (quotePath off: not as
    /// "caf\303\251"), fixed prefixes and no external diff (a user's diff.mnemonicPrefix,
    /// diff.noprefix or diff.external would change what the parser reads), renames found; `-w`
    /// with `ignore_whitespace`. `command` is "diff" or "diff-tree".
    fn diff_args(command: &'static str, ignore_whitespace: bool) -> Vec<&'static str> {
        let mut args = vec![
            "-c",
            "core.quotePath=false",
            command,
            "--no-color",
            "--no-ext-diff",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "-M",
        ];
        if ignore_whitespace {
            args.push("-w");
        }
        args
    }

    /// `name` as the full ref of the branch it names, the local one first: a tag or a hash of
    /// the same name never stands in for it. A name that is none stays as it is.
    fn full_ref(&self, name: &str) -> String {
        [format!("refs/heads/{name}"), format!("refs/remotes/{name}")]
            .into_iter()
            .find(|r| self.has_ref(r))
            .unwrap_or_else(|| name.to_string())
    }

    /// The commits `branch` has that `base` hasn't (`git log base..branch`), newest first.
    pub fn compare_log(&self, base: &str, branch: &str) -> Result<Vec<Commit>, GitError> {
        let format = format!("--format={}", parse::LOG_FORMAT);
        let date = format!("--date={}", parse::LOG_DATE);
        let range = format!("{}..{}", self.full_ref(base), self.full_ref(branch));
        let out = self.run(&["log", "-z", &format, &date, &range, "--"])?;
        Ok(parse::parse_log(&out))
    }

    /// What `branch` changed since it left `base` (`git diff base...branch`), as a pull request
    /// shows it.
    pub fn range_diff(
        &self,
        base: &str,
        branch: &str,
        ignore_whitespace: bool,
    ) -> Result<Vec<FileDiff>, GitError> {
        let range = format!("{}...{}", self.full_ref(base), self.full_ref(branch));
        let mut args = Self::diff_args("diff", ignore_whitespace);
        args.push(&range);
        args.push("--");
        Ok(parse_diff(&self.text(&args)?, DIFF_LIMIT))
    }

    /// One commit's changes: against its first parent (a merge's too), a root commit's against
    /// nothing.
    pub fn commit_changes(
        &self,
        hash: &str,
        ignore_whitespace: bool,
    ) -> Result<Vec<FileDiff>, GitError> {
        let mut args = Self::diff_args("diff-tree", ignore_whitespace);
        args.extend([
            "-p",
            "-r",
            "--root",
            "--no-commit-id",
            "--diff-merges=first-parent",
            hash,
        ]);
        Ok(parse_diff(&self.text(&args)?, DIFF_LIMIT))
    }

    /// One side's diff; the files in `whole` are read again while `asked_bytes` (shared by the
    /// whole read of the working diff) has room: one past it keeps its normal diff.
    fn side_diff(
        &self,
        staged: bool,
        ignore_whitespace: bool,
        whole: &[(bool, String)],
        asked_bytes: &mut u64,
    ) -> Result<Vec<FileDiff>, GitError> {
        let mut args = Self::diff_args("diff", ignore_whitespace);
        if staged {
            args.push("--cached");
        }
        let mut files = parse_diff(&self.text(&args)?, DIFF_LIMIT);
        for (_, path) in whole.iter().filter(|(s, _)| *s == staged) {
            let Some(i) = files.iter().position(|f| f.path() == path) else {
                continue;
            };
            // The working file's size stands for what the re-read will write (it is counted as
            // written once read), so a file past the budget is never read.
            let size = working_size(&self.root.join(path));
            if *asked_bytes + size > ASKED_BYTES {
                continue;
            }
            let mut args = args.clone();
            args.push(WHOLE);
            args.push("--");
            if files[i].status == Change::Renamed
                && let Some(from) = files[i].old_path.as_deref()
            {
                args.push(from);
            }
            args.push(path);
            let out = self.run_paths(&args)?;
            *asked_bytes += out.len() as u64;
            if let Some(file) = parse_diff(&String::from_utf8_lossy(&out), DIFF_LIMIT).pop() {
                files[i] = file;
            }
        }
        Ok(files)
    }

    /// An untracked file as a new file's diff: its text as added lines; binary, or too large
    /// past `DIFF_LIMIT` (not read).
    fn untracked_diff(&self, path: &str) -> FileDiff {
        let full = self.root.join(path);
        let size = working_size(&full);
        if size > DIFF_LIMIT as u64 {
            let mut file = FileDiff::empty();
            file.new_path = Some(path.to_string());
            file.status = Change::Added;
            file.too_large = true;
            return file;
        }
        let bytes = working_bytes(&full).unwrap_or_default();
        let text = if bytes.iter().take(8 * 1024).any(|b| *b == 0) {
            diff::new_binary_diff(path)
        } else {
            diff::new_file_diff(path, &String::from_utf8_lossy(&bytes))
        };
        parse_diff(&text, DIFF_LIMIT).pop().expect("one file")
    }

    /// The working file's size and line count: the file row's "12 KB", and the fold after
    /// its last hunk.
    fn count(&self, file: &mut FileDiff) {
        let Some(path) = &file.new_path else {
            return;
        };
        let full = self.root.join(path);
        let Ok(meta) = std::fs::symlink_metadata(&full) else {
            return;
        };
        file.bytes = Some(meta.len());
        if file.binary || file.too_large || file.unread || meta.len() > DIFF_LIMIT as u64 {
            return;
        }
        // ponytail: reads every changed file on each poll to count its lines; cache by mtime
        // if a tree with many large changed files stalls the background executor.
        if let Some(bytes) = working_bytes(&full) {
            let n = bytes.iter().filter(|b| **b == b'\n').count()
                + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"));
            file.lines = Some(n as u32);
        }
    }
}

/// Which side of a conflict to keep, whatever the operation: Ours is the `<<<<<<<` part (HEAD's,
/// the branch rebased onto in a rebase), Theirs the `>>>>>>>` one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Ours,
    Theirs,
}

impl Repo {
    /// Takes one side of a conflicted file (it stays unresolved until staged).
    pub fn resolve(&self, path: &str, side: Side) -> Result<(), GitError> {
        let flag = match side {
            Side::Ours => "--ours",
            Side::Theirs => "--theirs",
        };
        self.run_paths(&["checkout", flag, "--", path]).map(drop)
    }

    /// `git <operation> --abort`.
    pub fn abort(&self, op: Operation) -> Result<(), GitError> {
        self.run(&[op.name(), "--abort"]).map(drop)
    }

    /// `git <operation> --continue`; GIT_EDITOR=true keeps the prepared message.
    pub fn continue_(&self, op: Operation) -> Result<(), GitError> {
        self.run(&[op.name(), "--continue"]).map(drop)
    }
}
/// How `git pull` joins the upstream's commits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pull {
    #[default]
    FastForward,
    Merge,
    Rebase,
}

impl Repo {
    pub fn fetch(&self) -> Result<(), GitError> {
        self.run(&["fetch", "--prune"]).map(drop)
    }

    pub fn pull(&self, how: Pull) -> Result<(), GitError> {
        let flag = match how {
            Pull::FastForward => "--ff-only",
            Pull::Merge => "--no-rebase",
            Pull::Rebase => "--rebase",
        };
        self.run(&["pull", flag]).map(drop)
    }

    pub fn push(&self) -> Result<(), GitError> {
        self.run(&["push"]).map(drop)
    }

    /// `git push -u <remote> refs/heads/<branch>`: the branch goes to the server and follows it
    /// from then. The full ref keeps a name like `+main` or `a:b` from being read as a forced
    /// push or a `src:dst` refspec.
    pub fn publish(&self, remote: &str, branch: &str) -> Result<(), GitError> {
        self.run(&["push", "-u", remote, &format!("refs/heads/{branch}")])
            .map(drop)
    }

    /// Pushes `branch`, not the one checked out, to its upstream: the remote and the ref it
    /// follows are read from its config, so a branch tracking another name goes there.
    pub fn push_branch(&self, branch: &str) -> Result<(), GitError> {
        let get = |key: String| -> Result<String, GitError> {
            Ok(self.text(&["config", "--get", &key])?.trim().to_string())
        };
        let remote = get(format!("branch.{branch}.remote"))?;
        let merge = get(format!("branch.{branch}.merge"))?;
        self.run(&["push", &remote, &format!("refs/heads/{branch}:{merge}")])
            .map(drop)
    }

    /// The remotes' names, as `git remote` lists them.
    pub fn remotes(&self) -> Result<Vec<String>, GitError> {
        Ok(self
            .text(&["remote"])?
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    }

    /// The branch a pull request would go into: the one `origin/HEAD` names, else `main`, else
    /// `master`; the local one when there is one ("main"), else origin's ("origin/main"). None
    /// when there is none of them.
    pub fn default_branch(&self) -> Option<String> {
        let has = |r: String| self.run(&["rev-parse", "--verify", "-q", &r]).is_ok();
        let mut names = Vec::new();
        // parse_branches leaves origin/HEAD out: it is its own command.
        if let Ok(head) = self.text(&["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"])
            && let Some(name) = head.trim().strip_prefix("origin/")
        {
            names.push(name.to_string());
        }
        names.extend(["main".to_string(), "master".to_string()]);
        names.into_iter().find_map(|name| {
            if has(format!("refs/heads/{name}")) {
                Some(name)
            } else if has(format!("refs/remotes/origin/{name}")) {
                Some(format!("origin/{name}"))
            } else {
                None
            }
        })
    }
}

/// What Sync did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Synced {
    /// Got `got` commits from the server, sent `sent`.
    Done { got: u32, sent: u32 },
    /// Both sides have new commits: nothing was changed; `sync_join` joins them.
    Diverged { ahead: u32, behind: u32 },
    /// The branch has no upstream any more (deleted on the server, pruned by the fetch): nothing
    /// was sent; Publish sends it again.
    Gone,
    /// The branch has no upstream yet (never published): nothing was sent; Publish sends it.
    Unpublished,
}

impl Repo {
    /// Sync: fetch, then a fast-forward to the upstream when it has new commits, then a push
    /// when this branch has new ones. The counts are the status's after the fetch (what each
    /// side has that the other hasn't). Both new: nothing is joined. An upstream the fetch found
    /// deleted: nothing is sent.
    pub fn sync(&self) -> Result<Synced, GitError> {
        self.fetch()?;
        let s = self.status()?;
        // No branch.ab: its counts mean nothing, and a push would have nowhere to go.
        if s.upstream.is_none() {
            return Ok(Synced::Unpublished);
        }
        if s.gone {
            return Ok(Synced::Gone);
        }
        let (sent, got) = (s.ahead, s.behind);
        if sent > 0 && got > 0 {
            return Ok(Synced::Diverged {
                ahead: sent,
                behind: got,
            });
        }
        if got > 0 {
            self.run(&["merge", "--ff-only", "-q", "@{u}"])?;
        }
        if sent > 0 {
            self.push()?;
        }
        Ok(Synced::Done { got, sent })
    }

    /// A diverged Sync, joined: the user's commits put on top (`rebase`) or a merge, either one
    /// with `--autostash` (a rebase refuses to start over uncommitted changes), then the push.
    pub fn sync_join(&self, rebase: bool) -> Result<Synced, GitError> {
        let got = self.status()?.behind;
        let how = if rebase { "--rebase" } else { "--no-rebase" };
        self.run(&["pull", how, "--autostash"])?;
        let sent = self.status()?.ahead;
        if sent > 0 {
            self.push()?;
        }
        Ok(Synced::Done { got, sent })
    }
}

/// Where History's search looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Search {
    /// Commit messages (`--grep`), without case.
    #[default]
    Message,
    /// Authors' names and emails (`--author`), without case.
    Author,
    /// Commits that added or removed the text (`-S`).
    Code,
}

/// What History lists.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryQuery {
    /// HEAD and every branch, tag and remote branch (not `--all`, which has the stashes); else
    /// HEAD's history.
    pub all: bool,
    pub search: Option<(Search, String)>,
}

impl Repo {
    /// `count` commits of the query after the first `skip`, in `--date-order` (children before
    /// parents, so the graph's lanes never cross back; every page shares it). Searches take the
    /// text as written (`-F`: a bracket is a bracket). Before the first commit: none.
    pub fn history(
        &self,
        q: &HistoryQuery,
        skip: usize,
        count: usize,
    ) -> Result<Vec<Commit>, GitError> {
        let format = format!("--format={}", parse::LOG_FORMAT);
        let date = format!("--date={}", parse::LOG_DATE);
        let (skip, count) = (format!("--skip={skip}"), format!("-n{count}"));
        let search: Vec<String> = match &q.search {
            Some((Search::Message, t)) => vec!["-i".into(), "-F".into(), format!("--grep={t}")],
            Some((Search::Author, t)) => vec!["-i".into(), "-F".into(), format!("--author={t}")],
            Some((Search::Code, t)) => vec![format!("-S{t}")],
            None => Vec::new(),
        };
        let mut args = vec!["log", "-z", &format, &date, "--date-order", &skip, &count];
        if q.all {
            args.extend(["HEAD", "--branches", "--tags", "--remotes"]);
        }
        args.extend(search.iter().map(String::as_str));
        args.push("--");
        match self.run(&args) {
            Ok(out) => Ok(parse::parse_log(&out)),
            Err(e)
                if e.stderr.contains("does not have any commits")
                    || e.stderr.contains("unknown revision") =>
            {
                Ok(Vec::new())
            }
            Err(e) => Err(e),
        }
    }

    /// One commit, as History lists it (Blame opens one that isn't loaded).
    pub fn commit_info(&self, hash: &str) -> Result<Commit, GitError> {
        let format = format!("--format={}", parse::LOG_FORMAT);
        let date = format!("--date={}", parse::LOG_DATE);
        let out = self.run(&["log", "-z", "-1", &format, &date, hash, "--"])?;
        parse::parse_log(&out).pop().ok_or_else(|| GitError {
            command: shown(&self.program, &["log", "-1", hash]),
            code: None,
            stderr: format!("fatal: no commit {hash}"),
            missing: false,
        })
    }

    /// HEAD's commits not on its upstream (`git rev-list @{u}..HEAD`): History's "not pushed".
    /// None with no upstream, or one gone from the server.
    pub fn unpushed(&self) -> Result<HashSet<String>, GitError> {
        if self.run(&["rev-parse", "--verify", "-q", "@{u}"]).is_err() {
            return Ok(HashSet::new());
        }
        Ok(self
            .text(&["rev-list", "@{u}..HEAD"])?
            .lines()
            .map(str::to_string)
            .collect())
    }

    /// `path`'s history through its renames (`--follow`), each commit with the file's path then.
    pub fn file_log(
        &self,
        path: &str,
        skip: usize,
        count: usize,
    ) -> Result<Vec<(Commit, CommitFile)>, GitError> {
        let format = format!("--format={}", parse::LOG_FORMAT);
        let date = format!("--date={}", parse::LOG_DATE);
        // `--skip` counts the commits `--follow` leaves out too (git 2.55: skipping 1 skips a
        // commit that didn't touch the file), `-n` only the shown ones: read the first
        // `skip + count` and drop the first `skip` here. Parsing from the newest also keeps
        // each entry's path right across a rename on an earlier page.
        let count = format!("-n{}", skip + count);
        let out = self.run_paths(&[
            "-c",
            "core.quotePath=false",
            "log",
            "-z",
            &format,
            &date,
            "--follow",
            "--name-status",
            "-M",
            &count,
            "--",
            path,
        ])?;
        let mut log = parse::parse_file_log(&out, path);
        log.drain(..skip.min(log.len()));
        Ok(log)
    }

    /// `path` as it was in `rev`, as a checkout would write it: `git cat-file --filters` applies
    /// the working tree's line endings and filters to the stored bytes.
    pub fn file_at(&self, rev: &str, path: &str) -> Result<Vec<u8>, GitError> {
        self.run(&["cat-file", "--filters", &format!("{rev}:{path}")])
    }

    /// Who last changed each line of `path`: in the working tree, or as it was in `rev`.
    // ponytail: no size limit; a blame of a multi-megabyte file is slow but the panel's rows
    // are virtual. Refuse past DIFF_LIMIT if one stalls the block.
    pub fn blame(&self, path: &str, rev: Option<&str>) -> Result<Vec<BlameLine>, GitError> {
        let mut args = vec!["blame", "--porcelain"];
        args.extend(rev);
        args.extend(["--", path]);
        Ok(crate::blame::parse_blame(&self.run(&args)?))
    }

    pub fn commit_files(&self, hash: &str) -> Result<Vec<CommitFile>, GitError> {
        let out = self.run(&[
            "diff-tree",
            "--root",
            "--no-commit-id",
            "-r",
            "-z",
            "-M",
            "--name-status",
            hash,
        ])?;
        Ok(parse::parse_name_status(&out))
    }

    /// Detached at `hash`.
    pub fn checkout_commit(&self, hash: &str) -> Result<(), GitError> {
        self.run(&["checkout", "-q", "--detach", hash]).map(drop)
    }

    /// A branch named `name` at `from` (HEAD without), checked out. A name git refuses, or a
    /// remote branch's own (`origin/main`: git would make it, and from then on `origin/main`
    /// would be ambiguous and name this one), is an error before anything is made.
    pub fn new_branch(&self, name: &str, from: Option<&str>) -> Result<(), GitError> {
        self.check_ref_format(name)?;
        if self.has_ref(&format!("refs/remotes/{name}")) {
            return Err(self.fatal(
                &["checkout", "-b", name],
                format!(
                    "'{name}' is a remote branch: a local branch of that name would \
                     make '{name}' ambiguous."
                ),
            ));
        }
        if self.has_ref(&format!("refs/tags/{name}")) {
            return Err(self.fatal(
                &["checkout", "-b", name],
                format!("'{name}' is a tag: a branch of that name would make '{name}' ambiguous."),
            ));
        }
        let mut args = vec!["checkout", "-q", "-b", name];
        if let Some(from) = from {
            args.push(from);
        }
        self.run(&args).map(drop)
    }

    /// A tag `name` at `hash`; not one a branch is named: `name` would be ambiguous.
    pub fn tag(&self, name: &str, hash: &str) -> Result<(), GitError> {
        if self.has_ref(&format!("refs/heads/{name}")) {
            return Err(self.fatal(
                &["tag", name, hash],
                format!("'{name}' is a branch: a tag of that name would make '{name}' ambiguous."),
            ));
        }
        self.run(&["tag", name, hash]).map(drop)
    }

    /// `full` (a ref's whole name) exists.
    fn has_ref(&self, full: &str) -> bool {
        self.run(&["show-ref", "--verify", "--quiet", full]).is_ok()
    }

    /// A refusal as git would word it, for the command `args` that wasn't run.
    fn fatal(&self, args: &[&str], why: String) -> GitError {
        GitError {
            command: shown(&self.program, args),
            code: None,
            stderr: format!("fatal: {why}"),
            missing: false,
        }
    }

    pub fn revert(&self, hash: &str) -> Result<(), GitError> {
        self.run(&["revert", "--no-edit", hash]).map(drop)
    }

    /// Ok when git accepts `name` as a branch name.
    pub fn check_ref_format(&self, name: &str) -> Result<(), GitError> {
        self.run(&["check-ref-format", "--branch", name]).map(drop)
    }

    /// Local and remote branches, sorted as git lists them (local first).
    pub fn branches(&self) -> Result<Vec<Branch>, GitError> {
        let format = format!("--format={}", parse::BRANCH_FORMAT);
        let out = self.run(&["for-each-ref", &format, "refs/heads", "refs/remotes"])?;
        Ok(parse::parse_branches(&out))
    }

    /// The branch `name`, checked out. The `--` makes it a branch only: a name that isn't one
    /// (deleted meanwhile) but is a folder's would otherwise restore that folder from the index.
    pub fn checkout(&self, name: &str) -> Result<(), GitError> {
        self.run(&["checkout", "-q", name, "--"]).map(drop)
    }

    /// "origin/x": a local x tracking it, checked out.
    pub fn checkout_remote(&self, name: &str) -> Result<(), GitError> {
        self.run(&["checkout", "-q", "--track", name, "--"])
            .map(drop)
    }

    pub fn rename_branch(&self, from: &str, to: &str) -> Result<(), GitError> {
        self.check_ref_format(to)?;
        self.run(&["branch", "-m", from, to]).map(drop)
    }

    /// Merges `name` into the current branch.
    pub fn merge(&self, name: &str) -> Result<(), GitError> {
        self.run(&["merge", "--no-edit", &self.full_ref(name)])
            .map(drop)
    }

    pub fn delete_branch(&self, name: &str, force: bool) -> Result<(), GitError> {
        self.run(&["branch", if force { "-D" } else { "-d" }, name])
            .map(drop)
    }

    /// The commits only `branch` has (on no other branch, local or remote): what deleting it
    /// loses.
    // ponytail: a name holding glob characters (`*?[`) is read as a pattern by --exclude;
    // escape it if such names show up.
    pub fn unmerged_count(&self, branch: &str) -> Result<u32, GitError> {
        let out = self.text(&[
            "rev-list",
            "--count",
            &format!("refs/heads/{branch}"),
            "--not",
            &format!("--exclude={branch}"),
            "--branches",
            "--remotes",
        ])?;
        Ok(out.trim().parse().unwrap_or(0))
    }

    /// `git stash push -m message`, with untracked files when asked.
    pub fn stash_push(&self, message: &str, untracked: bool) -> Result<(), GitError> {
        let mut args = vec!["stash", "push", "-q", "-m", message];
        if untracked {
            args.push("-u");
        }
        self.run(&args).map(drop)
    }

    pub fn stashes(&self) -> Result<Vec<Stash>, GitError> {
        let format = format!("--format={}", parse::STASH_FORMAT);
        let out = self.run(&["stash", "list", "-z", &format])?;
        Ok(parse::parse_stashes(&out))
    }

    /// `stash@{N}` when it is still `stash`; an error when the list changed since it was read.
    fn stash_ref(&self, stash: &Stash) -> Result<String, GitError> {
        let name = format!("stash@{{{}}}", stash.index);
        let now = self
            .text(&["rev-parse", "--verify", "-q", &name])
            .unwrap_or_default();
        if now.trim() == stash.hash {
            Ok(name)
        } else {
            Err(GitError {
                command: shown(&self.program, &["stash", "list"]),
                code: None,
                stderr: "error: the stash list changed since it was read; it is read again"
                    .to_string(),
                missing: false,
            })
        }
    }

    pub fn stash_apply(&self, stash: &Stash) -> Result<(), GitError> {
        let name = self.stash_ref(stash)?;
        self.run(&["stash", "apply", "-q", &name]).map(drop)
    }

    pub fn stash_pop(&self, stash: &Stash) -> Result<(), GitError> {
        let name = self.stash_ref(stash)?;
        self.run(&["stash", "pop", "-q", &name]).map(drop)
    }

    pub fn stash_drop(&self, stash: &Stash) -> Result<(), GitError> {
        let name = self.stash_ref(stash)?;
        self.run(&["stash", "drop", "-q", &name]).map(drop)
    }

    /// A stash's changes, its new (untracked) files too, as the panel reads them.
    pub fn stash_diff(
        &self,
        stash: &Stash,
        ignore_whitespace: bool,
    ) -> Result<Vec<FileDiff>, GitError> {
        let name = self.stash_ref(stash)?;
        let mut args = vec![
            "-c",
            "core.quotePath=false",
            // Not --src-prefix/--dst-prefix: with --include-untracked, git 2.55 mangles them
            // ("diff --git a.txt ludea.txt"). The default prefixes, whatever the user's config.
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.mnemonicPrefix=false",
            "stash",
            "show",
            "-p",
            "--include-untracked",
            "--no-color",
            "--no-ext-diff",
            "-M",
        ];
        if ignore_whitespace {
            args.push("-w");
        }
        args.push(&name);
        Ok(parse_diff(&self.text(&args)?, DIFF_LIMIT))
    }
}

/// What a switch found of the changes once left on the branch it went to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Back {
    /// None were left there.
    Nothing,
    /// They are back in the working tree and their stash is gone.
    Popped,
    /// They couldn't come back cleanly (a conflict, a file in the way): git's line. The stash
    /// stays in Stashes.
    Kept(String),
}

/// The message of the stash a switch leaves changes in.
pub fn left_on(name: &str) -> String {
    format!("templum: left on {name}")
}

impl Repo {
    /// Switches to `to`: a local branch, or a remote one ("origin/x"), which goes to the local x
    /// when there is one and else makes it, tracking origin/x. With `leave` (the branch being
    /// left, or its short hash on a detached HEAD), the uncommitted changes, untracked files
    /// too, go first into a stash named `left_on(leave)`; a switch that then fails puts them
    /// straight back, so nothing stays hidden. Once switched, the newest stash left on `to` is
    /// popped (the others stay in Stashes).
    pub fn switch(&self, to: &str, remote: bool, leave: Option<&str>) -> Result<Back, GitError> {
        let local = if remote {
            to.split_once('/').map_or(to, |(_, b)| b)
        } else {
            to
        };
        let top = || {
            self.text(&["rev-parse", "-q", "--verify", "refs/stash"])
                .unwrap_or_default()
        };
        let mut stashed = false;
        if let Some(name) = leave {
            let before = top();
            self.stash_push(&left_on(name), true)?;
            // Nothing to stash makes no stash: never pop someone else's.
            stashed = top() != before;
        }
        let has_local = self
            .run(&[
                "rev-parse",
                "--verify",
                "-q",
                &format!("refs/heads/{local}"),
            ])
            .is_ok();
        let switched = if remote && !has_local {
            self.checkout_remote(to)
        } else {
            self.checkout(local)
        };
        if let Err(e) = switched {
            if stashed {
                let _ = self.pop_whole("stash@{0}");
            }
            return Err(e);
        }
        let mine = format!("On {local}: {}", left_on(local));
        let Some(stash) = self.stashes()?.into_iter().find(|s| s.message == mine) else {
            return Ok(Back::Nothing);
        };
        let name = self.stash_ref(&stash)?;
        Ok(match self.pop_whole(&name) {
            Ok(()) => Back::Popped,
            Err(e) => Back::Kept(e.first_line().to_string()),
        })
    }

    /// Pops a stash with its staged changes staged again (`--index`); when git can't do that
    /// part, as plain changes.
    fn pop_whole(&self, name: &str) -> Result<(), GitError> {
        match self.run(&["stash", "pop", "-q", "--index", name]) {
            Err(e) if e.stderr.contains("Try without --index") => {
                self.run(&["stash", "pop", "-q", name]).map(drop)
            }
            r => r.map(drop),
        }
    }
}

/// `name` can name a branch and a folder under `.worktrees`: letters, digits, `-`, `_`, `.` and
/// `/` between its parts; no part empty, starting or ending with `.`; no `..`; no `.lock` at the
/// end; not starting with `-` (git would take it for an option). Git refuses whatever else a
/// branch can't be.
fn worktree_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('-')
        && !name.contains("..")
        && !name.ends_with(".lock")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c))
        && name
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with('.'))
}

/// Adds `line` to the file at `path` (made, with its folder, when missing) unless one of its
/// lines already is that.
fn add_line(path: &Path, line: &str) -> io::Result<()> {
    let mut text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    if text.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(line);
    text.push('\n');
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(path, text)
}

/// The work tree holding `file`, without running git: the nearest folder above it with a `.git`
/// (a folder, or a file in a worktree or a submodule).
pub fn root_of(file: &Path) -> Option<&Path> {
    file.ancestors().skip(1).find(|d| d.join(".git").exists())
}

/// The folder `git clone <url>` would make: the URL's last part without `.git` ("app" for
/// `git@github.com:me/app.git` and `https://host/me/app/`).
pub fn clone_name(url: &str) -> String {
    let url = url.trim().trim_end_matches(['/', '\\']);
    let url = url.strip_suffix(".git").unwrap_or(url);
    url.rsplit(['/', ':', '\\'])
        .next()
        .unwrap_or("")
        .to_string()
}

/// Where a clone of `url` into `dest` goes: `dest` itself when it is an empty folder or doesn't
/// exist yet, else a folder named after the repository inside it.
pub fn clone_target(dest: &Path, url: &str) -> PathBuf {
    let empty = std::fs::read_dir(dest).map_or(true, |mut d| d.next().is_none());
    if empty {
        dest.to_path_buf()
    } else {
        dest.join(clone_name(url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program busy for an instant is run again; any other failure is said at once.
    #[test]
    fn a_busy_program_is_run_again_and_other_failures_are_not() {
        let mut tries = 0;
        let ran = retry_busy(|| {
            tries += 1;
            if tries < 3 {
                Err(io::Error::from(io::ErrorKind::ExecutableFileBusy))
            } else {
                Ok(tries)
            }
        });
        assert_eq!(ran.unwrap(), 3);
        let mut tries = 0;
        let missing = retry_busy(|| -> io::Result<()> {
            tries += 1;
            Err(io::Error::from(io::ErrorKind::NotFound))
        });
        assert_eq!(missing.unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(tries, 1);
    }

    /// The API gives every git call ten minutes; a command past its deadline is killed.
    #[cfg(unix)]
    #[test]
    fn a_deadline_kills_a_command_that_runs_past_it() {
        let started = Instant::now();
        let err = run_in(
            "sleep",
            Path::new("."),
            &["30"],
            None,
            Some(started + Duration::from_millis(100)),
        )
        .unwrap_err();
        assert!(err.cancelled(), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    /// A call of the API reads at most [`API_OUTPUT_MOST`] of what a command writes: past it the
    /// command is killed, not read to its end.
    #[cfg(unix)]
    #[test]
    fn a_command_writing_past_the_most_is_killed() {
        let at = Instant::now() + Duration::from_secs(60);
        let started = Instant::now();
        let err = run_in(
            "sh",
            Path::new("."),
            &["-c", "head -c 3000000000 /dev/zero"],
            None,
            Some(at),
        )
        .unwrap_err();
        assert!(err.too_large(), "{err}");
        assert!(started.elapsed() < Duration::from_secs(30));
        // Without a deadline (the popup's) nothing is cut.
        let out = run_in(
            "sh",
            Path::new("."),
            &["-c", "head -c 70000000 /dev/zero"],
            None,
            None,
        )
        .unwrap();
        assert_eq!(out.len(), 70_000_000);
    }

    /// What git started (ssh, a hook, a credential helper) dies with it at the deadline.
    #[cfg(unix)]
    #[test]
    fn a_deadline_kills_what_the_command_started() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("pid");
        let script = format!("sleep 60 & echo $! > {}; wait", pidfile.display());
        let mut c = Command::new("sh");
        c.args(["-c", &script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let deadline = Instant::now() + Duration::from_millis(500);
        let out = until_cancelled(&mut c, &NEVER, Some(deadline), true, None).unwrap();
        assert!(out.is_none());
        let pid = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .to_string();
        let alive = || {
            Command::new("kill")
                .args(["-0", &pid])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        };
        let until = Instant::now() + Duration::from_secs(5);
        while alive() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(), "the sleep it started is still running");
    }

    /// A call with a deadline is the API's: ssh and credential helpers never ask anyone.
    #[cfg(unix)]
    #[test]
    fn a_call_with_a_deadline_never_waits_on_a_prompt() {
        if std::env::var_os("SSH_ASKPASS_REQUIRE").is_some() {
            return;
        }
        let args = ["-c", "alias.env=!echo $SSH_ASKPASS_REQUIRE", "env"];
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo::init("git", dir.path()).unwrap();
        assert_eq!(repo.run(&args).unwrap(), b"\n");
        let at = Instant::now() + Duration::from_secs(60);
        assert_eq!(repo.with_deadline(at).run(&args).unwrap(), b"force\n");
    }
}
