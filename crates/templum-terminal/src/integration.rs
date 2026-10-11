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

            /// `program` started as Templum starts it, with HOME (and the config folder) here.
            fn spawn(&self, program: &str, a: &[&str]) -> Option<Terminal> {
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
                    },
                    Arc::new(|| {}),
                )
                .unwrap();
                Some(term)
            }
        }

        use std::sync::Arc;

        fn same(a: Option<PathBuf>, b: &Path) -> bool {
            a.and_then(|a| a.canonicalize().ok()) == b.canonicalize().ok()
        }

        fn wait(term: &Terminal, what: &str, done: impl Fn(&Terminal) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !done(term) {
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
                wait(&term, "false to end", |t| {
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

        #[test]
        fn zsh() {
            let home = Home::new();
            home.write(".zshrc", format!("cd '{}'\n", home.start()));
            // A zsh outside /usr (unpacked for a test run) finds its line editor through this:
            // zsh doesn't take MODULE_PATH from the environment.
            if let Ok(modules) = std::env::var("MODULE_PATH") {
                home.write(".zshenv", format!("module_path=('{modules}')\n"));
            }
            if let Some(term) = home.spawn("zsh", &[]) {
                check(term, &home, true);
            }
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
