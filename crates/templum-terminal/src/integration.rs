//! Shell integration: small scripts that make bash, zsh, fish and PowerShell tell Templum when a
//! command starts and ends and which folder they are in (see [`crate::tap`]). Like other
//! terminals, Templum loads them itself, around the user's own startup files.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// The scripts, by their path under the integration folder.
const FILES: [(&str, &str); 4] = [
    ("templum.bash", include_str!("integration/templum.bash")),
    ("zsh/.zshenv", include_str!("integration/zsh/.zshenv")),
    ("templum.fish", include_str!("integration/templum.fish")),
    ("templum.ps1", include_str!("integration/templum.ps1")),
];

/// Writes the scripts to `<dir>/shell-integration` (each only when it differs) and returns that
/// folder, for [`crate::SpawnOptions::integration`].
pub fn install(dir: &Path) -> io::Result<PathBuf> {
    let root = dir.join("shell-integration");
    for (name, text) in FILES {
        let path = root.join(name);
        if std::fs::read_to_string(&path).is_ok_and(|t| t == text) {
            continue;
        }
        let parent = path.parent().expect("a file under the folder");
        std::fs::create_dir_all(parent)?;
        // Beside it, then renamed over it: a shell starting meanwhile reads a whole one.
        let temp = parent.join(format!(".{}.{}", std::process::id(), file_name(&path)));
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, &path)?;
    }
    Ok(root)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// How to start a shell so that it loads the integration.
#[derive(Debug, PartialEq)]
pub(crate) struct Injection {
    /// The shell's new arguments; None: they stay as they are.
    pub args: Option<Vec<String>>,
    pub env: Vec<(String, String)>,
}

/// How to start `program` with `args` so that it loads the scripts in `dir`. None for other
/// programs, and when the arguments ask for something else: a command or a script to run, or
/// no startup files.
/// `zdotdir`: the ZDOTDIR the shell would get (zsh's startup files folder), put back by ours.
pub(crate) fn inject(
    program: &str,
    args: &[String],
    dir: &Path,
    zdotdir: Option<OsString>,
) -> Option<Injection> {
    let file = |name: &str| {
        let path = dir.join(name).to_string_lossy().into_owned();
        // Git Bash takes C:/… as well as C:\…, and \ is an escape in shells.
        if cfg!(windows) {
            path.replace('\\', "/")
        } else {
            path
        }
    };
    let stem = Path::new(program)
        .file_stem()?
        .to_string_lossy()
        .to_ascii_lowercase();
    let script = |a: &String| !a.starts_with('-');
    match stem.as_str() {
        "bash" if cfg!(windows) && wsl_bash(program) => None,
        "bash" => {
            // -c COMMAND, also in a group of short options (-lc).
            let short = |a: &String, o: char| {
                a.len() > 1 && a.starts_with('-') && !a.starts_with("--") && a.contains(o)
            };
            let other = [
                "--norc",
                "--noprofile",
                "--rcfile",
                "--init-file",
                "--posix",
            ];
            if args
                .iter()
                .any(|a| script(a) || short(a, 'c') || other.contains(&a.as_str()))
            {
                return None;
            }
            // A login shell ignores --init-file: ours reads the login files instead.
            let login = args.iter().any(|a| a == "--login" || short(a, 'l'));
            let mut new = vec!["--init-file".to_string(), file("templum.bash")];
            for a in args {
                if a == "--login" {
                    continue;
                }
                if short(a, 'l') {
                    let rest: String = a.chars().filter(|&c| c != 'l').collect();
                    if rest != "-" {
                        new.push(rest);
                    }
                    continue;
                }
                new.push(a.clone());
            }
            let env = if login {
                vec![("TEMPLUM_SHELL_LOGIN".to_string(), "1".to_string())]
            } else {
                Vec::new()
            };
            Some(Injection {
                args: Some(new),
                env,
            })
        }
        "zsh" => {
            let short =
                |a: &String, o: char| a.starts_with('-') && !a.starts_with("--") && a.contains(o);
            if args.iter().any(|a| {
                script(a)
                    || short(a, 'c')
                    || short(a, 'f')
                    || a == "--no-rcs"
                    || a == "--no-globalrcs"
            }) {
                return None;
            }
            let mut env = vec![("ZDOTDIR".to_string(), file("zsh"))];
            if let Some(zdotdir) = zdotdir {
                env.push((
                    "TEMPLUM_ZDOTDIR".to_string(),
                    zdotdir.to_string_lossy().into_owned(),
                ));
            }
            Some(Injection { args: None, env })
        }
        "fish" => {
            if args
                .iter()
                .any(|a| script(a) || a == "-c" || a.starts_with("--command"))
            {
                return None;
            }
            let quoted = file("templum.fish")
                .replace('\\', "\\\\")
                .replace('\'', "\\'");
            let mut new = args.to_vec();
            new.push("--init-command".to_string());
            new.push(format!("source '{quoted}'"));
            Some(Injection {
                args: Some(new),
                env: Vec::new(),
            })
        }
        "pwsh" | "powershell" => {
            let name = |a: &String| a.trim_start_matches('-').to_ascii_lowercase();
            let other = |n: &str| {
                !n.is_empty()
                    && ("command".starts_with(n)
                        || "file".starts_with(n)
                        || n == "e"
                        || n == "ec"
                        || (n.len() >= 2 && "encodedcommand".starts_with(n))
                        || (n.len() >= 4 && "noninteractive".starts_with(n)))
            };
            if args.iter().any(|a| script(a) || other(&name(a))) {
                return None;
            }
            // Read as a script block, not run as a file: no execution policy stops it.
            let path = dir
                .join("templum.ps1")
                .to_string_lossy()
                .replace('\'', "''");
            let mut new: Vec<String> = args
                .iter()
                .filter(|a| {
                    let n = name(a);
                    !(n.len() >= 3 && "noexit".starts_with(&n))
                })
                .cloned()
                .collect();
            new.extend([
                "-NoExit".to_string(),
                "-Command".to_string(),
                format!("try {{ . ([scriptblock]::Create([IO.File]::ReadAllText('{path}'))) }} catch {{}}"),
            ]);
            Some(Injection {
                args: Some(new),
                env: Vec::new(),
            })
        }
        _ => None,
    }
}

/// On Windows, `bash` found by name, or System32's, is WSL's: Linux would read our Windows path,
/// find nothing there, and skip the user's startup files with it.
fn wsl_bash(program: &str) -> bool {
    let p = program.to_ascii_lowercase().replace('/', "\\");
    !p.contains('\\') || p.contains("\\system32\\") || p.contains("\\windowsapps\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn dir() -> PathBuf {
        PathBuf::from("/data/shell-integration")
    }

    /// A file of `dir()` as the shells get it (on Windows, / instead of \).
    fn slashed(name: &str) -> String {
        let path = dir().join(name).to_string_lossy().into_owned();
        if cfg!(windows) {
            path.replace('\\', "/")
        } else {
            path
        }
    }

    fn new_args(program: &str, a: &[&str]) -> Option<Vec<String>> {
        inject(program, &args(a), &dir(), None).map(|i| i.args.unwrap_or_default())
    }

    #[test]
    fn bash_reads_ours_instead_of_bashrc() {
        let i = inject("/usr/bin/bash", &[], &dir(), None).unwrap();
        assert_eq!(
            i.args,
            Some(vec!["--init-file".to_string(), slashed("templum.bash")])
        );
        assert_eq!(i.env, vec![]);
    }

    #[test]
    fn a_login_bash_reads_the_login_files_through_ours() {
        // Git Bash's arguments on Windows.
        let i = inject(
            "C:/Program Files/Git/bin/bash.exe",
            &args(&["--login", "-i"]),
            &dir(),
            None,
        )
        .unwrap();
        assert_eq!(i.args.unwrap()[2..], args(&["-i"]));
        assert_eq!(i.env, vec![("TEMPLUM_SHELL_LOGIN".into(), "1".into())]);
        // A path: on Windows, `bash` alone is WSL's (see wsl_bash).
        assert_eq!(new_args("/bin/bash", &["-il"]).unwrap()[2..], args(&["-i"]));
        assert_eq!(new_args("/bin/bash", &["-l"]).unwrap().len(), 2);
    }

    #[test]
    fn bash_running_something_else_is_left_alone() {
        for a in [
            &["-c", "ls"][..],
            &["-lc", "ls"],
            &["--norc"],
            &["--rcfile", "x"],
            &["script.sh"],
        ] {
            assert_eq!(new_args("/bin/bash", a), None, "{a:?}");
        }
    }

    #[test]
    fn wsls_bash_isnt_git_bash() {
        assert!(wsl_bash("bash"));
        assert!(wsl_bash("bash.exe"));
        assert!(wsl_bash("C:\\Windows\\System32\\bash.exe"));
        assert!(wsl_bash(
            "C:/Users/u/AppData/Local/Microsoft/WindowsApps/bash.exe"
        ));
        assert!(!wsl_bash("C:\\Program Files\\Git\\bin\\bash.exe"));
        assert!(!wsl_bash("D:/tools/msys64/usr/bin/bash.exe"));
    }

    #[test]
    fn zsh_finds_ours_through_zdotdir_and_gets_the_users_back() {
        let i = inject(
            "zsh",
            &args(&["-l"]),
            &dir(),
            Some("/home/u/.config/zsh".into()),
        )
        .unwrap();
        assert_eq!(i.args, None);
        assert_eq!(
            i.env,
            vec![
                ("ZDOTDIR".into(), slashed("zsh")),
                ("TEMPLUM_ZDOTDIR".into(), "/home/u/.config/zsh".into()),
            ]
        );
        let i = inject("/bin/zsh", &[], &dir(), None).unwrap();
        assert_eq!(i.env.len(), 1, "no ZDOTDIR to put back");
        for a in [&["-c", "ls"][..], &["-f"], &["--no-rcs"], &["x.zsh"]] {
            assert_eq!(inject("zsh", &args(a), &dir(), None), None, "{a:?}");
        }
    }

    #[test]
    fn fish_sources_ours_after_its_config() {
        assert_eq!(
            new_args("/usr/bin/fish", &["-l"]),
            Some(vec![
                "-l".to_string(),
                "--init-command".to_string(),
                format!("source '{}'", slashed("templum.fish")),
            ])
        );
        let odd = inject("fish", &[], Path::new("/it's"), None).unwrap();
        assert!(odd.args.unwrap()[1].starts_with("source '/it\\'s"));
        assert_eq!(new_args("fish", &["-c", "ls"]), None);
        assert_eq!(new_args("fish", &["--command=ls"]), None);
    }

    #[test]
    fn powershell_runs_ours_and_stays() {
        let a = new_args("C:/Program Files/PowerShell/7/pwsh.exe", &["-NoLogo"]).unwrap();
        assert_eq!(a[..3], args(&["-NoLogo", "-NoExit", "-Command"]));
        let path = dir().join("templum.ps1").to_string_lossy().into_owned();
        assert_eq!(
            a[3],
            format!(
                "try {{ . ([scriptblock]::Create([IO.File]::ReadAllText('{path}'))) }} catch {{}}"
            )
        );
        assert_eq!(
            new_args("powershell", &["-NoExit"]).unwrap()[..2],
            args(&["-NoExit", "-Command"])
        );
        let odd = inject("pwsh", &[], Path::new("/it's"), None).unwrap();
        assert!(odd.args.unwrap()[2].contains("'/it''s"));
        for a in [
            &["-c", "ls"][..],
            &["-Command", "ls"],
            &["-File", "x.ps1"],
            &["-NonInteractive"],
            &["-ec", "x"],
            &["x.ps1"],
        ] {
            assert_eq!(new_args("pwsh", a), None, "{a:?}");
        }
    }

    #[test]
    fn other_programs_are_left_alone() {
        for p in ["nu", "cmd.exe", "wsl", "cat", ""] {
            assert_eq!(new_args(p, &[]), None, "{p}");
        }
    }

    /// Real shells in a PTY, each with its own home: only the startup file a test wrote is read,
    /// and it moves the shell to a folder. Skipped when the shell isn't installed.
    #[cfg(unix)]
    mod shells {
        use std::time::{Duration, Instant};

        use super::*;
        use crate::{Palette, SpawnOptions, TermOptions, Terminal};

        struct Home {
            dir: tempfile::TempDir,
            /// Where the startup file moves the shell.
            start: tempfile::TempDir,
        }

        impl Home {
            fn new() -> Self {
                Self {
                    dir: tempfile::tempdir().unwrap(),
                    start: tempfile::tempdir().unwrap(),
                }
            }

            fn write(&self, file: &str, text: String) {
                let path = self.dir.path().join(file);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }

            fn start(&self) -> String {
                self.start.path().display().to_string()
            }

            /// The folder of the account `claudinho` in this home, with a space in its path.
            fn account_folder(&self) -> String {
                let folder = self.dir.path().join("Templum Data/accounts/claudinho");
                folder.display().to_string()
            }

            /// What Templum gives a terminal for its accounts (`TEMPLUM_ACCOUNTS`): a line per
            /// account, tab-separated (id, program, variable, folder), whose folder it makes.
            fn accounts(&self) -> String {
                std::fs::create_dir_all(self.account_folder()).unwrap();
                format!(
                    "claudinho\tclaude\tCLAUDE_CONFIG_DIR\t{}",
                    self.account_folder()
                )
            }

            /// `program` started as Templum starts it, with HOME (and the config folder) here.
            fn spawn(&self, program: &str, a: &[&str]) -> Option<Terminal> {
                self.spawn_with(program, a, &[])
            }

            /// `spawn` with `env` in the shell's environment. The variables every Templum
            /// terminal gets for typed agents are empty unless `env` gives them: a `cargo test`
            /// run in a Templum terminal would hand the shell its own.
            fn spawn_with(
                &self,
                program: &str,
                a: &[&str],
                env: &[(&str, &str)],
            ) -> Option<Terminal> {
                let dirs = std::env::split_paths(&std::env::var_os("PATH")?).collect::<Vec<_>>();
                let Some(path) = crate::agent::find_program(program, &dirs) else {
                    eprintln!("skipped: {program} isn't installed");
                    return None;
                };
                let path = path.to_string_lossy().into_owned();
                let home = self.dir.path();
                let injection = inject(&path, &args(a), &install(home).unwrap(), None).unwrap();
                let mut command = vec![
                    format!("HOME={}", home.display()),
                    format!("XDG_CONFIG_HOME={}", home.join(".config").display()),
                ];
                for key in [
                    "TEMPLUM_CLAUDE_ARGS",
                    "TEMPLUM_CODEX_ARGS",
                    "TEMPLUM_COPILOT_ARGS",
                    "TEMPLUM_OPENCODE_CONFIG",
                    "TEMPLUM_KILO_CONFIG",
                    "TEMPLUM_QWEN_ARGS",
                    "TEMPLUM_QWEN_SETTINGS",
                    "TEMPLUM_PI_ARGS",
                    "TEMPLUM_ACCOUNTS",
                ] {
                    if !env.iter().any(|(k, _)| *k == key) {
                        command.push(format!("{key}="));
                    }
                }
                // The person's own variables for the agents: none, as outside Templum.
                for (_, var) in AGENT_VARS {
                    command.splice(0..0, ["-u".to_string(), var.to_string()]);
                }
                command.extend(env.iter().map(|(k, v)| format!("{k}={v}")));
                command.extend(injection.env.iter().map(|(k, v)| format!("{k}={v}")));
                command.push(path);
                command.extend(injection.args.unwrap_or_else(|| args(a)));
                let term = Terminal::spawn(
                    SpawnOptions {
                        cols: 100,
                        rows: 20,
                        cell_px: (8, 16),
                        command: Some(("env".to_string(), command)),
                        palette: Palette::default(),
                        options: TermOptions::default(),
                        working_directory: Some(home.to_path_buf()),
                        integration: None,
                        env: Vec::new(),
                    },
                    Arc::new(|| {}),
                )
                .unwrap();
                Some(term)
            }

            /// Agents in `<home>/bin` (`claude`, `codex`, `copilot`, `opencode`, `kilo`, `qwen`, `pi`) that write
            /// their arguments, one per line, to `<home>/<name>.args` (at once: moved there), and
            /// exit 3 (`$FAKE_EXIT` when set). What is piped into them, not typed, goes to
            /// `<home>/<name>.in` first; the variable an agent takes from its environment (`AGENT_VARS`)
            /// to `<home>/<name>.env`, `unset` when it has none. Returns a PATH with them first.
            fn fake_agents(&self) -> String {
                use std::os::unix::fs::PermissionsExt as _;
                let bin = self.dir.path().join("bin");
                for name in [
                    "claude", "codex", "copilot", "opencode", "kilo", "qwen", "pi",
                ] {
                    let args = self.dir.path().join(format!("{name}.args"));
                    let env = match AGENT_VARS.iter().find(|(n, _)| *n == name) {
                        Some((_, var)) => format!(
                            "printf '%s' \"${{{var}-unset}}\" > '{}'\n",
                            args.with_extension("env").display()
                        ),
                        None => String::new(),
                    };
                    self.write(
                        &format!("bin/{name}"),
                        format!(
                            "#!/bin/sh\n[ -t 0 ] || cat > '{1}'\n{env}\
                             for a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{0}.part'\n\
                             mv '{0}.part' '{0}'\nexit \"${{FAKE_EXIT:-3}}\"\n",
                            args.display(),
                            args.with_extension("in").display()
                        ),
                    );
                    let mode = std::fs::Permissions::from_mode(0o755);
                    std::fs::set_permissions(bin.join(name), mode).unwrap();
                }
                let path = std::env::var_os("PATH").unwrap_or_default();
                // Not a folder with the person's own `claudinho` (Templum's command for an account
                // of that name): the test's account deleted, it would run the fake `claude`.
                let theirs = std::env::split_paths(&path).filter(|d| !d.join("claudinho").exists());
                let dirs = std::iter::once(bin).chain(theirs);
                std::env::join_paths(dirs).unwrap().into_string().unwrap()
            }
        }

        /// What Templum gives a terminal for a `claude` typed in it, a path with a space included
        /// (macOS's `Application Support`).
        const CLAUDE_WORDS: &str = "--settings\n/tmp/Templum Data/claude-statusline.json\n--mcp-config=/tmp/Templum Data/mcp.json";
        /// What Templum gives a terminal for a `codex` typed in it (two of its flags).
        const CODEX_WORDS: &str = "-c\nmcp_servers.templum.command=\"/opt/templum\"\n-c\nhooks.Stop=[{hooks=[{type=\"command\",command=\"templum hook codex\",timeout=5}]}]";

        /// What Templum gives a terminal for a `copilot` typed in it; the last word names
        /// Templum's folder.
        const COPILOT_WORDS: &str = "--allow-all-mcp-server-instructions\n--additional-mcp-config=@/tmp/Templum Data/copilot/mcp.json\n--plugin-dir=/tmp/Templum Data/copilot/plugin";

        /// What Templum gives a terminal for an `opencode` or a `kilo` typed in it (a shorter
        /// one: quotes, a space and a `%`).
        const OPENCODE_CONFIG: &str =
            r#"{"plugin":["file:///tmp/Templum%20Data/opencode/templum.js"],"x":"a b"}"#;

        /// What Templum gives a terminal for a `qwen` typed in it: its words, and the settings
        /// file for its variable.
        const QWEN_WORDS: &str = "--mcp-config=/tmp/Templum Data/qwen/mcp.json";
        const QWEN_SETTINGS: &str = "/tmp/Templum Data/qwen/settings.json";
        /// What Templum gives a terminal for a `pi` typed in it.
        const PI_WORDS: &str = "-e\n/tmp/Templum Data/pi/templum.ts";

        /// The agents that take something of Templum's from a variable of their environment.
        const AGENT_VARS: [(&str, &str); 4] = [
            ("opencode", "OPENCODE_CONFIG_CONTENT"),
            ("kilo", "KILO_CONFIG_CONTENT"),
            ("qwen", "QWEN_CODE_SYSTEM_SETTINGS_PATH"),
            // An account's folder: the one `TEMPLUM_ACCOUNTS` names.
            ("claude", "CLAUDE_CONFIG_DIR"),
        ];

        /// The environment of a shell whose agents are `fake_agents`' on `path`, with Templum's
        /// words for each.
        fn with_agents<'a>(path: &'a str, accounts: &'a str) -> [(&'a str, &'a str); 10] {
            [
                ("PATH", path),
                ("TEMPLUM_ACCOUNTS", accounts),
                ("TEMPLUM_CLAUDE_ARGS", CLAUDE_WORDS),
                ("TEMPLUM_CODEX_ARGS", CODEX_WORDS),
                ("TEMPLUM_COPILOT_ARGS", COPILOT_WORDS),
                ("TEMPLUM_OPENCODE_CONFIG", OPENCODE_CONFIG),
                ("TEMPLUM_KILO_CONFIG", OPENCODE_CONFIG),
                ("TEMPLUM_QWEN_ARGS", QWEN_WORDS),
                ("TEMPLUM_QWEN_SETTINGS", QWEN_SETTINGS),
                ("TEMPLUM_PI_ARGS", PI_WORDS),
            ]
        }

        /// How a shell sets and unsets a variable of its environment, and runs a program without
        /// its functions: `{var}`, `{value}` and `{name}` stand for them.
        struct Syntax {
            set: &'static str,
            unset: &'static str,
            bare: &'static str,
        }

        const POSIX: Syntax = Syntax {
            set: "export {var}={value}",
            unset: "unset {var}",
            bare: "command {name}",
        };
        const FISH: Syntax = Syntax {
            set: "set -gx {var} {value}",
            unset: "set -e {var}",
            bare: "command {name}",
        };
        const PWSH: Syntax = Syntax {
            set: "$env:{var}='{value}'",
            unset: "Remove-Item Env:{var}",
            bare: "& (Get-Command {name} -CommandType Application | Select-Object -First 1)",
        };

        /// Waits until the fake `name` ran, and checks the variable it takes was `want` and its
        /// arguments `args`.
        fn wait_var(term: &Terminal, home: &Home, name: &str, want: &str, args: &[&str]) {
            wait_args(term, home, name, args);
            let file = home.dir.path().join(format!("{name}.env"));
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                want,
                "{name}'s variable"
            );
            std::fs::remove_file(&file).unwrap();
        }

        /// An `opencode` and a `kilo` typed at the prompt of a shell started `with_agents`: each
        /// gets its variable for that command only, never over the shell's own, its words as the
        /// person typed them, and its exit code is the command's (docs/specs/connections.md, Typed
        /// by hand: the shells).
        fn typed_with_variable(term: &Terminal, home: &Home, syntax: &Syntax) {
            for (name, var) in &AGENT_VARS[..2] {
                let fill = |template: &str, value: &str| {
                    template
                        .replace("{var}", var)
                        .replace("{value}", value)
                        .replace("{name}", name)
                };
                let finished = term.status().finished;
                term.write(&b"echo ready\r"[..]);
                wait(term, "echo to end", |t| {
                    t.status().finished > finished
                        && t.status().last.map(|o| o.code) == Some(Some(0))
                });
                term.write(format!("{name} run 'a b'\r").into_bytes());
                wait_var(term, home, name, OPENCODE_CONFIG, &["run", "a b"]);
                wait(term, &format!("{name}'s exit code"), |t| {
                    t.status().last.map(|o| o.code) == Some(Some(3))
                });
                // The shell itself never had it.
                term.write(format!("{}\r", fill(syntax.bare, "")).into_bytes());
                wait_var(term, home, name, "unset", &[]);
                // The person's own wins.
                term.write(format!("{}\r{name} x\r", fill(syntax.set, "mine")).into_bytes());
                wait_var(term, home, name, "mine", &["x"]);
                term.write(format!("{}\r", fill(syntax.unset, "")).into_bytes());
            }
        }

        /// A `qwen` typed at the prompt of a shell started `with_agents`: its word first, none
        /// when the person gives their own `--mcp-config` (it takes one) or starts with one of
        /// its commands (which refuse it), and its settings variable, never over the shell's own.
        fn typed_qwen(term: &Terminal, home: &Home, syntax: &Syntax) {
            let var = "QWEN_CODE_SYSTEM_SETTINGS_PATH";
            let lines = [
                ("qwen", vec![QWEN_WORDS]),
                ("qwen -p 'hi there'", vec![QWEN_WORDS, "-p", "hi there"]),
                // A prompt is a plain word too.
                ("qwen hi", vec![QWEN_WORDS, "hi"]),
                ("qwen mcp list", vec!["mcp", "list"]),
                (
                    "qwen --mcp-config=mine.json",
                    vec!["--mcp-config=mine.json"],
                ),
                (
                    "qwen --mcp-config mine.json",
                    vec!["--mcp-config", "mine.json"],
                ),
            ];
            for (line, want) in lines {
                term.write(format!("{line}\r").into_bytes());
                wait_var(term, home, "qwen", QWEN_SETTINGS, &want);
            }
            let set = syntax.set.replace("{var}", var).replace("{value}", "mine");
            term.write(format!("{set}\rqwen\r").into_bytes());
            wait_var(term, home, "qwen", "mine", &[QWEN_WORDS]);
            term.write(format!("{}\r", syntax.unset.replace("{var}", var)).into_bytes());
        }

        /// Waits until the fake `name` ran, and checks it got `want`, then deletes what it wrote.
        fn wait_args(term: &Terminal, home: &Home, name: &str, want: &[&str]) {
            let file = home.dir.path().join(format!("{name}.args"));
            let text: String = want.iter().map(|w| format!("{w}\n")).collect();
            // Moved there whole: once there, it is all the fake got.
            wait(term, &format!("{name} to run"), |_| {
                std::fs::read_to_string(&file).is_ok_and(|got| {
                    assert_eq!(got, text, "{name}'s arguments");
                    true
                })
            });
            std::fs::remove_file(&file).unwrap();
        }

        /// Once the startup file's folder is reported, types `line`, which runs the fake `name`,
        /// pressing Enter again until it ran: PowerShell can drop the first Enter.
        fn type_first(term: &Terminal, home: &Home, line: &str, name: &str) {
            wait(term, "the startup file's folder", |t| {
                same(t.processes().reported, home.start.path())
            });
            term.write(format!("{line}\r").into_bytes());
            let file = home.dir.path().join(format!("{name}.args"));
            wait_pressing(term, "the first line to run", Some(b"\r"), |_| {
                file.exists()
            });
        }

        /// An account's command (`claudinho`) typed at the prompt of a shell started `with_agents`,
        /// before anything else: its agent runs with Templum's words, then the person's, and the
        /// account's folder in its variable; the shell doesn't keep the variable, so a `claude`
        /// typed next has none (docs/specs/accounts.md, Typed in a terminal).
        fn typed_account(term: &Terminal, home: &Home) {
            let claude: Vec<&str> = CLAUDE_WORDS.lines().collect();
            type_first(term, home, "claudinho hi", "claude");
            wait_var(
                term,
                home,
                "claude",
                &home.account_folder(),
                &[&claude[..], &["hi"]].concat(),
            );
            term.write(&b"claude\r"[..]);
            wait_var(term, home, "claude", "unset", &claude);
        }

        /// After `typed_account`: the account is deleted (its folder is gone) while this terminal
        /// stays open, and `claudinho` typed now doesn't run its agent: that would make the folder
        /// again, with a login no row manages. With `status`, the command is waited for.
        fn typed_deleted_account(term: &Terminal, home: &Home, status: bool) {
            std::fs::remove_dir_all(home.account_folder()).unwrap();
            let finished = term.status().finished;
            term.write(&b"claudinho hi\r"[..]);
            if status {
                wait(term, "claudinho to end", |t| t.status().finished > finished);
            } else {
                std::thread::sleep(Duration::from_millis(800));
            }
            let file = home.dir.path().join("claude.args");
            assert!(!file.exists(), "a deleted account's agent was run");
        }

        /// A `claude` and a `codex` typed at the prompt of a shell started `with_agents`: Templum's
        /// words come first, an empty argument stays, none comes twice, and none once `unset`
        /// empties the variable (docs/specs/connections.md, Agents typed in a terminal). With
        /// `status`, the agent's exit code is the command's.
        fn typed_agents(term: &Terminal, home: &Home, unset: &str, status: bool) {
            let claude: Vec<&str> = CLAUDE_WORDS.lines().collect();
            let codex: Vec<&str> = CODEX_WORDS.lines().collect();
            type_first(term, home, "claude", "claude");
            wait_args(term, home, "claude", &claude);
            if status {
                wait(term, "claude's exit code", |t| {
                    t.status().last.map(|o| o.code) == Some(Some(3))
                });
            }
            let lines = [
                (
                    "claude hi \"\"",
                    "claude",
                    [&claude[..], &["hi", ""]].concat(),
                ),
                ("claude --settings x", "claude", vec!["--settings", "x"]),
                (
                    "claude --mcp-config=y hi",
                    "claude",
                    vec!["--mcp-config=y", "hi"],
                ),
                // PowerShell makes an array of a,b: passed on as one word, as to a program.
                (
                    "claude --x a,b",
                    "claude",
                    [&claude[..], &["--x", "a,b"]].concat(),
                ),
                (
                    "codex resume --last",
                    "codex",
                    [&codex[..], &["resume", "--last"]].concat(),
                ),
                (
                    "codex -c 'mcp_servers.templum.command=\"z\"'",
                    "codex",
                    vec!["-c", "mcp_servers.templum.command=\"z\""],
                ),
            ];
            for (line, name, want) in lines {
                term.write(format!("{line}\r").into_bytes());
                wait_args(term, home, name, &want);
            }
            term.write(format!("{unset}\rclaude hi\r").into_bytes());
            wait_args(term, home, "claude", &["hi"]);
        }

        /// A `copilot` typed at the prompt of a shell started `with_agents`, after `typed_agents`:
        /// Templum's words first, none when the person's words hold the last of them, start
        /// with a subcommand or ask for the help or the version (AWS's `copilot`), none once
        /// `unset` empties the variable (docs/specs/connections.md, Typed by hand: the shells).
        /// With `status`, its exit code is the command's.
        fn typed_copilot(term: &Terminal, home: &Home, unset: &str, status: bool) {
            let words: Vec<&str> = COPILOT_WORDS.lines().collect();
            if status {
                // The last command (an agent's) exited 3 too: one that exits 0 first.
                let finished = term.status().finished;
                term.write(&b"echo ready\r"[..]);
                wait(term, "echo to end", |t| {
                    t.status().finished > finished
                        && t.status().last.map(|o| o.code) == Some(Some(0))
                });
            }
            term.write(&b"copilot\r"[..]);
            wait_args(term, home, "copilot", &words);
            if status {
                wait(term, "copilot's exit code", |t| {
                    t.status().last.map(|o| o.code) == Some(Some(3))
                });
            }
            let mine = words[2];
            let lines = [
                (
                    "copilot -p 'hi there'".to_string(),
                    [&words[..], &["-p", "hi there"]].concat(),
                ),
                (
                    "copilot --plugin-dir=/mine".to_string(),
                    [&words[..], &["--plugin-dir=/mine"]].concat(),
                ),
                (
                    "copilot --model gpt-5.4".to_string(),
                    [&words[..], &["--model", "gpt-5.4"]].concat(),
                ),
                (format!("copilot '{mine}'"), vec![mine]),
                ("copilot plugin list".to_string(), vec!["plugin", "list"]),
                ("copilot app deploy".to_string(), vec!["app", "deploy"]),
                ("copilot --help".to_string(), vec!["--help"]),
                ("copilot -h".to_string(), vec!["-h"]),
                ("copilot -v".to_string(), vec!["-v"]),
                (
                    "copilot --model gpt-5.4 --version".to_string(),
                    vec!["--model", "gpt-5.4", "--version"],
                ),
            ];
            for (line, want) in lines {
                term.write(format!("{line}\r").into_bytes());
                wait_args(term, home, "copilot", &want);
            }
            term.write(format!("{unset}\rcopilot x\r").into_bytes());
            wait_args(term, home, "copilot", &["x"]);
        }

        /// A `pi` typed at the prompt of a shell started `with_agents`: Templum's words first,
        /// none when the person's words hold its file or start with one of Pi's commands (which
        /// it reads only as the first word), none once `unset` empties the variable.
        fn typed_pi(term: &Terminal, home: &Home, unset: &str) {
            let words: Vec<&str> = PI_WORDS.lines().collect();
            let file = words[1];
            let lines = [
                ("pi".to_string(), words.clone()),
                (
                    "pi -p 'hi there'".to_string(),
                    [&words[..], &["-p", "hi there"]].concat(),
                ),
                (
                    "pi -e mine.ts".to_string(),
                    [&words[..], &["-e", "mine.ts"]].concat(),
                ),
                (format!("pi -e '{file}'"), vec!["-e", file]),
                ("pi list".to_string(), vec!["list"]),
                ("pi install npm:x".to_string(), vec!["install", "npm:x"]),
            ];
            for (line, want) in lines {
                term.write(format!("{line}\r").into_bytes());
                wait_args(term, home, "pi", &want);
            }
            term.write(format!("{unset}\rpi x\r").into_bytes());
            wait_args(term, home, "pi", &["x"]);
        }

        use std::sync::Arc;

        fn same(a: Option<PathBuf>, b: &Path) -> bool {
            a.and_then(|a| a.canonicalize().ok()) == b.canonicalize().ok()
        }

        fn wait(term: &Terminal, what: &str, done: impl Fn(&Terminal) -> bool) {
            wait_pressing(term, what, None, done)
        }

        /// `wait`, typing `again` every 3 s until `done`. PowerShell (.NET) can drop an Enter
        /// typed while it switches the terminal to raw mode for its first prompt: the line stays
        /// typed and never runs. An empty line runs nothing, in every shell tested here.
        fn wait_pressing(
            term: &Terminal,
            what: &str,
            again: Option<&'static [u8]>,
            done: impl Fn(&Terminal) -> bool,
        ) {
            let deadline = Instant::now() + Duration::from_secs(30);
            let mut next = Instant::now() + Duration::from_secs(3);
            while !done(term) {
                if let Some(bytes) = again
                    && Instant::now() > next
                {
                    term.write(bytes);
                    next = Instant::now() + Duration::from_secs(3);
                }
                if Instant::now() > deadline {
                    let mut screen = crate::Snapshot::default();
                    screen.refresh(term.shared(), &Palette::default());
                    panic!(
                        "waited for {what}: {:?}, screen:\n{}",
                        term.status(),
                        screen.text()
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        /// The startup file's folder is reported; with `commands`, a failing command and a `cd`
        /// are reported as they end.
        fn check(term: Terminal, home: &Home, commands: bool) {
            wait(&term, "the startup file's folder", |t| {
                same(t.processes().reported, home.start.path())
            });
            if commands {
                term.write(&b"false\r"[..]);
                wait_pressing(&term, "false to end", Some(b"\r"), |t| {
                    let s = t.status();
                    s.finished == 1 && s.last.map(|o| o.code) == Some(Some(1))
                });
                // Its prompt's line is marked with how it ended, and is a prompt to jump to.
                wait(&term, "false's mark", |t| {
                    let mut screen = crate::Snapshot::default();
                    screen.refresh(t.shared(), &Palette::default());
                    screen.rows.iter().any(|r| {
                        r.text.contains("false") && r.mark == Some(crate::Mark::Ended(Some(1)))
                    })
                });
                for _ in 0..30 {
                    term.write(&b"\r"[..]);
                }
                wait(&term, "a prompt above", |t| t.jump_to_prompt(true));
            }
            let then = tempfile::tempdir().unwrap();
            term.write(format!("cd '{}'\r", then.path().display()).into_bytes());
            wait(&term, "cd's folder", |t| {
                same(t.processes().reported, then.path())
            });
            if commands {
                wait(&term, "cd to end", |t| {
                    let s = t.status();
                    s.finished == 2
                        && s.last.map(|o| o.code) == Some(Some(0))
                        && s.running.is_none()
                });
            }
        }

        /// Bash 4.4 brought PS0, which marks a command's start (macOS has 3.2).
        fn bash_has_ps0() -> bool {
            std::process::Command::new("bash")
                .args([
                    "-c",
                    "(( BASH_VERSINFO[0] * 100 + BASH_VERSINFO[1] >= 404 ))",
                ])
                .status()
                .is_ok_and(|s| s.success())
        }

        #[test]
        fn bash() {
            let home = Home::new();
            home.write(".bashrc", format!("cd '{}'\n", home.start()));
            if let Some(term) = home.spawn("bash", &[]) {
                check(term, &home, bash_has_ps0());
            }
        }

        #[test]
        fn bash_as_a_login_shell() {
            let home = Home::new();
            home.write(".bash_profile", format!("cd '{}'\n", home.start()));
            if let Some(term) = home.spawn("bash", &["--login", "-i"]) {
                check(term, &home, bash_has_ps0());
            }
        }

        /// ble.sh, a line editor for bash, attaches after the startup files, setting
        /// PROMPT_COMMAND back as it was.
        #[test]
        fn bash_with_ble_sh() {
            let home = Home::new();
            let found = ["/usr/share/blesh/ble.sh", "/usr/local/share/blesh/ble.sh"]
                .into_iter()
                .find(|p| Path::new(p).is_file());
            let Some(ble) = found else {
                eprintln!("skipped: ble.sh isn't installed");
                return;
            };
            home.write(
                ".bashrc",
                format!(
                    "source {ble} --noattach\ncd '{}'\nble-attach\n",
                    home.start()
                ),
            );
            if let Some(term) = home.spawn("bash", &[]) {
                check(term, &home, bash_has_ps0());
            }
        }

        /// A zsh outside /usr (unpacked for a test run) finds its line editor through this: zsh
        /// doesn't take MODULE_PATH from the environment.
        fn zsh_modules(home: &Home) {
            if let Ok(modules) = std::env::var("MODULE_PATH") {
                home.write(".zshenv", format!("module_path=('{modules}')\n"));
            }
        }

        #[test]
        fn zsh() {
            let home = Home::new();
            home.write(".zshrc", format!("cd '{}'\n", home.start()));
            zsh_modules(&home);
            if let Some(term) = home.spawn("zsh", &[]) {
                check(term, &home, true);
            }
        }

        #[test]
        fn bash_gives_typed_agents_templums_words() {
            let home = Home::new();
            home.write(".bashrc", format!("cd '{}'\n", home.start()));
            let path = home.fake_agents();
            let accounts = home.accounts();
            if let Some(term) = home.spawn_with("bash", &[], &with_agents(&path, &accounts)) {
                typed_account(&term, &home);
                typed_deleted_account(&term, &home, bash_has_ps0());
                typed_agents(&term, &home, "unset TEMPLUM_CLAUDE_ARGS", bash_has_ps0());
                typed_copilot(&term, &home, "unset TEMPLUM_COPILOT_ARGS", bash_has_ps0());
                if bash_has_ps0() {
                    typed_with_variable(&term, &home, &POSIX);
                }
                typed_qwen(&term, &home, &POSIX);
                typed_pi(&term, &home, "unset TEMPLUM_PI_ARGS");
            }
        }

        /// The person's own function of an account's name, read before Templum defines its own,
        /// runs instead (bash; the others check for one the same way).
        #[test]
        fn bash_keeps_the_persons_account_function() {
            let home = Home::new();
            home.write(
                ".bashrc",
                format!(
                    "claudinho() {{ command claude own; }}\ncd '{}'\n",
                    home.start()
                ),
            );
            let path = home.fake_agents();
            let accounts = home.accounts();
            let Some(term) = home.spawn_with("bash", &[], &with_agents(&path, &accounts)) else {
                return;
            };
            type_first(&term, &home, "claudinho hi", "claude");
            wait_args(&term, &home, "claude", &["own"]);
        }

        /// `claude` is installed after a command made zsh read its folder (npm's with nvm, its
        /// own installer's ~/.local/bin): found all the same, as by `command`.
        #[test]
        fn zsh_gives_typed_agents_templums_words() {
            let home = Home::new();
            home.write(".zshrc", format!("cd '{}'\n", home.start()));
            zsh_modules(&home);
            home.fake_agents();
            let bin = home.dir.path().join("bin");
            std::fs::remove_file(bin.join("claude")).unwrap();
            // No other `claude` further on, which zsh would run instead.
            let path = format!("{}:/usr/bin:/bin", bin.display());
            let accounts = home.accounts();
            let Some(term) = home.spawn_with("zsh", &[], &with_agents(&path, &accounts)) else {
                return;
            };
            wait(&term, "the startup file's folder", |t| {
                same(t.processes().reported, home.start.path())
            });
            term.write(&b"ls\r"[..]);
            wait(&term, "ls to end", |t| t.status().finished == 1);
            home.fake_agents();
            typed_account(&term, &home);
            typed_deleted_account(&term, &home, true);
            typed_agents(&term, &home, "unset TEMPLUM_CLAUDE_ARGS", true);
            typed_copilot(&term, &home, "unset TEMPLUM_COPILOT_ARGS", true);
            typed_with_variable(&term, &home, &POSIX);
            typed_qwen(&term, &home, &POSIX);
            typed_pi(&term, &home, "unset TEMPLUM_PI_ARGS");
        }

        /// A `}` is only a brace in command position when the person's .zshenv sets
        /// `ignore_close_braces` or `ignore_braces`: ours has none outside it, so the whole file
        /// still parses and the account is a command.
        #[test]
        fn zsh_with_ignore_braces_still_has_the_accounts() {
            for option in ["ignore_close_braces", "ignore_braces"] {
                let home = Home::new();
                home.write(".zshrc", format!("cd '{}'\n", home.start()));
                let modules = std::env::var("MODULE_PATH")
                    .map(|m| format!("module_path=('{m}')\n"))
                    .unwrap_or_default();
                home.write(".zshenv", format!("{modules}setopt {option}\n"));
                let path = home.fake_agents();
                let accounts = home.accounts();
                let Some(term) = home.spawn_with("zsh", &[], &with_agents(&path, &accounts)) else {
                    return;
                };
                typed_account(&term, &home);
            }
        }

        /// The person's alias and function, read before Templum defines its own (bash) or after
        /// (zsh's .zshrc): the alias adds its words after Templum's; the function runs instead.
        fn keeps_the_persons_alias_and_function(program: &str, file: &str) {
            let home = Home::new();
            home.write(
                file,
                format!(
                    "alias claude='claude --model opus'\n\
                     codex() {{ command codex mine \"$@\"; }}\n\
                     cd '{}'\n",
                    home.start()
                ),
            );
            if program == "zsh" {
                zsh_modules(&home);
            }
            let path = home.fake_agents();
            let Some(term) = home.spawn_with(program, &[], &with_agents(&path, "")) else {
                return;
            };
            type_first(&term, &home, "claude hi", "claude");
            let claude: Vec<&str> = CLAUDE_WORDS.lines().collect();
            wait_args(
                &term,
                &home,
                "claude",
                &[&claude[..], &["--model", "opus", "hi"]].concat(),
            );
            term.write(&b"codex hi\r"[..]);
            wait_args(&term, &home, "codex", &["mine", "hi"]);
        }

        #[test]
        fn bash_keeps_the_persons_alias_and_function() {
            keeps_the_persons_alias_and_function("bash", ".bashrc");
        }

        #[test]
        fn zsh_keeps_the_persons_alias_and_function() {
            keeps_the_persons_alias_and_function("zsh", ".zshrc");
        }

        /// `set -e` in the `.bashrc`: the shell stays, and the agent runs.
        #[test]
        fn bash_with_errexit_runs_the_agent() {
            let home = Home::new();
            home.write(
                ".bashrc",
                format!("set -e\nexport FAKE_EXIT=0\ncd '{}'\n", home.start()),
            );
            let path = home.fake_agents();
            let Some(term) = home.spawn_with("bash", &[], &with_agents(&path, "")) else {
                return;
            };
            type_first(&term, &home, "claude hi", "claude");
            let claude: Vec<&str> = CLAUDE_WORDS.lines().collect();
            wait_args(&term, &home, "claude", &[&claude[..], &["hi"]].concat());
            term.write(&b"codex hi\r"[..]);
            let codex: Vec<&str> = CODEX_WORDS.lines().collect();
            wait_args(&term, &home, "codex", &[&codex[..], &["hi"]].concat());
        }

        /// A `claude` typed where none is installed: the shell's own complaint, nothing of
        /// Templum's on the screen, and a failed command.
        #[test]
        fn a_missing_agent_is_told_as_without_templum() {
            let shells: [(&str, &[&str], &str, &str, bool); 5] = [
                ("bash", &[], ".bashrc", "cd", bash_has_ps0()),
                ("zsh", &[], ".zshrc", "cd", true),
                ("fish", &[], ".config/fish/config.fish", "cd", true),
                // fish before 3.2 has no fish_command_not_found.
                (
                    "fish",
                    &[],
                    ".config/fish/config.fish",
                    "functions -e fish_command_not_found; cd",
                    true,
                ),
                (
                    "pwsh",
                    &["-NoLogo"],
                    ".config/powershell/Microsoft.PowerShell_profile.ps1",
                    "Set-Location",
                    true,
                ),
            ];
            for (program, a, file, cd, status) in shells {
                let home = Home::new();
                home.write(file, format!("{cd} '{}'\n", home.start()));
                if program == "zsh" {
                    zsh_modules(&home);
                }
                // No `claude` there, nor from mise's fish config, which puts its tools back.
                let env = [
                    ("PATH", "/usr/bin:/bin"),
                    ("MISE_FISH_AUTO_ACTIVATE", "0"),
                    ("TEMPLUM_CLAUDE_ARGS", CLAUDE_WORDS),
                    ("TEMPLUM_CODEX_ARGS", CODEX_WORDS),
                ];
                let Some(term) = home.spawn_with(program, a, &env) else {
                    continue;
                };
                wait(&term, "the startup file's folder", |t| {
                    same(t.processes().reported, home.start.path())
                });
                term.write(&b"claude\r"[..]);
                let screen = || {
                    let mut screen = crate::Snapshot::default();
                    screen.refresh(term.shared(), &Palette::default());
                    screen.text()
                };
                if status {
                    wait_pressing(&term, "claude to fail", Some(b"\r"), |t| {
                        let s = t.status();
                        s.finished >= 1 && !matches!(s.last.map(|o| o.code), Some(Some(0)))
                    });
                } else {
                    wait(&term, "claude to be told", |_| {
                        screen().matches("claude").count() > 1
                    });
                }
                let shown = screen();
                assert!(!shown.contains("emplum"), "{program}:\n{shown}");
            }
        }

        /// Started without Templum's words, the shell has no `claude` of Templum's: words set
        /// later reach no one.
        #[test]
        fn bash_without_the_variables_has_no_function() {
            let home = Home::new();
            home.write(".bashrc", format!("cd '{}'\n", home.start()));
            let path = home.fake_agents();
            let Some(term) = home.spawn_with("bash", &[], &[("PATH", &path)]) else {
                return;
            };
            type_first(
                &term,
                &home,
                "export TEMPLUM_CLAUDE_ARGS=--x TEMPLUM_CODEX_ARGS=--y; claude hi",
                "claude",
            );
            wait_args(&term, &home, "claude", &["hi"]);
            term.write(&b"codex hi\r"[..]);
            wait_args(&term, &home, "codex", &["hi"]);
        }

        #[test]
        fn fish() {
            let home = Home::new();
            home.write(
                ".config/fish/config.fish",
                format!("cd '{}'\n", home.start()),
            );
            if let Some(term) = home.spawn("fish", &[]) {
                check(term, &home, true);
            }
        }

        #[test]
        fn fish_gives_typed_agents_templums_words() {
            let home = Home::new();
            home.write(
                ".config/fish/config.fish",
                format!("cd '{}'\n", home.start()),
            );
            let path = home.fake_agents();
            let accounts = home.accounts();
            if let Some(term) = home.spawn_with("fish", &[], &with_agents(&path, &accounts)) {
                typed_account(&term, &home);
                typed_deleted_account(&term, &home, true);
                typed_agents(&term, &home, "set -e TEMPLUM_CLAUDE_ARGS", true);
                typed_copilot(&term, &home, "set -e TEMPLUM_COPILOT_ARGS", true);
                typed_with_variable(&term, &home, &FISH);
                typed_qwen(&term, &home, &FISH);
                typed_pi(&term, &home, "set -e TEMPLUM_PI_ARGS");
            }
        }

        /// fish's `alias` makes a function: the person's, which runs instead of Templum's.
        #[test]
        fn fish_keeps_the_persons_function() {
            let home = Home::new();
            home.write(
                ".config/fish/config.fish",
                format!(
                    "function codex; command codex mine $argv; end\n\
                     alias claude 'claude --model opus'\n\
                     cd '{}'\n",
                    home.start()
                ),
            );
            let path = home.fake_agents();
            let Some(term) = home.spawn_with("fish", &[], &with_agents(&path, "")) else {
                return;
            };
            type_first(&term, &home, "codex hi", "codex");
            wait_args(&term, &home, "codex", &["mine", "hi"]);
            term.write(&b"claude hi\r"[..]);
            wait_args(&term, &home, "claude", &["--model", "opus", "hi"]);
        }

        /// The agent's exit code reaches the prompt's mark, though a function's caller sees `$?`
        /// true.
        #[test]
        fn powershell_gives_typed_agents_templums_words() {
            let home = Home::new();
            home.write(
                ".config/powershell/Microsoft.PowerShell_profile.ps1",
                format!("Set-Location '{}'\n", home.start()),
            );
            let path = home.fake_agents();
            let accounts = home.accounts();
            let Some(term) = home.spawn_with("pwsh", &["-NoLogo"], &with_agents(&path, &accounts))
            else {
                return;
            };
            typed_account(&term, &home);
            typed_deleted_account(&term, &home, true);
            typed_agents(&term, &home, "Remove-Item Env:TEMPLUM_CLAUDE_ARGS", true);
            typed_copilot(&term, &home, "Remove-Item Env:TEMPLUM_COPILOT_ARGS", true);
            typed_with_variable(&term, &home, &PWSH);
            typed_qwen(&term, &home, &PWSH);
            typed_pi(&term, &home, "Remove-Item Env:TEMPLUM_PI_ARGS");
            // What is piped into an agent reaches it, as `git diff | claude -p review`; one not
            // piped into keeps the terminal.
            let piped = |name: &str| {
                let file = home.dir.path().join(format!("{name}.in"));
                std::fs::read_to_string(file).unwrap_or_default()
            };
            // Each line is typed at the prompt, once the command before it ended, and Enter is
            // pressed again until it ran: PowerShell can drop what is typed while it gets its next
            // prompt ready, and an Enter while it starts reading. `pi x` ends with its fake's 3.
            let pipe = |line: &str, name: &str| {
                let ended = term.status().finished;
                term.write(format!("{line}\r").into_bytes());
                let file = home.dir.path().join(format!("{name}.args"));
                wait_pressing(&term, line, Some(b"\r"), |_| file.exists());
                ended
            };
            assert!(!home.dir.path().join("claude.in").exists());
            wait(&term, "pi to end", |t| {
                let s = t.status();
                s.running.is_none() && s.last.map(|o| o.code) == Some(Some(3))
            });
            let ended = pipe("'x' | claude -p review", "claude");
            wait_args(&term, &home, "claude", &["-p", "review"]);
            assert_eq!(piped("claude"), "x\n");
            wait(&term, "claude to end", |t| t.status().finished > ended);
            pipe("'y' | codex exec -", "codex");
            let codex: Vec<&str> = CODEX_WORDS.lines().collect();
            wait_args(
                &term,
                &home,
                "codex",
                &[&codex[..], &["exec", "-"]].concat(),
            );
            assert_eq!(piped("codex"), "y\n");
        }

        /// In PowerShell an alias comes before any function: the person's wins too.
        #[test]
        fn powershell_keeps_the_persons_function_and_alias() {
            let home = Home::new();
            home.write(
                ".config/powershell/Microsoft.PowerShell_profile.ps1",
                format!(
                    "function codex {{ & (Get-Command codex -CommandType Application | \
                     Select-Object -First 1) mine @args }}\n\
                     Set-Alias claude '{}'\n\
                     Set-Location '{}'\n",
                    home.dir.path().join("bin").join("claude").display(),
                    home.start()
                ),
            );
            let path = home.fake_agents();
            let Some(term) = home.spawn_with("pwsh", &["-NoLogo"], &with_agents(&path, "")) else {
                return;
            };
            type_first(&term, &home, "codex hi", "codex");
            wait_args(&term, &home, "codex", &["mine", "hi"]);
            term.write(&b"claude hi\r"[..]);
            wait_args(&term, &home, "claude", &["hi"]);
        }

        #[test]
        fn powershell() {
            let home = Home::new();
            home.write(
                ".config/powershell/Microsoft.PowerShell_profile.ps1",
                format!("Set-Location '{}'\n", home.start()),
            );
            if let Some(term) = home.spawn("pwsh", &["-NoLogo"]) {
                check(term, &home, true);
            }
        }
    }

    #[test]
    fn install_writes_each_script_once() {
        let d = tempfile::tempdir().unwrap();
        let root = install(d.path()).unwrap();
        assert_eq!(root, d.path().join("shell-integration"));
        for (name, text) in FILES {
            assert_eq!(std::fs::read_to_string(root.join(name)).unwrap(), text);
        }
        let bash = root.join("templum.bash");
        std::fs::write(&bash, "old").unwrap();
        let before = std::fs::metadata(root.join("templum.fish"))
            .unwrap()
            .modified()
            .unwrap();
        install(d.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(&bash).unwrap(),
            FILES[0].1,
            "rewritten"
        );
        let after = std::fs::metadata(root.join("templum.fish"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after, "the same file isn't written again");
        let names: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 4, "no temporary file left: {names:?}");
    }
}
