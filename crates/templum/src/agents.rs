//! The AI agents in effect: Templum's own, with the user's changes from `[agents]` in
//! config.toml, then the user's own agents. The canvas's menu, the folder dialog, detecting
//! agents in terminals and restoring them all read this list. See docs/specs/agents.md.

use std::collections::BTreeMap;

use templum_terminal::{Agent, Detect, program_name};

use crate::config::{AgentConfig, valid_agent_id};

/// One agent as Templum uses it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AgentEntry {
    /// Its key in `[agents]` and in saved canvases.
    pub id: String,
    pub title: String,
    /// Typed into the shell to start it.
    pub command: String,
    /// Brings it back in its folder; None: a new session.
    pub resume: Option<String>,
    /// How to install it: Templum's own agents only.
    pub install: Option<&'static str>,
    /// One of Templum's own (it can be reset), or None for the user's (it can be deleted).
    pub builtin: Option<Agent>,
    /// The user changed one of Templum's own.
    pub changed: bool,
    /// Listed in the canvas's menu.
    pub show: bool,
}

impl AgentEntry {
    /// The program its command runs, to look for on this computer.
    pub fn program(&self) -> &str {
        self.command.split_whitespace().next().unwrap_or_default()
    }

    /// What brings it back on restore.
    pub fn resume_command(&self) -> &str {
        self.resume.as_deref().unwrap_or(&self.command)
    }

    /// How to tell it from a terminal's processes: Templum's names for its own, and the program
    /// of the command in effect (a wrapper the user set).
    pub fn detect(&self) -> Detect {
        let mut detect = match self.builtin {
            Some(agent) => agent.detect(),
            None => Detect {
                id: self.id.clone(),
                names: Vec::new(),
                packages: Vec::new(),
            },
        };
        let program = program_name(&self.command);
        if !program.is_empty() && !detect.names.contains(&program) {
            detect.names.push(program);
        }
        detect
    }
}

/// Templum's agents with `config`'s changes, in their order, then the user's, by id.
pub(crate) fn effective(config: &BTreeMap<String, AgentConfig>) -> Vec<AgentEntry> {
    fn text(v: &Option<String>) -> Option<&str> {
        v.as_deref().map(str::trim).filter(|v| !v.is_empty())
    }
    let resume = |v: &Option<String>, default: Option<&str>| match v.as_deref().map(str::trim) {
        Some("") => None,
        Some(r) => Some(r.to_string()),
        None => default.map(str::to_string),
    };
    let ours = Agent::all().map(|agent| {
        let spec = agent.spec();
        let set = config.get(spec.id);
        let empty = AgentConfig::default();
        let set_ = set.unwrap_or(&empty);
        AgentEntry {
            id: spec.id.to_string(),
            title: text(&set_.name).unwrap_or(spec.title).to_string(),
            command: text(&set_.command).unwrap_or(spec.command).to_string(),
            resume: resume(&set_.resume, spec.resume),
            install: Some(spec.install),
            builtin: Some(agent),
            changed: set.is_some_and(|s| *s != AgentConfig::default()),
            show: set_.show.unwrap_or(true),
        }
    });
    let theirs = config
        .iter()
        .filter(|(id, _)| Agent::from_name(id).is_none())
        .filter_map(|(id, set)| {
            Some(AgentEntry {
                id: id.clone(),
                title: text(&set.name)?.to_string(),
                command: text(&set.command)?.to_string(),
                resume: resume(&set.resume, None),
                install: None,
                builtin: None,
                changed: false,
                show: set.show.unwrap_or(true),
            })
        });
    ours.chain(theirs).collect()
}

/// What the settings dialog edits.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Draft {
    pub name: String,
    pub command: String,
    /// Empty: a new session on restore.
    pub resume: String,
    pub show: bool,
}

impl Draft {
    pub fn of(entry: &AgentEntry) -> Draft {
        Draft {
            name: entry.title.clone(),
            command: entry.command.clone(),
            resume: entry.resume.clone().unwrap_or_default(),
            show: entry.show,
        }
    }

    /// Why it can't be saved, if it can't.
    pub fn problem(&self) -> Option<&'static str> {
        if self.name.trim().is_empty() {
            Some("An agent needs a name")
        } else if self.command.trim().is_empty() {
            Some("An agent needs a command")
        } else {
            None
        }
    }

    /// What `[agents.<id>]` holds for it: only what differs from Templum's default for one of
    /// its own (None when nothing does), everything for one of the user's.
    pub fn to_config(&self, builtin: Option<Agent>) -> Option<AgentConfig> {
        let (name, command, resume) = (self.name.trim(), self.command.trim(), self.resume.trim());
        let config = match builtin {
            Some(agent) => {
                let default_resume = agent.spec().resume.unwrap_or("");
                AgentConfig {
                    name: (name != agent.title()).then(|| name.to_string()),
                    command: (command != agent.command()).then(|| command.to_string()),
                    resume: (resume != default_resume).then(|| resume.to_string()),
                    show: (!self.show).then_some(false),
                }
            }
            None => AgentConfig {
                name: Some(name.to_string()),
                command: Some(command.to_string()),
                resume: (!resume.is_empty()).then(|| resume.to_string()),
                show: (!self.show).then_some(false),
            },
        };
        (config != AgentConfig::default()).then_some(config)
    }
}

/// `config` as the value of `agents.<id>` in config.toml.
pub(crate) fn to_value(config: &AgentConfig) -> toml_edit::InlineTable {
    let mut table = toml_edit::InlineTable::new();
    for (key, value) in [
        ("name", &config.name),
        ("command", &config.command),
        ("resume", &config.resume),
    ] {
        if let Some(value) = value {
            table.insert(key, value.as_str().into());
        }
    }
    if let Some(show) = config.show {
        table.insert("show", show.into());
    }
    table
}

/// A new agent's id, from its name: `"My Agent!"` is `my-agent`, or `my-agent-2` when taken.
pub(crate) fn new_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let slug: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if valid_agent_id(&slug) {
        slug
    } else {
        "agent".to_string()
    };
    std::iter::once(base.clone())
        .chain((2..).map(|n| format!("{base}-{n}")))
        .find(|id| !taken(id) && Agent::from_name(id).is_none())
        .expect("a free id")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;

    fn agents(text: &str) -> Vec<AgentEntry> {
        effective(&parse(text).unwrap().agents)
    }

    fn by<'a>(list: &'a [AgentEntry], id: &str) -> &'a AgentEntry {
        list.iter().find(|a| a.id == id).unwrap()
    }

    #[test]
    fn ours_come_first_as_they_are_without_changes() {
        let list = agents("");
        assert_eq!(list.len(), Agent::all().count());
        let claude = by(&list, "claude");
        assert_eq!(claude.title, "Claude Code");
        assert_eq!(claude.command, "claude");
        assert_eq!(claude.resume_command(), "claude --continue");
        assert!(claude.show && !claude.changed && claude.builtin.is_some());
        assert_eq!(
            by(&list, "copilot").resume_command(),
            "copilot",
            "a new session"
        );
    }

    #[test]
    fn changes_apply_and_the_users_agents_follow() {
        let list = agents(concat!(
            "[agents]\n",
            "claude = { command = \"claude --model opus\", resume = \"\" }\n",
            "codex = { show = false }\n",
            "zed = { name = \"Zed agent\", command = \"zed-agent --tui\" }\n",
        ));
        let claude = by(&list, "claude");
        assert_eq!(claude.command, "claude --model opus");
        assert_eq!(
            claude.resume_command(),
            "claude --model opus",
            "empty: a new session"
        );
        assert!(claude.changed);
        assert!(!by(&list, "codex").show);
        let zed = list.last().unwrap();
        assert_eq!((zed.id.as_str(), zed.title.as_str()), ("zed", "Zed agent"));
        assert_eq!(
            (zed.builtin, zed.install, zed.resume.clone()),
            (None, None, None)
        );
        assert_eq!(zed.program(), "zed-agent");
        assert_eq!(zed.detect().names, ["zed-agent"]);
        // A wrapper set as Claude's command is Claude too.
        let wrapped = agents("[agents.claude]\ncommand = \"~/bin/cc --x\"\n");
        let names = by(&wrapped, "claude").detect().names;
        assert!(names.contains(&"claude".to_string()) && names.contains(&"cc".to_string()));
    }

    #[test]
    fn only_what_differs_from_ours_is_kept() {
        let claude = Agent::CLAUDE;
        let mut draft = Draft::of(&by(&agents(""), "claude").clone());
        assert_eq!(
            draft.to_config(Some(claude)),
            None,
            "unchanged: nothing to keep"
        );
        draft.command = " claude --model opus ".into();
        draft.show = false;
        let kept = draft.to_config(Some(claude)).unwrap();
        assert_eq!(kept.command.as_deref(), Some("claude --model opus"));
        assert_eq!(
            (kept.name, kept.resume, kept.show),
            (None, None, Some(false))
        );
        let value = to_value(&draft.to_config(Some(claude)).unwrap()).to_string();
        assert!(
            value.contains("command = \"claude --model opus\""),
            "{value}"
        );
        // One of the user's keeps everything it needs.
        let mine = Draft {
            name: "Mine".into(),
            command: "mine".into(),
            resume: String::new(),
            show: true,
        };
        let kept = mine.to_config(None).unwrap();
        assert_eq!(
            (kept.name.as_deref(), kept.command.as_deref()),
            (Some("Mine"), Some("mine"))
        );
        assert_eq!(kept.resume, None);
        assert_eq!(
            Draft {
                command: " ".into(),
                ..mine
            }
            .problem(),
            Some("An agent needs a command")
        );
    }

    #[test]
    fn new_ids_come_from_the_name_and_stay_free() {
        assert_eq!(new_id("My Agent!", |_| false), "my-agent");
        assert_eq!(new_id("My Agent", |id| id == "my-agent"), "my-agent-2");
        assert_eq!(new_id("Claude", |_| false), "claude-2", "ours are taken");
        assert_eq!(new_id("日本", |_| false), "agent");
    }
}
