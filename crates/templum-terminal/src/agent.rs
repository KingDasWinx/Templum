//! The coding agents Templum recognizes in a terminal, to resume them when a canvas is restored.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    /// As the saved canvas spells it.
    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }

    pub fn from_name(name: &str) -> Option<Agent> {
        match name {
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            _ => None,
        }
    }

    /// What resumes the folder's latest conversation (both checked on 2026-10-10).
    pub fn resume_command(self) -> &'static str {
        match self {
            Agent::Claude => "claude --continue",
            Agent::Codex => "codex resume --last",
        }
    }
}

/// The agent a process is, from its name and arguments (`args[0]` is the program). An agent
/// installed with npm runs as `node` with its script (the package's, or the bin link) first.
pub fn agent_of(name: &str, args: &[&str]) -> Option<Agent> {
    match stem(name).as_str() {
        "node" => {
            let script = args.iter().skip(1).find(|a| !a.starts_with('-'))?;
            let script = script.replace('\\', "/");
            if script.contains("@anthropic-ai/claude-code") {
                Some(Agent::Claude)
            } else if script.contains("@openai/codex") {
                Some(Agent::Codex)
            } else {
                Agent::from_name(&stem(&script))
            }
        }
        other => Agent::from_name(other),
    }
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

    #[test]
    fn agents_are_known_by_name_or_by_the_script_node_runs() {
        let cases: &[(&str, &[&str], Option<Agent>)] = &[
            ("claude", &["claude"], Some(Agent::Claude)),
            (
                "claude.exe",
                &["C:\\Users\\u\\.local\\bin\\claude.exe"],
                Some(Agent::Claude),
            ),
            ("codex", &["codex", "resume"], Some(Agent::Codex)),
            ("codex.exe", &["codex.exe"], Some(Agent::Codex)),
            (
                "node",
                &[
                    "node",
                    "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js",
                ],
                Some(Agent::Claude),
            ),
            (
                "node.exe",
                &[
                    "node.exe",
                    "C:\\npm\\node_modules\\@openai\\codex\\bin\\codex.js",
                ],
                Some(Agent::Codex),
            ),
            (
                "node",
                &["node", "/home/u/.npm-global/bin/claude"],
                Some(Agent::Claude),
            ),
            (
                "node",
                &["node", "--no-warnings", "/usr/bin/codex"],
                Some(Agent::Codex),
            ),
            ("node", &["node", "server.js", "codex"], None),
            ("node", &["node"], None),
            ("vim", &["vim", "claude.md"], None),
            ("zsh", &["-zsh"], None),
        ];
        for (name, args, want) in cases {
            assert_eq!(agent_of(name, args), *want, "{name} {args:?}");
        }
    }

    #[test]
    fn names_and_commands() {
        for agent in [Agent::Claude, Agent::Codex] {
            assert_eq!(Agent::from_name(agent.name()), Some(agent));
        }
        assert_eq!(Agent::from_name("rm -rf ~"), None);
        assert_eq!(Agent::Claude.resume_command(), "claude --continue");
        assert_eq!(Agent::Codex.resume_command(), "codex resume --last");
    }
}
