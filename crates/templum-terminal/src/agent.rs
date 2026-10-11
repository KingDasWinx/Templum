//! The AI coding agents Templum knows: how to start each one in a folder, how to install it, how
//! to tell from a process that it runs in a terminal, and what brings it back when a canvas is
//! restored. Facts checked on 2026-10-10 against each agent's docs (see docs/specs/agents.md).

use std::path::{Path, PathBuf};

/// One agent's facts.
#[derive(Debug, PartialEq, Eq)]
pub struct Spec {
    /// As the saved canvas spells it; never changes.
    pub id: &'static str,
    pub title: &'static str,
    /// Typed into the shell to start it in the shell's folder.
    pub command: &'static str,
    /// Resumes the folder's latest session. Only for agents whose resume is limited to the
    /// folder: any other would bring back another project's conversation.
    pub resume: Option<&'static str>,
    /// How to install it, for someone who hasn't.
    pub install: &'static str,
    /// Its process or script names (the program, a renamed process title, a native child).
    pub names: &'static [&'static str],
    /// Pieces of its npm package's path, in a `node` process's script argument.
    pub packages: &'static [&'static str],
}

const SPECS: [Spec; 20] = [
    Spec {
        id: "claude",
        title: "Claude Code",
        command: "claude",
        resume: Some("claude --continue"),
        install: "curl -fsSL https://claude.ai/install.sh | bash",
        names: &["claude"],
        packages: &["@anthropic-ai/claude-code"],
    },
    Spec {
        id: "codex",
        title: "Codex",
        command: "codex",
        resume: Some("codex resume --last"),
        install: "npm install -g @openai/codex",
        names: &["codex"],
        packages: &["@openai/codex"],
    },
    Spec {
        id: "gemini",
        title: "Gemini CLI",
        command: "gemini",
        resume: Some("gemini --resume"),
        install: "npm install -g @google/gemini-cli",
        names: &["gemini"],
        packages: &["@google/gemini-cli"],
    },
    Spec {
        id: "copilot",
        title: "GitHub Copilot CLI",
        command: "copilot",
        resume: None,
        install: "curl -fsSL https://gh.io/copilot-install | bash",
        names: &["copilot"],
        packages: &["@github/copilot"],
    },
    Spec {
        id: "cursor",
        title: "Cursor CLI",
        command: "cursor-agent",
        resume: None,
        install: "curl https://cursor.com/install -fsS | bash",
        names: &["cursor-agent"],
        packages: &["cursor-agent/"],
    },
    Spec {
        id: "opencode",
        title: "OpenCode",
        command: "opencode",
        resume: Some("opencode --continue"),
        install: "curl -fsSL https://opencode.ai/install | bash",
        names: &["opencode"],
        packages: &["node_modules/opencode-ai/"],
    },
    Spec {
        id: "amp",
        title: "Amp",
        command: "amp",
        resume: Some("amp threads continue --last"),
        install: "curl -fsSL https://ampcode.com/install.sh | bash",
        names: &["amp"],
        packages: &["@sourcegraph/amp"],
    },
    Spec {
        id: "kiro",
        title: "Kiro CLI",
        command: "kiro-cli chat",
        resume: Some("kiro-cli chat --resume"),
        install: "curl -fsSL https://cli.kiro.dev/install | bash",
        names: &["kiro-cli"],
        packages: &[],
    },
    Spec {
        id: "qwen",
        title: "Qwen Code",
        command: "qwen",
        resume: Some("qwen --continue"),
        install: "npm install -g @qwen-code/qwen-code@latest",
        names: &["qwen"],
        packages: &["@qwen-code/qwen-code"],
    },
    Spec {
        id: "grok",
        title: "Grok Build",
        command: "grok",
        resume: Some("grok --continue"),
        install: "curl -fsSL https://x.ai/cli/install.sh | bash",
        names: &["grok"],
        packages: &["@xai-official/grok"],
    },
    Spec {
        id: "kimi",
        title: "Kimi Code",
        command: "kimi",
        resume: Some("kimi --continue"),
        install: "curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash",
        names: &["kimi", "kimi-code"],
        packages: &["@moonshot-ai/kimi-code"],
    },
    Spec {
        id: "vibe",
        title: "Mistral Vibe",
        command: "vibe",
        resume: Some("vibe --continue"),
        install: "curl -LsSf https://mistral.ai/vibe/install.sh | bash",
        names: &["vibe"],
        packages: &[],
    },
    Spec {
        id: "aider",
        title: "Aider",
        command: "aider",
        resume: Some("aider --restore-chat-history"),
        install: "curl -LsSf https://aider.chat/install.sh | sh",
        names: &["aider"],
        packages: &[],
    },
    Spec {
        id: "goose",
        title: "Goose",
        command: "goose session",
        resume: None,
        install: "curl -fsSL https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh | bash",
        names: &["goose"],
        packages: &[],
    },
    Spec {
        id: "crush",
        title: "Crush",
        command: "crush",
        resume: None,
        install: "npm install -g @charmland/crush",
        names: &["crush"],
        packages: &["@charmland/crush"],
    },
    Spec {
        id: "droid",
        title: "Droid",
        command: "droid",
        resume: None,
        install: "curl -fsSL https://app.factory.ai/cli | sh",
        names: &["droid"],
        packages: &["node_modules/droid/"],
    },
    Spec {
        id: "auggie",
        title: "Auggie",
        command: "auggie",
        resume: None,
        install: "npm install -g @augmentcode/auggie",
        names: &["auggie"],
        packages: &["@augmentcode/auggie"],
    },
    Spec {
        id: "kilo",
        title: "Kilo Code",
        command: "kilo",
        resume: Some("kilo --continue"),
        install: "npm install -g @kilocode/cli",
        names: &["kilo", "kilocode", ".kilo"],
        packages: &["@kilocode/cli"],
    },
    Spec {
        id: "cline",
        title: "Cline",
        command: "cline",
        resume: None,
        install: "npm install -g cline",
        names: &["cline", ".cline"],
        packages: &["node_modules/cline/"],
    },
    Spec {
        id: "pi",
        title: "Pi",
        command: "pi",
        resume: Some("pi --continue"),
        install: "curl -fsSL https://pi.dev/install.sh | sh",
        names: &["pi"],
        packages: &["@earendil-works/pi-coding-agent"],
    },
];

/// An agent Templum knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Agent(usize);

impl Agent {
    pub const CLAUDE: Agent = Agent(0);
    pub const CODEX: Agent = Agent(1);

    /// Every agent, in the order menus list them.
    pub fn all() -> impl Iterator<Item = Agent> {
        (0..SPECS.len()).map(Agent)
    }

    pub fn spec(self) -> &'static Spec {
        &SPECS[self.0]
    }

    /// As the saved canvas spells it.
    pub fn name(self) -> &'static str {
        self.spec().id
    }

    pub fn from_name(name: &str) -> Option<Agent> {
        Agent::all().find(|a| a.spec().id == name)
    }

    pub fn title(self) -> &'static str {
        self.spec().title
    }

    pub fn command(self) -> &'static str {
        self.spec().command
    }

    /// The program the command runs, to tell whether it is installed.
    pub fn program(self) -> &'static str {
        self.command().split_whitespace().next().unwrap_or_default()
    }

    pub fn install(self) -> &'static str {
        self.spec().install
    }

    /// What brings it back in its folder: its resume command when that is limited to the
    /// folder, else a new session.
    pub fn resume_command(self) -> &'static str {
        self.spec().resume.unwrap_or(self.command())
    }
}

/// The agent a process is, from its name and arguments (`args[0]` is the program). An agent
/// installed with npm runs as `node` with its script first: the package's file, or the bin link
/// the shell found. One installed with pip or uv runs as its script, by name.
pub fn agent_of(name: &str, args: &[&str]) -> Option<Agent> {
    let program = args.first().map_or_else(String::new, |a| stem(a));
    let by_name = |n: &str| Agent::all().find(|a| a.spec().names.contains(&n));
    // The name, or the first argument: on macOS an npm agent's `process.title` replaces it.
    if let Some(agent) = by_name(&stem(name)).or_else(|| by_name(&program)) {
        return Some(agent);
    }
    // Node 26 on Linux names the process after its main thread; Cursor's bundled Node keeps its
    // own name, so its script is what tells.
    let interpreter = |n: &str| matches!(n, "node" | "node-mainthread" | "bun");
    let script = args.iter().skip(1).find(|a| !a.starts_with('-'))?;
    let script = script.replace('\\', "/");
    if let Some(agent) = Agent::all().find(|a| a.spec().packages.iter().any(|p| script.contains(p)))
    {
        return Some(agent);
    }
    if !(interpreter(&stem(name)) || interpreter(&program)) {
        return None;
    }
    // An npm bin link (`…/bin/claude`); a script of the user's has an extension.
    Path::new(&script)
        .extension()
        .is_none()
        .then(|| by_name(&stem(&script)))
        .flatten()
}

/// Whether `program` can be run: found in PATH, or in the folders npm, bun, cargo and the like
/// install into (an app started from a desktop menu may not get the shell's PATH).
pub fn installed(program: &str) -> bool {
    find_program(program, &program_dirs()).is_some()
}

/// PATH, then the usual per-user install folders.
fn program_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::home_dir() {
        for sub in [
            ".local/bin",
            ".npm-global/bin",
            ".bun/bin",
            ".cargo/bin",
            ".deno/bin",
            ".volta/bin",
            "go/bin",
            "AppData/Roaming/npm",
            // Version managers that put shims on the shell's PATH only (mise, asdf).
            ".local/share/mise/shims",
            ".asdf/shims",
        ] {
            dirs.push(home.join(sub));
        }
        // nvm: every installed Node's bin folder.
        // ponytail: one read_dir; fnm and others add theirs when someone needs them.
        if let Ok(versions) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            dirs.extend(versions.flatten().map(|v| v.path().join("bin")));
        }
    }
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    dirs
}

/// The first `program` among `dirs` that can be run (on Windows, with a program extension).
fn find_program(program: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        ["exe", "cmd", "bat", "ps1"]
            .iter()
            .map(|ext| format!("{program}.{ext}"))
            .collect()
    } else {
        vec![program.to_string()]
    };
    dirs.iter()
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|path| runnable(path))
}

#[cfg(unix)]
fn runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn runnable(path: &Path) -> bool {
    path.is_file()
}

/// A program's lowercase file name without its extension (`.exe`), from either kind of slash.
fn stem(path: &str) -> String {
    Path::new(&path.replace('\\', "/"))
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(id: &str) -> Option<Agent> {
        Some(Agent::from_name(id).unwrap())
    }

    #[test]
    fn agents_are_known_by_name_or_by_the_script_node_runs() {
        let cases: &[(&str, &[&str], Option<Agent>)] = &[
            ("claude", &["claude"], named("claude")),
            (
                "claude.exe",
                &["C:\\Users\\u\\.local\\bin\\claude.exe"],
                named("claude"),
            ),
            ("codex", &["codex", "resume"], named("codex")),
            ("codex.exe", &["codex.exe"], named("codex")),
            (
                "node",
                &[
                    "node",
                    "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js",
                ],
                named("claude"),
            ),
            (
                "node.exe",
                &[
                    "node.exe",
                    "C:\\npm\\node_modules\\@openai\\codex\\bin\\codex.js",
                ],
                named("codex"),
            ),
            (
                "node",
                &["node", "/home/u/.npm-global/bin/claude"],
                named("claude"),
            ),
            (
                "node",
                &["node", "--no-warnings", "/usr/bin/codex"],
                named("codex"),
            ),
            // Node 26 on Linux: the main thread names the process.
            (
                "node-MainThread",
                &["node", "/x/node_modules/@openai/codex/bin/codex.js"],
                named("codex"),
            ),
            (
                "node-MainThread",
                &["node", "/usr/bin/claude"],
                named("claude"),
            ),
            // macOS: `process.title = "claude"` overwrites the arguments.
            ("node", &["claude", "TERM=xterm-256color"], named("claude")),
            // The agents added on 2026-10-10, as their processes look.
            (
                "node",
                &[
                    "node",
                    "/usr/lib/node_modules/@google/gemini-cli/bundle/gemini.js",
                ],
                named("gemini"),
            ),
            ("node", &["node", "/usr/local/bin/gemini"], named("gemini")),
            ("copilot", &["copilot"], named("copilot")),
            (
                "node",
                &["node", "/x/node_modules/@github/copilot/npm-loader.js"],
                named("copilot"),
            ),
            // Cursor's bundled Node: argv[0] is `…/agent`, a name too common to trust.
            (
                "node",
                &[
                    "/home/u/.local/bin/agent",
                    "/home/u/.local/share/cursor-agent/versions/2026.10.01/index.js",
                ],
                named("cursor"),
            ),
            ("node", &["/home/u/.local/bin/agent", "x.js"], None),
            ("opencode", &["opencode"], named("opencode")),
            ("amp", &["amp", "threads", "continue"], named("amp")),
            ("kiro-cli", &["kiro-cli", "chat"], named("kiro")),
            (
                "node",
                &["node", "/x/@qwen-code/qwen-code/cli-entry.js"],
                named("qwen"),
            ),
            ("grok", &["grok"], named("grok")),
            // Kimi's npm build renames its process.
            ("node", &["kimi-code"], named("kimi")),
            // A uv or pip script: its own name, run by Python.
            (
                "vibe",
                &["/usr/bin/python3", "/home/u/.local/bin/vibe"],
                named("vibe"),
            ),
            (
                "aider",
                &["python3", "/home/u/.local/bin/aider"],
                named("aider"),
            ),
            ("goose", &["goose", "session"], named("goose")),
            ("crush", &["crush"], named("crush")),
            ("droid", &["droid"], named("droid")),
            (
                "node",
                &["node", "/x/node_modules/@augmentcode/auggie/augment.mjs"],
                named("auggie"),
            ),
            // Kilo and Cline: a node parent, and a native child named with a dot.
            (".kilo", &["/x/.kilo"], named("kilo")),
            ("node", &["node", "/usr/bin/cline"], named("cline")),
            ("node", &["pi"], named("pi")),
            // The user's own scripts and programs.
            ("node", &["node", "claude.js"], None),
            ("node-MainThread", &["node", "tools/codex.mjs"], None),
            ("node", &["node", "server.js", "codex"], None),
            ("node", &["node"], None),
            ("python3", &["python3", "manage.py", "runserver"], None),
            ("vim", &["vim", "claude.md"], None),
            ("zsh", &["-zsh"], None),
        ];
        for (name, args, want) in cases {
            assert_eq!(agent_of(name, args), *want, "{name} {args:?}");
        }
    }

    #[test]
    fn every_agent_has_its_facts_and_a_unique_name() {
        let mut ids: Vec<&str> = Agent::all().map(Agent::name).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), SPECS.len(), "ids are unique");
        for agent in Agent::all() {
            let spec = agent.spec();
            assert_eq!(Agent::from_name(spec.id), Some(agent));
            assert!(
                !spec.title.is_empty() && !spec.install.is_empty(),
                "{spec:?}"
            );
            assert!(
                spec.names.contains(&agent.program()),
                "{}: its program is one of its names",
                spec.id
            );
            assert!(
                agent.resume_command().starts_with(agent.program()),
                "{spec:?}"
            );
        }
        assert_eq!(Agent::from_name("rm -rf ~"), None);
    }

    #[test]
    fn only_a_resume_limited_to_the_folder_is_used() {
        assert_eq!(Agent::CLAUDE.resume_command(), "claude --continue");
        assert_eq!(Agent::CODEX.resume_command(), "codex resume --last");
        // Copilot's resume may reopen another folder's session: a new one instead.
        assert_eq!(named("copilot").unwrap().resume_command(), "copilot");
        assert_eq!(named("goose").unwrap().resume_command(), "goose session");
    }

    #[test]
    fn a_program_is_found_only_where_it_can_run() {
        let d = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "fakeagent.cmd"
        } else {
            "fakeagent"
        };
        let file = d.path().join(name);
        std::fs::write(&file, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                find_program("fakeagent", &[d.path().into()]),
                None,
                "not executable"
            );
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let dirs = [d.path().join("missing"), d.path().into()];
        assert_eq!(find_program("fakeagent", &dirs), Some(file));
        assert_eq!(find_program("otheragent", &dirs), None);
    }
}
