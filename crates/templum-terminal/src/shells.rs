//! The shells a terminal can run: the ones found on this computer, the preferred first, and the
//! one a terminal runs when the config names none. See docs/specs/shells.md.

use std::path::{Path, PathBuf};

/// A shell to run, as the settings list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellOption {
    /// As menus show it: "Git Bash", "PowerShell 7", "zsh".
    pub label: String,
    /// What to run: a path, or a name found in PATH.
    pub program: String,
    pub args: Vec<String>,
}

impl ShellOption {
    fn new(label: &str, program: impl Into<String>, args: &[&str]) -> ShellOption {
        ShellOption {
            label: label.to_string(),
            program: program.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }
}

/// The shells found here, the preferred first.
pub fn available_shells() -> Vec<ShellOption> {
    if cfg!(windows) {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let env = WindowsEnv {
            program_files: var("ProgramFiles"),
            program_files_x86: var("ProgramFiles(x86)"),
            local_app_data: var("LOCALAPPDATA"),
            system_root: var("SystemRoot"),
        };
        let dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        windows_shells(
            &env,
            |path| Path::new(path).is_file(),
            |program| crate::agent::find_program(program, &dirs).map(|p| p.display().to_string()),
        )
    } else {
        let listed = std::fs::read_to_string("/etc/shells").unwrap_or_default();
        unix_shells(&listed, |path| Path::new(path).is_file())
    }
}

/// What a terminal runs when the config names no program. On Windows: Git Bash, else
/// PowerShell 7, else Windows PowerShell (Windows PowerShell 5.1 is plain and slow, and agents
/// work best in a POSIX shell). Elsewhere None: the user's login shell ($SHELL).
pub fn default_shell() -> Option<ShellOption> {
    if cfg!(windows) {
        Some(windows_default(&available_shells()))
    } else {
        None
    }
}

/// How the default shell is named in the settings: "Git Bash"; "zsh" for $SHELL.
pub fn default_shell_label() -> String {
    match default_shell() {
        Some(shell) => shell.label,
        None => std::env::var("SHELL")
            .ok()
            .and_then(|s| {
                Path::new(&s)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "sh".to_string()),
    }
}

/// Where Windows keeps programs, from its environment.
#[derive(Default)]
struct WindowsEnv {
    program_files: Option<String>,
    program_files_x86: Option<String>,
    local_app_data: Option<String>,
    system_root: Option<String>,
}

/// Windows's shells in order of preference. `exists` tells whether a file exists; `which` finds
/// a program in PATH.
fn windows_shells(
    env: &WindowsEnv,
    exists: impl Fn(&str) -> bool,
    which: impl Fn(&str) -> Option<String>,
) -> Vec<ShellOption> {
    let under = |base: &Option<String>, rest: &str| base.as_ref().map(|b| format!("{b}\\{rest}"));
    let first =
        |candidates: Vec<Option<String>>| candidates.into_iter().flatten().find(|p| exists(p));
    // Git for Windows: its bash beside the `git` in PATH (`…\Git\cmd\git.exe`), or where its
    // installer puts it.
    let beside_git = which("git").and_then(|git| {
        let cmd = git.rsplit_once('\\')?.0;
        let root = cmd.rsplit_once('\\')?.0;
        Some(format!("{root}\\bin\\bash.exe"))
    });
    let git_bash = first(vec![
        beside_git,
        under(&env.program_files, "Git\\bin\\bash.exe"),
        under(&env.program_files_x86, "Git\\bin\\bash.exe"),
        under(&env.local_app_data, "Programs\\Git\\bin\\bash.exe"),
    ]);
    let pwsh =
        which("pwsh").or_else(|| first(vec![under(&env.program_files, "PowerShell\\7\\pwsh.exe")]));
    let system = |rest: &str| first(vec![under(&env.system_root, rest)]);
    [
        git_bash.map(|p| ShellOption::new("Git Bash", p, &["--login", "-i"])),
        pwsh.map(|p| ShellOption::new("PowerShell 7", p, &["-NoLogo"])),
        system("System32\\WindowsPowerShell\\v1.0\\powershell.exe")
            .or_else(|| which("powershell"))
            .map(|p| ShellOption::new("Windows PowerShell", p, &["-NoLogo"])),
        system("System32\\wsl.exe").map(|p| ShellOption::new("WSL", p, &[])),
        which("nu").map(|p| ShellOption::new("Nushell", p, &[])),
        system("System32\\cmd.exe").map(|p| ShellOption::new("Command Prompt", p, &[])),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The first of Git Bash, PowerShell 7 and Windows PowerShell found; Windows PowerShell by name
/// when none is.
fn windows_default(found: &[ShellOption]) -> ShellOption {
    ["Git Bash", "PowerShell 7", "Windows PowerShell"]
        .iter()
        .find_map(|label| found.iter().find(|s| s.label == *label))
        .cloned()
        .unwrap_or_else(|| ShellOption::new("Windows PowerShell", "powershell", &["-NoLogo"]))
}

/// The login shells `/etc/shells` lists that exist, once each by name, in its order.
fn unix_shells(listed: &str, exists: impl Fn(&str) -> bool) -> Vec<ShellOption> {
    let mut shells: Vec<ShellOption> = Vec::new();
    for path in listed.lines().map(str::trim).filter(|l| l.starts_with('/')) {
        let name = path.rsplit('/').next().unwrap_or(path);
        // Not shells to type into.
        if matches!(
            name,
            "nologin" | "false" | "git-shell" | "rbash" | "tmux" | "screen"
        ) || shells.iter().any(|s| s.label == name)
            || !exists(path)
        {
            continue;
        }
        shells.push(ShellOption::new(name, path, &[]));
    }
    shells
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows() -> WindowsEnv {
        WindowsEnv {
            program_files: Some("C:\\Program Files".into()),
            program_files_x86: Some("C:\\Program Files (x86)".into()),
            local_app_data: Some("C:\\Users\\u\\AppData\\Local".into()),
            system_root: Some("C:\\Windows".into()),
        }
    }

    fn labels(shells: &[ShellOption]) -> Vec<&str> {
        shells.iter().map(|s| s.label.as_str()).collect()
    }

    #[test]
    fn windows_shells_are_found_the_preferred_first() {
        let files = [
            "C:\\Program Files\\Git\\bin\\bash.exe",
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
            "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            "C:\\Windows\\System32\\wsl.exe",
            "C:\\Windows\\System32\\cmd.exe",
        ];
        let which = |p: &str| (p == "nu").then(|| "C:\\Users\\u\\.cargo\\bin\\nu.exe".to_string());
        let shells = windows_shells(&windows(), |p| files.contains(&p), which);
        assert_eq!(
            labels(&shells),
            [
                "Git Bash",
                "PowerShell 7",
                "Windows PowerShell",
                "WSL",
                "Nushell",
                "Command Prompt"
            ]
        );
        assert_eq!(shells[0].program, "C:\\Program Files\\Git\\bin\\bash.exe");
        assert_eq!(shells[0].args, ["--login", "-i"]);
        assert_eq!(windows_default(&shells).label, "Git Bash");
    }

    #[test]
    fn git_bash_is_found_beside_the_git_in_path() {
        let bash = "D:\\tools\\Git\\bin\\bash.exe";
        let which = |p: &str| (p == "git").then(|| "D:\\tools\\Git\\cmd\\git.exe".to_string());
        let shells = windows_shells(&windows(), |p| p == bash, which);
        assert_eq!(shells[0].program, bash);
    }

    #[test]
    fn without_git_bash_the_default_is_powershell_7_then_windows_powershell() {
        let pwsh = |p: &str| (p == "pwsh").then(|| "C:\\pwsh\\pwsh.exe".to_string());
        let shells = windows_shells(&windows(), |_| false, pwsh);
        assert_eq!(windows_default(&shells).label, "PowerShell 7");
        let none = windows_shells(&WindowsEnv::default(), |_| false, |_| None);
        assert!(none.is_empty());
        let fallback = windows_default(&none);
        assert_eq!(
            (fallback.label.as_str(), fallback.program.as_str()),
            ("Windows PowerShell", "powershell")
        );
    }

    #[test]
    fn unix_shells_come_from_etc_shells_once_each() {
        let listed = "# /etc/shells\n/bin/sh\n/bin/bash\n/usr/bin/bash\n/usr/bin/zsh\n\
                      /usr/bin/git-shell\n/usr/bin/nologin\n/usr/bin/fish\n/opt/gone/ksh\n";
        let shells = unix_shells(listed, |p| p != "/opt/gone/ksh");
        assert_eq!(labels(&shells), ["sh", "bash", "zsh", "fish"]);
        assert_eq!(shells[1].program, "/bin/bash", "the first listed");
    }
}
