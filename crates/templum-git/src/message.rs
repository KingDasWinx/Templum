//! A commit message from an agent: the request built from a template, the agent's command run
//! through the user's login shell (with a timeout and a cancel), and its answer split into a
//! summary and a description. No UI: see docs/specs/git.md, section 7.

use std::fmt;
use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::diff::{new_binary_diff, new_file_diff};
use crate::parse::{Change, parse_name_status};
use crate::repo::{DIFF_LIMIT, GitError, Repo, until_cancelled};

/// The request when the user hasn't written their own (Settings ▸ Git ▸ Prompt): Templum's rules
/// for a good commit message first, the person's last, after the diff, where an agent heeds them
/// best (a long diff between drowns them); theirs win.
pub const PROMPT: &str = "\
You write git commit messages. This is a one-shot request from Templum's Generate button, not a \
conversation: ask nothing, run no tools, explain nothing. Answer with the commit message alone.

The message:
- Its first line is the summary, at most 72 characters, in Conventional Commits form: \
type(scope): summary. The type is feat, fix, perf, refactor, docs, test, build, ci, chore or \
style; the scope is the area changed, left out when there is none; the summary is in the \
imperative mood (\"add\", not \"added\" or \"adds\"), with no period at the end.
- Then a blank line and a body of a few short lines, wrapped at 72 characters, saying what \
changed and why, when the summary alone doesn't say it; none for a change the summary says whole.
- Plain text: no Markdown, no code fences, no quotes around it, nothing before or after it.
- No credit to an AI or a tool: no Co-Authored-By or \"Generated with\" line.
- Take the types, scopes, language and casing from this repository's last commits below; if they \
follow another convention than Conventional Commits, follow theirs.
- The person's own rules, if any, are at the end, after the diff: where they disagree with these \
rules, the summary's form included, follow theirs.

The last commits' summaries in this repository, newest first:
{recent_commits}

The changed files:
{files}

{truncated}
The diff:
{diff}

The person's own rules:
{instructions}
";

/// Settings ▸ Git ▸ Try it's change: a small fix in a file of no repository, so none of the
/// user's code is sent.
pub const SAMPLE_FILE: &str = "M src/greet.rs";
pub const SAMPLE_DIFF: &str = "\
diff --git a/src/greet.rs b/src/greet.rs
--- a/src/greet.rs
+++ b/src/greet.rs
@@ -1,3 +1,3 @@
 pub fn greet(name: &str) -> String {
-    format!(\"Helo, {name}\")
+    format!(\"Hello, {name}!\")
 }
";

/// What a request holds besides its template.
pub struct Request<'a> {
    /// The user's commit rules.
    pub instructions: &'a str,
    /// Every changed file, one per line ("M src/x.rs"), whole even when the diff is cut.
    pub files: &'a [String],
    pub diff: &'a str,
    /// The last commits' summaries, newest first.
    pub recent: &'a [String],
    /// The diff past this many bytes is cut, at a line's end.
    pub max_bytes: usize,
}

/// `template` with its placeholders filled: `{instructions}`, `{files}`, `{diff}`,
/// `{recent_commits}`, and `{truncated}`, a sentence when the diff was cut (else nothing).
pub fn request(template: &str, r: &Request) -> String {
    let (diff, was_cut) = cut(r.diff, r.max_bytes);
    let truncated = if was_cut {
        "The diff was cut: the list of files above is complete."
    } else {
        ""
    };
    fill(
        template,
        &[
            ("instructions", r.instructions),
            ("files", &r.files.join("\n")),
            ("recent_commits", &r.recent.join("\n")),
            ("truncated", truncated),
            ("diff", diff),
        ],
    )
}

/// `template` with each `{name}` of `values` replaced, in one pass: a value's own text is never
/// read for placeholders (a diff can hold "{files}"). Other braces stay as they are.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let found = values
            .iter()
            .find(|(name, _)| after.strip_prefix(name).is_some_and(|r| r.starts_with('}')));
        match found {
            Some((name, value)) => {
                out.push_str(value);
                rest = &after[name.len() + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `diff` up to `max` bytes, cut after the last whole line that fits (never inside a
/// character), and whether it was cut.
fn cut(diff: &str, max: usize) -> (&str, bool) {
    if diff.len() <= max {
        return (diff, false);
    }
    let end = diff.as_bytes()[..max]
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |i| i + 1);
    (&diff[..end], true)
}

/// An agent's answer as a summary (its first line) and a description (the rest), without the
/// code fences an agent may add anyway. None when it has no summary.
pub fn split(answer: &str) -> Option<(String, String)> {
    let text = answer
        .lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    let (summary, description) = text.split_once('\n').unwrap_or((text, ""));
    let summary = summary.trim();
    (!summary.is_empty()).then(|| (summary.to_string(), description.trim().to_string()))
}

/// Why an agent gave no message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Reading the repository failed.
    Git(GitError),
    /// The shell couldn't start, or the request couldn't be written.
    CouldntRun(String),
    /// The command ended with an error: its exit code (None: stopped by a signal) and stderr.
    Failed { code: Option<i32>, stderr: String },
    /// It ran past the timeout, and was stopped.
    TimedOut(Duration),
    /// It ended well, with no answer.
    Empty { stderr: String },
    /// The person stopped it.
    Cancelled,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Git(e) => write!(f, "{}", e.first_line()),
            Failure::CouldntRun(why) => write!(f, "Couldn't run the agent's command: {why}"),
            Failure::Failed {
                code: Some(code), ..
            } => write!(f, "The agent's command failed (exit code {code})"),
            Failure::Failed { code: None, .. } => {
                write!(f, "The agent's command was stopped by a signal")
            }
            Failure::TimedOut(t) => {
                write!(f, "The agent didn't answer within {} s", t.as_secs())
            }
            Failure::Empty { .. } => write!(f, "The agent answered nothing"),
            Failure::Cancelled => write!(f, "Cancelled"),
        }
    }
}

impl Failure {
    /// What Details shows: the sentence, then what the command wrote to stderr (git's command
    /// and stderr for a git error).
    pub fn details(&self) -> String {
        let more = match self {
            Failure::Git(e) => format!("{}\n{}", e.command, e.stderr),
            Failure::Failed { stderr, .. } | Failure::Empty { stderr } => stderr.clone(),
            _ => String::new(),
        };
        if more.trim().is_empty() {
            self.to_string()
        } else {
            format!("{self}\n\n{}", more.trim_end())
        }
    }
}

/// Runs `command`, a generate command, through the user's login shell in `folder`: `{model}`
/// is `model`; `request` is its standard input, or the file `{prompt_file}` names; the answer is
/// its standard output, or what it wrote to the file `{output_file}` names, with `env` set (an
/// account's folder). Stopped past `timeout`, or as soon as `cancel` is set.
pub fn run(
    command: &str,
    model: &str,
    request: &str,
    folder: &Path,
    timeout: Duration,
    cancel: &AtomicBool,
    env: &[(String, String)],
) -> Result<String, Failure> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Failure::Cancelled);
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    // The request holds the person's code: in a folder only they can read, made new (never one
    // someone else put there first).
    let dir = std::env::temp_dir().join(format!("templum-{}-{n}", std::process::id()));
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut folder_of_files = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut folder_of_files, 0o700);
    folder_of_files
        .create(&dir)
        .map_err(|e| Failure::CouldntRun(format!("couldn't make {}: {e}", dir.display())))?;
    let (prompt_file, output_file) = (dir.join("request.txt"), dir.join("answer.txt"));
    let result = (|| {
        std::fs::write(&prompt_file, request)
            .map_err(|e| Failure::CouldntRun(format!("couldn't write the request: {e}")))?;
        let line = fill(
            command,
            &[
                ("model", &quote(model)),
                ("prompt_file", &quote(&prompt_file.to_string_lossy())),
                ("output_file", &quote(&output_file.to_string_lossy())),
            ],
        );
        let stdin = if command.contains("{prompt_file}") {
            Stdio::null()
        } else {
            Stdio::from(
                File::open(&prompt_file)
                    .map_err(|e| Failure::CouldntRun(format!("couldn't read the request: {e}")))?,
            )
        };
        let (shell, args) = login_shell();
        let mut c = Command::new(&shell);
        c.args(args)
            .arg(&line)
            .current_dir(folder)
            // Claude Code's own commit rules end every message with a Co-Authored-By trailer:
            // the user's instructions are the only rules (they ask for one there if they want it).
            .env("CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS", "1")
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // No console window flashing behind the app.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let out = match until_cancelled(&mut c, cancel, Some(Instant::now() + timeout), false, None)
        {
            Err(e) => return Err(Failure::CouldntRun(format!("{shell}: {e}"))),
            Ok(None) if cancel.load(Ordering::Relaxed) => return Err(Failure::Cancelled),
            Ok(None) => return Err(Failure::TimedOut(timeout)),
            Ok(Some(out)) => out,
        };
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        if !out.status.success() {
            return Err(Failure::Failed {
                code: out.status.code(),
                stderr,
            });
        }
        let answer = if command.contains("{output_file}") {
            decode(std::fs::read(&output_file).unwrap_or_default())
        } else {
            decode(out.stdout)
        };
        if answer.trim().is_empty() {
            Err(Failure::Empty { stderr })
        } else {
            Ok(answer)
        }
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// The user's login shell and how it takes a command line, as a terminal would run it (PATH,
/// aliases, version managers): `$SHELL -l -c`, `/bin/sh` without `$SHELL` (an app opened from
/// a desktop menu may have none); PowerShell on Windows.
// ponytail: on Windows a native program's standard input through `-Command` and its real exit
// code (PowerShell reports 1) are unchecked; append `; exit $LASTEXITCODE` and test on Windows
// if a Windows user's agent needs them.
fn login_shell() -> (String, &'static [&'static str]) {
    if cfg!(windows) {
        (
            "powershell".to_string(),
            &["-NoLogo", "-NonInteractive", "-Command"],
        )
    } else {
        let shell = std::env::var("SHELL").ok().filter(|s| !s.trim().is_empty());
        (
            shell.unwrap_or_else(|| "/bin/sh".to_string()),
            &["-l", "-c"],
        )
    }
}

/// `s` as one word for `login_shell`'s shell: single quotes, each quote inside escaped its way
/// (sh, bash, zsh and fish take `'\''`; PowerShell `''`).
fn quote(s: &str) -> String {
    if cfg!(windows) {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// An answer's text: UTF-8 (a byte-order mark dropped), or UTF-16 with its byte-order mark, what
/// Windows PowerShell's `>` writes.
fn decode(bytes: Vec<u8>) -> String {
    match bytes.as_slice() {
        [0xFF, 0xFE, rest @ ..] => String::from_utf16_lossy(
            &rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_le_bytes(*p))
                .collect::<Vec<_>>(),
        ),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(&bytes).into_owned(),
    }
}

/// The agent that writes commit messages, and how: what Generate runs (the popup builds it from
/// Settings ▸ Git, the API's `git` tool at home likewise). It never crosses the network.
#[derive(Clone, Debug)]
pub struct Writer {
    /// Its generate command (`{model}` is `model`).
    pub command: String,
    pub model: String,
    /// An account's variable, naming its folder.
    pub env: Vec<(String, String)>,
    /// This repository's commit rules, else Settings ▸ Git's.
    pub instructions: String,
    /// The request's template: [`PROMPT`] unless the person wrote their own.
    pub template: String,
    /// The diff past this many bytes is cut.
    pub max_bytes: usize,
    pub timeout: Duration,
}

/// Reads what the commit would take, builds the request and runs the writer's agent. The bool
/// says HEAD moved meanwhile: a commit made from a terminal took what the answer describes.
pub fn generate(
    repo: &Repo,
    w: &Writer,
    all: bool,
    cancel: &AtomicBool,
) -> (Result<String, Failure>, bool) {
    let head = repo.head().ok();
    let answer = (|| {
        let changes = changes(repo, all, w.max_bytes).map_err(Failure::Git)?;
        let request = request(
            &w.template,
            &Request {
                instructions: &w.instructions,
                files: &changes.files,
                diff: &changes.diff,
                recent: &changes.recent,
                max_bytes: w.max_bytes,
            },
        );
        run(
            &w.command, &w.model, &request, &repo.root, w.timeout, cancel, &w.env,
        )
    })();
    (answer, repo.head().ok() != head)
}

/// What a request is about.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// Every changed file, "M src/x.rs" ("R old -> new" for a rename), whole even when the diff
    /// is cut.
    pub files: Vec<String>,
    pub diff: String,
    /// The last 10 commits' summaries, newest first; none before the first commit.
    pub recent: Vec<String>,
}

/// What the next commit would take: the staged changes; with `all`, every change as Commit all
/// changes takes them (`git add --all`), the tracked files against HEAD, then each new file's
/// text. A new file is listed but not read once the diff is past `max_bytes` (the request cuts
/// it there anyway), or when it is past `DIFF_LIMIT` itself.
pub fn changes(repo: &Repo, all: bool, max_bytes: usize) -> Result<Changes, GitError> {
    // Before the first commit there is no HEAD to compare with: what is staged is all there is.
    let against = if all && repo.head().is_ok() {
        "HEAD"
    } else {
        "--cached"
    };
    // As the diff panel reads it: names as themselves, the usual prefixes, no external diff.
    let diff = repo.run(&[
        "-c",
        "core.quotePath=false",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "-M",
        against,
    ])?;
    let mut diff = String::from_utf8_lossy(&diff).into_owned();
    let names = repo.run(&["diff", "--name-status", "-z", "-M", against])?;
    let mut files: Vec<String> = parse_name_status(&names)
        .iter()
        .map(|f| match &f.from {
            Some(from) => format!("{} {from} -> {}", f.change.letter(), f.path),
            None => format!("{} {}", f.change.letter(), f.path),
        })
        .collect();
    if all {
        let status = repo.status()?;
        for entry in status
            .entries
            .iter()
            .filter(|e| e.unstaged == Some(Change::Untracked))
        {
            files.push(format!("A {}", entry.path));
            let path = repo.root.join(&entry.path);
            let size = std::fs::symlink_metadata(&path).map_or(0, |m| m.len());
            if diff.len() > max_bytes || size > DIFF_LIMIT as u64 {
                continue;
            }
            let bytes = crate::repo::working_bytes(&path).unwrap_or_default();
            diff.push_str(&if bytes.iter().take(8 * 1024).any(|b| *b == 0) {
                new_binary_diff(&entry.path)
            } else {
                new_file_diff(&entry.path, &String::from_utf8_lossy(&bytes))
            });
        }
    }
    let recent = repo
        .run(&["log", "-n10", "--format=%s"])
        .map(|out| {
            String::from_utf8_lossy(&out)
                .lines()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok(Changes {
        files,
        diff,
        recent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_filled_in_one_pass() {
        // A value's own braces are never read: a diff can hold "{files}" or a format string.
        assert_eq!(
            fill("{a} and {b}, {c} {a", &[("a", "x{b}"), ("b", "y")]),
            "x{b} and y, {c} {a"
        );
        assert_eq!(fill("{}{{a}}", &[("a", "1")]), "{}{1}");
    }

    /// Templum's own rules for a good commit come first; the person's come last, after the diff,
    /// and win.
    #[test]
    fn the_default_request_has_templums_rules_then_the_persons() {
        let files = ["M src/a.rs".to_string()];
        let recent = ["feat(parser): read numbers".to_string()];
        let text = request(
            PROMPT,
            &Request {
                instructions: "Write the summary in Portuguese.",
                files: &files,
                diff: "+one\n",
                recent: &recent,
                max_bytes: 1024,
            },
        );
        let at = |what: &str| {
            text.find(what)
                .unwrap_or_else(|| panic!("{what:?} in {text}"))
        };
        let rules = [
            "not a conversation",
            "Conventional Commits form: type(scope): summary",
            "at most 72 characters",
            "imperative mood",
            "a blank line and a body",
            "no code fences, no quotes",
            "No credit to an AI",
            "this repository's last commits",
            "follow theirs.\n\n",
        ];
        let last_rule = rules.iter().map(|r| at(r)).max().unwrap();
        let theirs = at("Write the summary in Portuguese.");
        assert!(last_rule < at("feat(parser): read numbers") && at("+one") < theirs);
        assert!(text.ends_with("The person's own rules:\nWrite the summary in Portuguese.\n"));
    }

    #[test]
    fn a_request_has_every_file_and_says_when_its_diff_is_cut() {
        let files = ["M src/a.rs".to_string(), "A new.txt".to_string()];
        let recent = ["Fix the parser".to_string()];
        let diff = "+one\n+two\n+three\n";
        let r = |max_bytes| {
            request(
                "{instructions}|{files}|{recent_commits}|{truncated}|{diff}",
                &Request {
                    instructions: "Imperative.",
                    files: &files,
                    diff,
                    recent: &recent,
                    max_bytes,
                },
            )
        };
        assert_eq!(
            r(1024),
            "Imperative.|M src/a.rs\nA new.txt|Fix the parser||+one\n+two\n+three\n"
        );
        assert_eq!(
            r(12),
            "Imperative.|M src/a.rs\nA new.txt|Fix the parser|\
             The diff was cut: the list of files above is complete.|+one\n+two\n"
        );
        // Cut at a line's end, so never inside a character.
        let wide = "ééé\nééé\n";
        assert_eq!(cut(wide, 9), ("ééé\n", true));
        assert_eq!(cut(wide, 5), ("", true));
        assert_eq!(cut(wide, 14), (wide, false));
        // The default template has every placeholder.
        for name in [
            "instructions",
            "files",
            "diff",
            "recent_commits",
            "truncated",
        ] {
            assert!(PROMPT.contains(&format!("{{{name}}}")), "{name}");
        }
    }

    #[test]
    fn an_answer_splits_into_a_summary_and_a_description() {
        assert_eq!(
            split("Fix the parser\n\nIt read past the end.\nTwice.\n"),
            Some((
                "Fix the parser".to_string(),
                "It read past the end.\nTwice.".to_string()
            ))
        );
        assert_eq!(
            split("```\nFix it\r\n\r\nWhy.\r\n```\n"),
            Some(("Fix it".to_string(), "Why.".to_string())),
            "fences and CRLF"
        );
        assert_eq!(
            split("  Only a summary  \n"),
            Some(("Only a summary".to_string(), String::new()))
        );
        assert_eq!(
            split("Summary\nno blank line"),
            Some(("Summary".to_string(), "no blank line".to_string()))
        );
        assert_eq!(split(" \n```\n```\n"), None);
    }

    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use crate::Repo;

    fn never() -> AtomicBool {
        AtomicBool::new(false)
    }

    const LONG: Duration = Duration::from_secs(30);

    #[test]
    fn a_command_answers_on_its_standard_output_with_the_model_quoted() {
        let dir = tempfile::tempdir().unwrap();
        // sh and PowerShell both have echo; the quote must survive either's quoting.
        let answer = run(
            "echo {model}",
            "it's",
            "the request",
            dir.path(),
            LONG,
            &never(),
            &[],
        )
        .unwrap();
        assert_eq!(answer.trim(), "it's");
    }

    #[test]
    fn claude_code_is_run_without_its_own_commit_rules() {
        let dir = tempfile::tempdir().unwrap();
        let line = if cfg!(windows) {
            "echo $env:CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS"
        } else {
            "echo $CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS"
        };
        let answer = run(line, "m", "r", dir.path(), LONG, &never(), &[]).unwrap();
        assert_eq!(answer.trim(), "1");
    }

    #[test]
    fn an_accounts_variable_is_set_on_the_command() {
        let dir = tempfile::tempdir().unwrap();
        let line = if cfg!(windows) {
            "echo $env:CLAUDE_CONFIG_DIR"
        } else {
            "echo $CLAUDE_CONFIG_DIR"
        };
        let env = [("CLAUDE_CONFIG_DIR".to_string(), "/acc".to_string())];
        let answer = run(line, "m", "r", dir.path(), LONG, &never(), &env).unwrap();
        assert_eq!(answer.trim(), "/acc");
    }

    #[cfg(unix)]
    #[test]
    fn the_request_goes_to_the_standard_input_or_the_prompt_file() {
        let dir = tempfile::tempdir().unwrap();
        let answer = run("cat", "m", "the request\n", dir.path(), LONG, &never(), &[]).unwrap();
        assert_eq!(answer, "the request\n");
        let answer = run(
            "cat {prompt_file}",
            "m",
            "from a file\n",
            dir.path(),
            LONG,
            &never(),
            &[],
        );
        assert_eq!(answer.unwrap(), "from a file\n");
    }

    #[test]
    fn an_answer_written_to_the_output_file_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let answer = run(
            "echo 'Fix it' > {output_file}",
            "m",
            "r",
            dir.path(),
            LONG,
            &never(),
            &[],
        );
        assert_eq!(answer.unwrap().trim(), "Fix it");
        assert_eq!(
            decode(vec![0xFF, 0xFE, b'O', 0, b'K', 0]),
            "OK",
            "Windows PowerShell's > writes UTF-16"
        );
        assert_eq!(decode("\u{feff}OK".as_bytes().to_vec()), "OK");
    }

    #[test]
    fn a_failing_or_silent_command_says_how() {
        let dir = tempfile::tempdir().unwrap();
        let line = if cfg!(windows) {
            "exit 3"
        } else {
            "echo oops >&2; exit 3"
        };
        let err = run(line, "m", "r", dir.path(), LONG, &never(), &[]).unwrap_err();
        let Failure::Failed { code, stderr } = &err else {
            panic!("{err:?}")
        };
        assert_eq!(*code, Some(3));
        if cfg!(unix) {
            assert!(stderr.contains("oops"), "{stderr}");
            assert!(err.details().contains("oops"), "{}", err.details());
        }
        assert_eq!(err.to_string(), "The agent's command failed (exit code 3)");
        let err = run("exit 0", "m", "r", dir.path(), LONG, &never(), &[]).unwrap_err();
        assert!(matches!(err, Failure::Empty { .. }), "{err:?}");
        assert_eq!(err.to_string(), "The agent answered nothing");
    }

    #[test]
    fn a_command_that_hangs_is_stopped_at_the_timeout_or_on_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let short = Duration::from_millis(500);
        let err = run("sleep 20", "m", "r", dir.path(), short, &never(), &[]).unwrap_err();
        assert_eq!(err, Failure::TimedOut(short));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "stopped, not waited for"
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
        });
        let err = run("sleep 20", "m", "r", dir.path(), LONG, &cancel, &[]).unwrap_err();
        assert_eq!(err, Failure::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn the_changes_are_the_staged_ones_or_every_one() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo::init("git", dir.path()).unwrap();
        for args in [
            ["config", "user.name", "Ada"].as_slice(),
            &["config", "user.email", "ada@example.com"],
            &["config", "commit.gpgsign", "false"],
            &["config", "core.autocrlf", "false"],
        ] {
            repo.run(args).unwrap();
        }
        let write = |name: &str, text: &str| std::fs::write(dir.path().join(name), text).unwrap();
        // Before the first commit there is no HEAD: every change is new.
        write("a.txt", "one\n");
        let first = changes(&repo, true, 1024).unwrap();
        assert_eq!(first.files, ["A a.txt"]);
        assert!(first.diff.contains("+one"), "{}", first.diff);
        assert!(first.recent.is_empty());
        repo.run(&["add", "a.txt"]).unwrap();
        repo.run(&["commit", "-q", "-m", "First"]).unwrap();
        write("a.txt", "two\n");
        write("b.txt", "staged\n");
        repo.run(&["add", "b.txt"]).unwrap();
        write("new.txt", "brand new\n");
        let staged = changes(&repo, false, 1024).unwrap();
        assert_eq!(staged.files, ["A b.txt"]);
        assert!(
            staged.diff.contains("+staged") && !staged.diff.contains("+two"),
            "{}",
            staged.diff
        );
        assert_eq!(staged.recent, ["First"]);
        let all = changes(&repo, true, 1024).unwrap();
        assert_eq!(all.files, ["M a.txt", "A b.txt", "A new.txt"]);
        for part in ["+two", "+staged", "+brand new"] {
            assert!(all.diff.contains(part), "{part} in {}", all.diff);
        }
        // Past the cap the new files are listed, not read.
        let capped = changes(&repo, true, 1).unwrap();
        assert_eq!(capped.files, all.files);
        assert!(!capped.diff.contains("+brand new"), "{}", capped.diff);
    }

    #[cfg(unix)]
    #[test]
    fn the_request_is_kept_where_only_this_user_can_read_it() {
        let dir = tempfile::tempdir().unwrap();
        let line = "ls -ld \"$(dirname {prompt_file})\"; dirname {prompt_file}";
        let answer = run(line, "m", "r", dir.path(), LONG, &never(), &[]).unwrap();
        let (listing, folder) = answer.trim().split_once('\n').unwrap();
        assert!(listing.starts_with("drwx------"), "{listing}");
        assert!(!std::path::Path::new(folder).exists(), "{folder} removed");
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_left_holding_the_output_is_not_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let short = Duration::from_millis(500);
        // The shell ends at once; the sleep it left keeps its standard output open.
        let err = run(
            "sleep 20 & echo hi",
            "m",
            "r",
            dir.path(),
            short,
            &never(),
            &[],
        )
        .unwrap_err();
        assert_eq!(err, Failure::TimedOut(short));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "stopped, not waited for"
        );
    }
}
