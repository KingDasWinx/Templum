//! The AI agents in effect: Templum's own, with the user's changes from `[agents]` in
//! config.toml, then the user's own agents. The canvas's menu, the folder dialog, detecting
//! agents in terminals and restoring them all read this list. See docs/specs/agents.md.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use templum_terminal::{Agent, Detect, program_name};
use templum_usage::accounts::command::{self, SHELL_WORDS};
use templum_usage::accounts::{Kind, Report, Share, When};

use crate::config::{AgentConfig, CommitMessage, valid_agent_id};

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
    /// Runs it without a terminal to write a commit message (`{model}`, `{prompt_file}`,
    /// `{output_file}`); None: it writes none.
    pub generate: Option<String>,
    /// Its models for commit messages, the recommended one first.
    pub models: Vec<String>,
    /// The label of the shell it opens in; None: the default shell.
    pub shell: Option<String>,
    /// An account of `builtin` (docs/specs/accounts.md): its folder and what it shares.
    pub account: Option<Account>,
}

/// An account: its agent run with its own folder.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Account {
    pub kind: &'static Kind,
    /// What its agent's variable names, with no trailing separator: the macOS Keychain names a
    /// login by this exact text.
    pub folder: PathBuf,
    /// The person chose it; false: `<data>/accounts/<id>`.
    pub chosen: bool,
    /// Its title is config.toml's `name`; false: the default, its agent's and its id.
    pub named: bool,
    /// Its items shared, by key: the agent's id or another account of it.
    pub share: BTreeMap<String, String>,
}

/// The state folder of `kind`'s own folder, as Templum's environment names it among `agents`.
pub(crate) fn own_state(kind: &Kind, agents: &[AgentEntry]) -> Option<PathBuf> {
    // Tests never reach the person's own agent folders (~/.claude…): `<OWN_FOLDERS>/<agent>`.
    #[cfg(test)]
    let _ = agents;
    #[cfg(test)]
    return OWN_FOLDERS
        .with_borrow(|dir| dir.as_ref().map(|d| kind.state_dir(&d.join(kind.agent))));
    #[cfg(not(test))]
    own_state_in(
        kind,
        std::env::var_os(kind.var),
        std::env::home_dir().as_deref(),
        agents,
    )
}

/// `own_state` with its variable `var` and the home folder `home`. A variable that names one of
/// `agents`' accounts' folders is Templum's started from that account's block, not the agent's.
fn own_state_in(
    kind: &Kind,
    var: Option<std::ffi::OsString>,
    home: Option<&Path>,
    agents: &[AgentEntry],
) -> Option<PathBuf> {
    let var = var.filter(|v| {
        !agents
            .iter()
            .filter_map(|a| a.account.as_ref())
            .any(|a| a.folder == Path::new(v))
    });
    kind.own_folder(var, home).map(|f| kind.state_dir(&f))
}

/// Where the agents keep their files, and the relay its own, for the data folder `data`. Tests
/// get the agents' `<OWN_FOLDERS>/<agent>` (as `own_state` does), never the person's `~/.claude`.
pub(crate) fn dirs(data: &Path) -> templum_usage::Dirs {
    #[cfg(test)]
    let own = |agent| templum_usage::accounts::kind(agent).and_then(|k| own_state(k, &[]));
    #[cfg(test)]
    return templum_usage::Dirs {
        relay: templum_usage::relay_dir(data),
        claude: own("claude"),
        codex: own("codex"),
        qwen: None,
        gemini: own("gemini"),
    };
    #[cfg(not(test))]
    templum_usage::Dirs::of(data)
}

#[cfg(test)]
thread_local! {
    /// Where a test's agents keep their own folders; None: they have none.
    pub(crate) static OWN_FOLDERS: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

impl AgentEntry {
    /// An account's variable and its folder.
    pub fn var(&self) -> Option<(&'static str, PathBuf)> {
        self.account
            .as_ref()
            .map(|a| (a.kind.var, a.folder.clone()))
    }

    /// An account's choice for each of its items: the state folder it is shared with, among
    /// `agents`, or its own.
    pub fn shares(&self, agents: &[AgentEntry]) -> Vec<Share> {
        let Some(account) = &self.account else {
            return Vec::new();
        };
        let of = account.kind.agent;
        account
            .kind
            .items
            .iter()
            .map(|item| Share {
                key: item.key,
                from: account.share.get(item.key).and_then(|target| {
                    if target == of {
                        own_state(account.kind, agents)
                    } else {
                        let other = agents.iter().find(|a| a.id == *target)?.account.as_ref()?;
                        Some(other.kind.state_dir(&other.folder))
                    }
                }),
            })
            .collect()
    }

    /// Applies this account's choices to its folder (`when`), after those of the accounts it
    /// shares from, directly or through them, as they start: a link of its own never makes one of
    /// their shared items a real folder. Their own start tells what they met.
    pub fn apply(&self, agents: &[AgentEntry], when: When) -> Report {
        let Some(account) = &self.account else {
            return Report::default();
        };
        let mut seen = vec![self.id.as_str()];
        let mut first = Vec::new();
        upstream(self, agents, &mut seen, &mut first);
        for other in first {
            let theirs = other.account.as_ref().expect("an account");
            templum_usage::accounts::apply(
                theirs.kind,
                &theirs.folder,
                &other.shares(agents),
                &other.sources(agents),
                When::Starting,
                !theirs.chosen,
            );
        }
        templum_usage::accounts::apply(
            account.kind,
            &account.folder,
            &self.shares(agents),
            &self.sources(agents),
            when,
            !account.chosen,
        )
    }

    /// Every state folder a share of this account may point at: its agent's own folder's, then
    /// each of its other accounts'.
    pub fn sources(&self, agents: &[AgentEntry]) -> Vec<PathBuf> {
        let Some(account) = &self.account else {
            return Vec::new();
        };
        let others = agents.iter().filter(|a| a.id != self.id).filter_map(|a| {
            let other = a.account.as_ref()?;
            (other.kind.agent == account.kind.agent).then(|| other.kind.state_dir(&other.folder))
        });
        own_state(account.kind, agents)
            .into_iter()
            .chain(others)
            .collect()
    }

    /// The program its command runs, to look for on this computer.
    pub fn program(&self) -> &str {
        self.command.split_whitespace().next().unwrap_or_default()
    }

    /// What brings it back on restore.
    pub fn resume_command(&self) -> &str {
        self.resume.as_deref().unwrap_or(&self.command)
    }

    /// What starts a fork of its session `session` (docs/specs/agent-tools.md, spawn_agent with
    /// fork): its builtin's, as a resume by id is; None for one of the person's agents.
    pub fn fork_command(&self, session: &str) -> Option<String> {
        self.forking()?.fork_session(session)
    }

    /// The agent of Templum's it forks as, when it can fork.
    pub fn forking(&self) -> Option<Agent> {
        self.builtin.filter(|agent| agent.spec().fork_id.is_some())
    }

    /// What starts it: its command, which starts a new session with the id `id` when it is one
    /// of Templum's agents that takes one and the command runs its program (not a wrapper).
    pub fn start_command(&self, id: &str) -> String {
        let arg = self
            .builtin
            .filter(|agent| self.program() == agent.program())
            .and_then(|agent| agent.new_session(id));
        match arg {
            Some(arg) => format!("{} {arg}", self.command),
            None => self.command.clone(),
        }
    }

    /// What brings back its session `session` (an id) on restore: that session, when it is one
    /// of Templum's agents that resumes by id and its resume command is Templum's own; else its
    /// resume command, which brings back the folder's latest. `kept`: whether the agent still
    /// has that session; one it dropped (Gemini's, with no message) starts again on its id.
    pub fn resume_command_for(&self, session: Option<&str>, kept: bool) -> String {
        let ours = self
            .builtin
            .filter(|agent| self.resume.as_deref() == agent.spec().resume);
        match (ours, session) {
            (Some(_), Some(id)) if !kept => self.start_command(id),
            (Some(agent), Some(id)) => agent
                .resume_session(id)
                .unwrap_or_else(|| self.resume_command().to_string()),
            _ => self.resume_command().to_string(),
        }
    }

    /// The words that start it on `model` and `effort` (`spawn_agent`), after its command: its
    /// builtin's, the person's own agents taking neither; then a role's `args`, the person's words
    /// for its command line, typed as they wrote them, as an `[agents]` command is.
    pub fn launch_args_with(
        &self,
        model: Option<&str>,
        effort: Option<&str>,
        args: Option<&str>,
    ) -> Result<String, String> {
        let flags = match self.builtin {
            Some(agent) => agent.launch_args(model, effort)?,
            None if model.is_none() && effort.is_none() => String::new(),
            None => {
                return Err(format!(
                    "Templum doesn't give {} a model or an effort: it is one of your own agents.",
                    self.title
                ));
            }
        };
        Ok(flags + &args.map_or(String::new(), |a| format!(" {}", a.trim())))
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
                var: None,
            },
        };
        // An agent with accounts: its folder is read from its process, to name the block's account.
        detect.var = templum_usage::accounts::kind(&self.id).map(|kind| kind.var.to_string());
        let program = program_name(&self.command);
        if !program.is_empty() && !detect.names.contains(&program) {
            detect.names.push(program);
        }
        detect
    }
}

/// The accounts `entry` shares from, through them too, the farthest first, each once (`seen`).
// ponytail: whole accounts in one order; a loop across items (claude3's skills from claude4,
// claude4's settings from claude3) applies one of them before the other's item is linked.
fn upstream<'a>(
    entry: &AgentEntry,
    agents: &'a [AgentEntry],
    seen: &mut Vec<&'a str>,
    out: &mut Vec<&'a AgentEntry>,
) {
    for target in entry.account.iter().flat_map(|a| a.share.values()) {
        let Some(other) = agents
            .iter()
            .find(|a| a.id == *target && a.account.is_some())
        else {
            continue;
        };
        if seen.contains(&other.id.as_str()) {
            continue;
        }
        seen.push(&other.id);
        upstream(other, agents, seen, out);
        out.push(other);
    }
}

/// Templum's agents with `config`'s changes, in their order, then the user's, by id, then the
/// accounts, by id (none without Templum's data folder `data`). `codex`: Codex's own list of
/// models (`codex_models`).
pub(crate) fn effective(
    config: &BTreeMap<String, AgentConfig>,
    codex: &[String],
    data: Option<&Path>,
) -> Vec<AgentEntry> {
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
        let (generate, models) = preset(agent, codex);
        AgentEntry {
            id: spec.id.to_string(),
            title: text(&set_.name).unwrap_or(spec.title).to_string(),
            command: text(&set_.command).unwrap_or(spec.command).to_string(),
            resume: resume(&set_.resume, spec.resume),
            install: Some(spec.install),
            builtin: Some(agent),
            changed: set.is_some_and(|s| *s != AgentConfig::default()),
            show: set_.show.unwrap_or(true),
            generate: text(&set_.generate).or(generate).map(str::to_string),
            models: set_.models.clone().unwrap_or(models),
            shell: text(&set_.shell).map(str::to_string),
            account: None,
        }
    });
    let ours: Vec<AgentEntry> = ours.collect();
    let accounts = config.iter().filter_map(|(id, set)| {
        let kind = templum_usage::accounts::kind(set.of.as_deref()?)?;
        let agent = ours.iter().find(|a| a.id == kind.agent)?;
        let (folder, chosen) = match text(&set.folder) {
            Some(folder) => (crate::launcher::expand(folder), true),
            None => (data?.join("accounts").join(id), false),
        };
        Some(AgentEntry {
            id: id.clone(),
            title: text(&set.name)
                .map_or_else(|| format!("{} ({id})", agent.title), str::to_string),
            changed: false,
            show: set.show.unwrap_or(true),
            // An account's own shell, never its agent's.
            shell: text(&set.shell).map(str::to_string),
            account: Some(Account {
                kind,
                folder: folder.components().collect(),
                chosen,
                named: text(&set.name).is_some(),
                share: set.share.clone().unwrap_or_default(),
            }),
            ..agent.clone()
        })
    });
    let accounts: Vec<AgentEntry> = accounts.collect();
    let theirs = config
        .iter()
        .filter(|(id, set)| Agent::from_name(id).is_none() && set.of.is_none())
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
                generate: text(&set.generate).map(str::to_string),
                models: set.models.clone().unwrap_or_default(),
                shell: text(&set.shell).map(str::to_string),
                account: None,
            })
        });
    ours.into_iter().chain(theirs).chain(accounts).collect()
}

/// The agents to look for in a terminal's processes: an account isn't one (its block's account
/// names it, docs/specs/accounts.md, Its block).
pub(crate) fn detects(agents: &[AgentEntry]) -> Vec<Detect> {
    agents
        .iter()
        .filter(|a| a.account.is_none())
        .map(AgentEntry::detect)
        .collect()
}

/// Where the commands of accounts go: `~/.local/bin`, on every OS. Tests never reach it: a test
/// that needs it sets `COMMANDS` to a temporary folder.
pub(crate) fn commands_dir() -> Option<PathBuf> {
    #[cfg(test)]
    return COMMANDS.with_borrow(Clone::clone);
    #[cfg(not(test))]
    std::env::home_dir().map(|home| home.join(".local").join("bin"))
}

#[cfg(test)]
thread_local! {
    /// Where a test's accounts' commands go; None: nowhere.
    pub(crate) static COMMANDS: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Whether `id` can be an account's command: a lowercase letter, then lowercase letters, digits,
/// `-` and `_`, at most 32 in all. Lowercase because macOS and Windows don't tell `Claude` from
/// `claude`.
pub(crate) fn is_command_word(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Rules 1 and 2 of docs/specs/accounts.md, Its name: a word, not one of Templum's.
fn word_problem(id: &str) -> Option<String> {
    if !is_command_word(id) {
        return Some("Use lowercase letters, digits, - and _, starting with a letter".into());
    }
    if Agent::from_name(id).is_some() {
        return Some(format!("{id} is one of Templum's agents"));
    }
    (id == "templum").then(|| "templum is Templum's own command".into())
}

/// Rules 4 to 6: not a word of a shell, nor a command of the computer (a program `program` finds,
/// a file of the `commands` folder) other than the one Templum wrote for `id` in `data`.
fn computer_problem(
    id: &str,
    data: Option<&Path>,
    commands: Option<&Path>,
    program: &dyn Fn(&str) -> Option<PathBuf>,
) -> Option<String> {
    if SHELL_WORDS.contains(&id) {
        return Some(format!("{id} is a word of your shell"));
    }
    let own = |path: &Path| data.is_some_and(|d| command::is_own(path, id, d));
    let kept = |dir: &Path| -> Option<PathBuf> {
        let file = |name: &std::ffi::OsStr| {
            let name = name.to_string_lossy().to_lowercase();
            name == id || (name.starts_with(&format!("{id}.")) && !name.ends_with(".templum-new"))
        };
        let found: Vec<PathBuf> = if cfg!(windows) {
            let all = std::fs::read_dir(dir).into_iter().flatten().flatten();
            all.map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(file))
                .collect()
        } else {
            vec![dir.join(id)]
        };
        found
            .into_iter()
            .find(|p| p.symlink_metadata().is_ok() && !own(p))
    };
    let taken = program(id)
        .filter(|path| !own(path))
        .or_else(|| commands.and_then(kept))?;
    Some(format!("{id} is already a command: {}", taken.display()))
}

/// What keeps `id` from being the command of an account of config.toml: rules 1, 2, 4, 5 and 6
/// of docs/specs/accounts.md, Its name. None: it can be one.
pub(crate) fn command_problem(
    id: &str,
    data: Option<&Path>,
    commands: Option<&Path>,
    program: impl Fn(&str) -> Option<PathBuf>,
) -> Option<String> {
    word_problem(id).or_else(|| computer_problem(id, data, commands, &program))
}

/// What keeps `name` from being a new account's id, the first rule broken in the order of
/// docs/specs/accounts.md, Its name. `taken`: an id of `[agents]`, in any case. `program` finds a
/// program on the computer (`templum_terminal::locate`). None: it is free.
pub(crate) fn name_problem(
    name: &str,
    taken: impl Fn(&str) -> bool,
    data: Option<&Path>,
    commands: Option<&Path>,
    program: impl Fn(&str) -> Option<PathBuf>,
) -> Option<String> {
    word_problem(name)
        .or_else(|| {
            taken(name).then(|| format!("{name} is already an agent or an account in Templum"))
        })
        .or_else(|| computer_problem(name, data, commands, &program))
        .or_else(|| {
            let left = data?.join("accounts").join(name);
            left.symlink_metadata().is_ok().then(|| {
                format!(
                    "Templum still keeps a folder for {name} ({}): choose another name",
                    tilde(&left)
                )
            })
        })
}

/// Where a program is on this computer, as agents are looked for. Tests never read the person's
/// PATH: there it finds nothing, and counts the looks in `LOOKS`.
pub(crate) fn locate(program: &str) -> Option<PathBuf> {
    #[cfg(test)]
    return {
        let _ = program;
        LOOKS.set(LOOKS.get() + 1);
        None
    };
    #[cfg(not(test))]
    templum_terminal::locate(program)
}

#[cfg(test)]
thread_local! {
    /// How many times this test's thread looked for a program: each look scans many folders.
    pub(crate) static LOOKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Makes the command of the account `entry` among `agents` in the commands folder, when its id
/// can be one and the file there is Templum's own or missing, and rewrites it when its text
/// changed. Its state: the file that runs it, or why there is none. And, apart, a failure the
/// person should be told (docs/specs/accounts.md, Its command).
pub(crate) fn make_command(
    entry: &AgentEntry,
    agents: &[AgentEntry],
    data: Option<&Path>,
) -> (Result<PathBuf, String>, Option<String>) {
    let Some(account) = &entry.account else {
        return (Err(format!("{} isn't an account", entry.title)), None);
    };
    let id = entry.id.as_str();
    let (Some(dir), Some(data)) = (commands_dir(), data) else {
        return (Err("Templum has no folder for commands".into()), None);
    };
    if let Some(why) = command_problem(id, Some(data), Some(&dir), locate) {
        return (Err(why), None);
    }
    let title = agents
        .iter()
        .find(|a| a.id == account.kind.agent)
        .map_or(account.kind.agent, |a| a.title.as_str());
    let home = std::env::home_dir();
    let command = command::Command {
        id,
        title,
        program: entry.program(),
        var: account.kind.var,
        folder: &account.folder,
        data,
        home: home.as_deref(),
    };
    match command::write(&dir, &command) {
        Ok(_) => (
            Ok(dir.join(if cfg!(windows) {
                format!("{id}.cmd")
            } else {
                id.to_string()
            })),
            None,
        ),
        Err(e) => (
            Err(e.to_string()),
            Some(format!("Couldn't make the command {id}: {e}")),
        ),
    }
}

/// Where an account's agent keeps its files.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Home {
    /// The account's id.
    pub id: String,
    /// Its agent's id.
    pub agent: String,
    /// Its state folder.
    pub state: PathBuf,
}

/// The folders whose plan limits Templum reads (`templum_usage::LIMITED`'s agents): each agent's
/// own, then its accounts'. A folder whose `sessions` are another's too (a share of them, either
/// way) has `sessions_shared`.
pub(crate) fn usage_homes(agents: &[AgentEntry]) -> Vec<templum_usage::AgentHome> {
    let shares_sessions = |id: &str| {
        agents
            .iter()
            .filter_map(|a| a.account.as_ref().map(|acc| (a, acc)))
            .any(|(a, acc)| {
                let to = acc.share.get("sessions");
                a.id == id && to.is_some() || to.is_some_and(|t| t == id)
            })
    };
    let own = templum_usage::LIMITED.iter().filter_map(|agent| {
        let kind = templum_usage::accounts::kind(agent)?;
        Some(templum_usage::AgentHome {
            id: agent.to_string(),
            agent: agent.to_string(),
            dir: own_state(kind, agents)?,
            sessions_shared: shares_sessions(agent),
        })
    });
    let accounts = agents.iter().filter_map(|a| {
        let account = a.account.as_ref()?;
        templum_usage::LIMITED
            .contains(&account.kind.agent)
            .then(|| templum_usage::AgentHome {
                id: a.id.clone(),
                agent: account.kind.agent.to_string(),
                dir: account.kind.state_dir(&account.folder),
                sessions_shared: shares_sessions(&a.id),
            })
    });
    own.chain(accounts).collect()
}

/// The agent `found` in a block whose shell has the account `account`'s variable: that account
/// when it is the account's agent, else what was found.
pub(crate) fn in_block(found: Option<&str>, account: Option<&Home>) -> Option<String> {
    match account {
        Some(home) if found == Some(home.agent.as_str()) => Some(home.id.clone()),
        _ => found.map(str::to_string),
    }
}

/// The account of `agent` whose folder is `folder` (a variable found in an agent's process),
/// among `homes`: compared as `effective` writes a folder, by its components.
pub(crate) fn account_of(agent: &str, folder: &Path, homes: &[Home]) -> Option<String> {
    let state =
        templum_usage::accounts::kind(agent)?.state_dir(&folder.components().collect::<PathBuf>());
    homes
        .iter()
        .find(|h| h.agent == agent && h.state == state)
        .map(|h| h.id.clone())
}

/// What `TEMPLUM_ACCOUNTS` holds (docs/specs/accounts.md, Typed in a terminal): a line per
/// account, `id`, program, variable and folder separated by tabs, raw. Each shell defines a
/// function per id, so an id that would break one (a word of the shell, one of Templum's own)
/// and a folder with a tab or a line break are left out.
pub(crate) fn accounts_env(agents: &[AgentEntry]) -> String {
    agents
        .iter()
        .filter_map(|a| {
            let (var, folder) = a.var()?;
            let folder = folder.to_string_lossy();
            let plain = !folder.contains(['\t', '\n', '\r']);
            (plain && command_problem(&a.id, None, None, |_| None).is_none())
                .then(|| format!("{}\t{}\t{var}\t{folder}", a.id, a.program()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Each account's [`Home`].
pub(crate) fn homes(agents: &[AgentEntry]) -> Vec<Home> {
    agents
        .iter()
        .filter_map(|a| {
            let account = a.account.as_ref()?;
            Some(Home {
                id: a.id.clone(),
                agent: account.kind.agent.to_string(),
                state: account.kind.state_dir(&account.folder),
            })
        })
        .collect()
}

/// Templum's command to write a commit message with one of its agents, and its models (the
/// recommended one first): Claude Code's and Codex's, whose models are its own list, `codex`,
/// which changes by itself. The others have none until the person sets one.
pub(crate) fn preset(agent: Agent, codex: &[String]) -> (Option<&'static str>, Vec<String>) {
    match agent.spec().id {
        // No tools and none of the person's settings (their hooks, plugins, MCP servers, which
        // turn the request into a session that reads and runs things): a one-shot answer. `=`:
        // an empty value PowerShell 5.1 would drop as `""`.
        "claude" => (
            Some("claude -p --model {model} --tools= --setting-sources="),
            ["haiku", "sonnet", "opus"].map(String::from).to_vec(),
        ),
        "codex" => (
            Some(
                "codex exec -m {model} --sandbox read-only --ephemeral --skip-git-repo-check \
                 -o {output_file} -",
            ),
            codex.to_vec(),
        ),
        _ => (None, Vec::new()),
    }
}

/// Codex's models as its own picker lists them: `~/.codex/models_cache.json`'s entries with
/// `"visibility": "list"`, by priority. None when Codex hasn't written it.
pub(crate) fn codex_models() -> Vec<String> {
    std::env::home_dir()
        .and_then(|home| std::fs::read_to_string(home.join(".codex/models_cache.json")).ok())
        .map_or_else(Vec::new, |json| codex_list(&json))
}

fn codex_list(json: &str) -> Vec<String> {
    #[derive(Deserialize)]
    struct Cache {
        models: Vec<Model>,
    }
    #[derive(Deserialize)]
    struct Model {
        slug: String,
        #[serde(default)]
        visibility: String,
        #[serde(default)]
        priority: i64,
    }
    let Ok(cache) = serde_json::from_str::<Cache>(json) else {
        return Vec::new();
    };
    let mut listed: Vec<Model> = cache
        .models
        .into_iter()
        .filter(|m| m.visibility == "list")
        .collect();
    listed.sort_by_key(|m| m.priority);
    listed.into_iter().map(|m| m.slug).collect()
}

/// What the settings dialog edits.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Draft {
    pub name: String,
    pub command: String,
    /// Empty: a new session on restore.
    pub resume: String,
    pub show: bool,
    /// Its generate command; empty: none (one of Templum's: its own).
    pub generate: String,
    /// Its models, one per line, the recommended one first; empty: none (one of Templum's: its
    /// own).
    pub models: String,
    /// The label of the shell it opens in; None: the default shell.
    pub shell: Option<String>,
}

impl Draft {
    pub fn of(entry: &AgentEntry) -> Draft {
        Draft {
            name: entry.title.clone(),
            command: entry.command.clone(),
            resume: entry.resume.clone().unwrap_or_default(),
            show: entry.show,
            generate: entry.generate.clone().unwrap_or_default(),
            models: entry.models.join("\n"),
            shell: entry.shell.clone(),
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
    /// its own (None when nothing does), everything for one of the user's. `codex`: Codex's own
    /// list of models, its default.
    pub fn to_config(&self, builtin: Option<Agent>, codex: &[String]) -> Option<AgentConfig> {
        let (name, command, resume) = (self.name.trim(), self.command.trim(), self.resume.trim());
        let generate = self.generate.trim();
        let shell = self
            .shell
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let models: Vec<String> = self
            .models
            .lines()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .collect();
        let config = match builtin {
            Some(agent) => {
                let default_resume = agent.spec().resume.unwrap_or("");
                let (default_generate, default_models) = preset(agent, codex);
                AgentConfig {
                    name: (name != agent.title()).then(|| name.to_string()),
                    command: (command != agent.command()).then(|| command.to_string()),
                    resume: (resume != default_resume).then(|| resume.to_string()),
                    show: (!self.show).then_some(false),
                    generate: (!generate.is_empty() && Some(generate) != default_generate)
                        .then(|| generate.to_string()),
                    models: (!models.is_empty() && models != default_models).then_some(models),
                    shell,
                    ..AgentConfig::default()
                }
            }
            None => AgentConfig {
                name: Some(name.to_string()),
                command: Some(command.to_string()),
                resume: (!resume.is_empty()).then(|| resume.to_string()),
                show: (!self.show).then_some(false),
                generate: (!generate.is_empty()).then(|| generate.to_string()),
                models: (!models.is_empty()).then_some(models),
                shell,
                ..AgentConfig::default()
            },
        };
        (config != AgentConfig::default()).then_some(config)
    }
}

/// `path` with the home folder as `~`, as a person types it.
pub(crate) fn tilde(path: &Path) -> String {
    match std::env::home_dir().and_then(|h| path.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// What the account dialog edits (docs/specs/accounts.md, The dialog).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AccountDraft {
    /// Its agent's id.
    pub of: String,
    pub name: String,
    pub show: bool,
    /// The folder the person chose, as typed; None: Templum's, `<data>/accounts/<id>`.
    pub folder: Option<String>,
    /// Its items shared, by key: the agent's id or another account of it.
    pub share: BTreeMap<String, String>,
    /// The label of the shell it opens in; None: the default shell.
    pub shell: Option<String>,
}

impl AccountDraft {
    pub fn of(entry: &AgentEntry) -> AccountDraft {
        let account = entry.account.as_ref();
        AccountDraft {
            of: account.map_or_else(String::new, |a| a.kind.agent.to_string()),
            name: if account.is_some_and(|a| a.named) {
                entry.title.clone()
            } else {
                String::new()
            },
            show: entry.show,
            folder: account.filter(|a| a.chosen).map(|a| tilde(&a.folder)),
            share: account.map(|a| a.share.clone()).unwrap_or_default(),
            shell: entry.shell.clone(),
        }
    }

    /// What `[agents.<id>]` holds for it.
    pub fn to_config(&self) -> AgentConfig {
        let name = self.name.trim();
        AgentConfig {
            of: Some(self.of.clone()),
            name: (!name.is_empty()).then(|| name.to_string()),
            show: (!self.show).then_some(false),
            folder: self
                .folder
                .as_deref()
                .map(str::trim)
                .filter(|f| !f.is_empty())
                .map(str::to_string),
            share: (!self.share.is_empty()).then(|| self.share.clone()),
            shell: self.shell.clone(),
            ..AgentConfig::default()
        }
    }

    /// Why it can't be saved as the account `id` among `agents` (config.toml's), with Templum's
    /// data folder `data`, if it can't.
    pub fn problem(
        &self,
        id: &str,
        agents: &BTreeMap<String, AgentConfig>,
        data: Option<&Path>,
    ) -> Option<String> {
        let mut agents = agents.clone();
        agents.insert(id.to_string(), self.to_config());
        crate::config::check_account(id, &agents, data).err()
    }
}

/// `config` as the value of `agents.<id>` in config.toml.
pub(crate) fn to_value(config: &AgentConfig) -> toml_edit::InlineTable {
    let mut table = toml_edit::InlineTable::new();
    for (key, value) in [
        ("of", &config.of),
        ("name", &config.name),
        ("command", &config.command),
        ("resume", &config.resume),
        ("generate", &config.generate),
        ("folder", &config.folder),
        ("shell", &config.shell),
    ] {
        if let Some(value) = value {
            table.insert(key, value.as_str().into());
        }
    }
    if let Some(show) = config.show {
        table.insert("show", show.into());
    }
    if let Some(models) = &config.models {
        let list: toml_edit::Array = models.iter().map(String::as_str).collect();
        table.insert("models", list.into());
    }
    if let Some(share) = &config.share {
        let mut shared = toml_edit::InlineTable::new();
        for (key, to) in share {
            shared.insert(key, to.as_str().into());
        }
        table.insert("share", shared.into());
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

/// The agent Generate ✨ runs, as `[git.commit_message]` chose it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CommitAgent {
    pub id: String,
    pub title: String,
    /// Its generate command.
    pub command: String,
    /// Its models, the recommended one first.
    pub models: Vec<String>,
    /// The model asked for: the one chosen, else the recommended one ("" when it has none).
    pub model: String,
    /// Its program is installed, and a model is known when its command needs one: Generate
    /// shows.
    pub ready: bool,
    /// An account's variable, naming its folder: the message is written on its plan.
    pub env: Option<(String, String)>,
}

impl CommitAgent {
    /// The program its command runs, to look for on this computer.
    pub fn program(&self) -> &str {
        self.command.split_whitespace().next().unwrap_or_default()
    }
}

/// The agent `settings` chose among `agents` (none chosen: the first with a generate command
/// whose program is installed, else the first with one), with its model. None when that agent has no generate command or is gone: Generate never
/// sends a diff to another agent than the one chosen. `installed`: whether a program can run
/// (`templum_terminal::installed`).
pub(crate) fn commit_agent(
    agents: &[AgentEntry],
    settings: &CommitMessage,
    installed: impl Fn(&str) -> bool,
) -> Option<CommitAgent> {
    let agent = match settings.agent.as_deref() {
        Some(id) => agents.iter().find(|a| a.id == id)?,
        None => {
            let mut writers = agents.iter().filter(|a| a.generate.is_some());
            let program = |a: &AgentEntry| {
                let generate = a.generate.as_deref().unwrap_or_default();
                installed(generate.split_whitespace().next().unwrap_or_default())
            };
            writers
                .clone()
                .find(|a| program(a))
                .or_else(|| writers.next())?
        }
    };
    let command = agent.generate.clone()?;
    let model = settings
        .model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .or_else(|| agent.models.first().cloned())
        .unwrap_or_default();
    let mut chosen = CommitAgent {
        id: agent.id.clone(),
        title: agent.title.clone(),
        command,
        models: agent.models.clone(),
        model,
        ready: false,
        env: agent
            .var()
            .map(|(name, folder)| (name.to_string(), folder.to_string_lossy().into_owned())),
    };
    chosen.ready = installed(chosen.program())
        && !(chosen.model.is_empty() && chosen.command.contains("{model}"));
    Some(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;
    use std::path::{Path, PathBuf};

    fn agents(text: &str) -> Vec<AgentEntry> {
        effective(&parse(text).unwrap().agents, &[], Some(Path::new("/data")))
    }

    #[test]
    fn a_shell_is_each_entrys_own_and_drafts_keep_it() {
        let list = agents(
            "[agents.claude]\nshell = \"fish\"\n\
             [agents.mine]\nname = \"Mine\"\ncommand = \"mine\"\nshell = \"WSL\"\n\
             [agents.claude2]\nof = \"claude\"\n\
             [agents.claude3]\nof = \"claude\"\nshell = \"zsh\"\n",
        );
        let claude = by(&list, "claude");
        assert_eq!(claude.shell.as_deref(), Some("fish"));
        assert!(claude.changed);
        assert_eq!(by(&list, "mine").shell.as_deref(), Some("WSL"));
        assert_eq!(by(&list, "codex").shell, None);
        assert_eq!(
            by(&list, "claude2").shell,
            None,
            "an account's own, not its agent's"
        );
        assert_eq!(by(&list, "claude3").shell.as_deref(), Some("zsh"));
        // The switch and the dialog save the whole entry: the shell goes with it.
        let set = Draft::of(claude).to_config(claude.builtin, &[]).unwrap();
        assert_eq!(set.shell.as_deref(), Some("fish"));
        assert!(to_value(&set).to_string().contains("shell = \"fish\""));
        let default = Draft {
            shell: None,
            ..Draft::of(claude)
        };
        assert_eq!(
            default.to_config(claude.builtin, &[]),
            None,
            "Default is Templum's: no entry"
        );
        let mine = by(&list, "mine");
        let set = Draft::of(mine).to_config(None, &[]).unwrap();
        assert_eq!(set.shell.as_deref(), Some("WSL"));
        let account = AccountDraft::of(by(&list, "claude3")).to_config();
        assert_eq!(account.shell.as_deref(), Some("zsh"));
    }

    #[test]
    fn an_account_runs_its_agents_command() {
        let text = concat!(
            "[agents]\n",
            "claude = { command = \"claude --model opus\" }\n",
            "claude2 = { of = \"claude\" }\n",
        );
        let list = agents(text);
        let claude2 = list.last().unwrap();
        assert_eq!(claude2.id, "claude2");
        assert_eq!(claude2.command, "claude --model opus");
        assert_eq!(
            claude2.resume_command(),
            by(&list, "claude").resume_command()
        );
        assert_eq!(claude2.builtin, Some(Agent::CLAUDE));
        assert_eq!(claude2.title, "Claude Code (claude2)");
        assert!(!claude2.changed);
        let account = claude2.account.as_ref().unwrap();
        assert_eq!(account.folder, Path::new("/data/accounts/claude2"));
        assert!(!account.chosen);
        let none = effective(&parse(text).unwrap().agents, &[], None);
        assert!(
            none.iter().all(|a| a.id != "claude2"),
            "no data folder, no account"
        );
    }

    #[test]
    fn its_variable_names_its_folder() {
        let list = agents("[agents]\nclaude2 = { of = \"claude\" }\n");
        assert_eq!(
            by(&list, "claude2").var(),
            Some(("CLAUDE_CONFIG_DIR", PathBuf::from("/data/accounts/claude2")))
        );
        assert_eq!(by(&list, "claude").var(), None);
        let home = std::env::home_dir().unwrap();
        for folder in ["~/w", "~/w/"] {
            let list = agents(&format!(
                "[agents.claude2]\nof = \"claude\"\nfolder = \"{folder}\"\n"
            ));
            let (_, path) = by(&list, "claude2").var().unwrap();
            // The macOS Keychain names a login by this exact text.
            assert_eq!(path.as_os_str(), home.join("w").as_os_str(), "{folder}");
            assert!(by(&list, "claude2").account.as_ref().unwrap().chosen);
        }
    }

    #[test]
    fn its_shares_point_at_their_sources() {
        let list = agents(concat!(
            "[agents]\n",
            "claude2 = { of = \"claude\", share = { skills = \"claude\" } }\n",
            "claude3 = { of = \"claude\" }\n",
            "claude4 = { of = \"claude\", share = { skills = \"claude3\" } }\n",
        ));
        OWN_FOLDERS.set(Some(PathBuf::from("/own")));
        let own = PathBuf::from("/own/claude");
        let from = |id: &str, key: &str| {
            by(&list, id)
                .shares(&list)
                .into_iter()
                .find(|s| s.key == key)
                .unwrap()
                .from
        };
        assert_eq!(from("claude2", "skills"), Some(own.clone()));
        assert_eq!(
            from("claude4", "skills"),
            Some(PathBuf::from("/data/accounts/claude3"))
        );
        assert_eq!(from("claude2", "plugins"), None, "its own");
        let sources = by(&list, "claude2").sources(&list);
        assert_eq!(
            sources,
            [
                own,
                PathBuf::from("/data/accounts/claude3"),
                PathBuf::from("/data/accounts/claude4")
            ]
        );
        let homes = homes(&list);
        assert_eq!(homes.len(), 3);
        assert_eq!(
            (
                homes[0].id.as_str(),
                homes[0].agent.as_str(),
                homes[0].state.as_path()
            ),
            ("claude2", "claude", Path::new("/data/accounts/claude2"))
        );
    }

    #[test]
    fn in_block_names_the_account() {
        let home = Home {
            id: "claude2".into(),
            agent: "claude".into(),
            state: PathBuf::from("/data/accounts/claude2"),
        };
        assert_eq!(
            in_block(Some("claude"), Some(&home)).as_deref(),
            Some("claude2")
        );
        assert_eq!(
            in_block(Some("codex"), Some(&home)).as_deref(),
            Some("codex")
        );
        assert_eq!(in_block(None, Some(&home)), None);
        assert_eq!(in_block(Some("claude"), None).as_deref(), Some("claude"));
    }

    #[test]
    fn each_folder_whose_limits_are_read_is_a_home() {
        OWN_FOLDERS.set(Some(PathBuf::from("/own")));
        let list = agents(concat!(
            "[agents]\n",
            "claude2 = { of = \"claude\" }\n",
            "codex2 = { of = \"codex\", share = { sessions = \"codex\" } }\n",
            "codex3 = { of = \"codex\" }\n",
            "gemini2 = { of = \"gemini\" }\n",
        ));
        let homes = usage_homes(&list);
        let ids: Vec<&str> = homes.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["claude", "codex", "claude2", "codex2", "codex3"]);
        let shared = |id: &str| homes.iter().find(|h| h.id == id).unwrap().sessions_shared;
        assert!(
            shared("codex") && shared("codex2"),
            "one sessions/ for both"
        );
        assert!(!shared("codex3") && !shared("claude"));
        assert_eq!(
            homes[2].dir,
            Path::new("/data/accounts/claude2"),
            "an account's own state folder"
        );
    }

    #[test]
    fn generate_runs_on_the_accounts_folder() {
        let list = agents("[agents]\nclaude2 = { of = \"claude\" }\n");
        let chose = |agent: &str| CommitMessage {
            agent: Some(agent.to_string()),
            ..CommitMessage::default()
        };
        let account = commit_agent(&list, &chose("claude2"), |_| true).unwrap();
        let (var, dir) = account.env.clone().unwrap();
        assert_eq!(var, "CLAUDE_CONFIG_DIR");
        // As a path: Windows spells it with backslashes.
        assert_eq!(Path::new(&dir), Path::new("/data/accounts/claude2"));
        assert_eq!(
            account.command,
            by(&list, "claude").generate.clone().unwrap()
        );
        assert_eq!(
            commit_agent(&list, &chose("claude"), |_| true).unwrap().env,
            None
        );
    }

    #[test]
    fn an_account_draft_round_trips() {
        let draft = AccountDraft {
            of: "claude".into(),
            name: "Claude Code 2".into(),
            show: false,
            folder: Some("~/work".into()),
            share: [("skills", "claude"), ("output-styles", "claude")]
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .into(),
            shell: Some("zsh".into()),
        };
        let config = draft.to_config();
        let text = format!("[agents]\nclaude2 = {}\n", to_value(&config));
        assert_eq!(parse(&text).unwrap().agents["claude2"], config, "{text}");
        let list = effective(&parse(&text).unwrap().agents, &[], Some(Path::new("/data")));
        assert_eq!(AccountDraft::of(by(&list, "claude2")), draft);
        let own = AccountDraft {
            folder: None,
            share: BTreeMap::new(),
            ..draft
        };
        let config = own.to_config();
        assert_eq!(
            (config.folder, config.share),
            (None, None),
            "Templum's folder, all its own"
        );
    }

    #[test]
    fn a_draft_with_a_cycle_is_refused() {
        let claude3 = AgentConfig {
            of: Some("claude".into()),
            share: Some([("skills".to_string(), "claude4".to_string())].into()),
            ..AgentConfig::default()
        };
        let agents: BTreeMap<String, AgentConfig> = [("claude3".to_string(), claude3)].into();
        let draft = AccountDraft {
            of: "claude".into(),
            name: "Claude Code 4".into(),
            show: true,
            folder: None,
            share: [("skills".to_string(), "claude3".to_string())].into(),
            shell: None,
        };
        let problem = draft.problem("claude4", &agents, None).unwrap();
        assert!(problem.contains("shares it back"), "{problem}");
        let fine = AccountDraft {
            share: BTreeMap::new(),
            ..draft
        };
        assert_eq!(fine.problem("claude4", &agents, None), None);
    }

    #[test]
    fn a_variable_naming_an_accounts_folder_isnt_the_agents_own() {
        let list = agents("[agents]\nclaude2 = { of = \"claude\" }\n");
        let claude = templum_usage::accounts::kind("claude").unwrap();
        let home = Path::new("/home/me");
        // Templum started in claude2's block has its variable.
        let var = Some("/data/accounts/claude2/".into());
        assert_eq!(
            own_state_in(claude, var, Some(home), &list),
            Some(home.join(".claude"))
        );
        let var = Some("/elsewhere".into());
        assert_eq!(
            own_state_in(claude, var, Some(home), &list),
            Some(PathBuf::from("/elsewhere"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_accounts_shared_from_are_applied_first() {
        let (own, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        OWN_FOLDERS.set(Some(own.path().to_path_buf()));
        let text = concat!(
            "[agents]\n",
            "claude3 = { of = \"claude\", share = { skills = \"claude\", settings = \"claude4\" } }\n",
            "claude4 = { of = \"claude\", share = { skills = \"claude3\" } }\n",
        );
        let list = effective(&parse(text).unwrap().agents, &[], Some(data.path()));
        let report = by(&list, "claude4").apply(&list, When::Starting);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        let accounts = data.path().join("accounts");
        assert_eq!(
            std::fs::read_link(accounts.join("claude3/skills")).unwrap(),
            own.path().join("claude/skills"),
            "claude3 first: its skills a link, not an empty folder"
        );
        assert_eq!(
            std::fs::read_link(accounts.join("claude4/skills")).unwrap(),
            accounts.join("claude3/skills")
        );
    }

    #[test]
    fn a_name_must_be_a_lowercase_word() {
        let rule = "Use lowercase letters, digits, - and _, starting with a letter";
        let long = "a".repeat(33);
        for bad in [
            "Claudinho",
            "claude inho",
            "1claude",
            "-x",
            "",
            "clau$de",
            &long,
        ] {
            let problem = name_problem(bad, |_| false, None, None, |_| None);
            assert_eq!(problem.as_deref(), Some(rule), "{bad}");
        }
        for good in ["claudinho", "claudio", "work-2", "a_b", &"a".repeat(32)] {
            assert_eq!(
                name_problem(good, |_| false, None, None, |_| None),
                None,
                "{good}"
            );
        }
    }

    #[test]
    fn a_name_isnt_an_agent_a_shell_word_nor_taken() {
        let p = |n: &str| name_problem(n, |x| x == "claudio", None, None, |_| None);
        assert_eq!(
            p("claude").as_deref(),
            Some("claude is one of Templum's agents")
        );
        assert_eq!(
            p("templum").as_deref(),
            Some("templum is Templum's own command")
        );
        assert_eq!(
            p("claudio").as_deref(),
            Some("claudio is already an agent or an account in Templum")
        );
        for word in ["cd", "source", "end", "test", "coproc"] {
            assert_eq!(p(word), Some(format!("{word} is a word of your shell")));
        }
    }

    #[test]
    fn a_name_isnt_a_command_of_the_computer() {
        use templum_usage::accounts::command::{Command, write};
        let (commands, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (commands, data) = (commands.path(), data.path());
        let found = |n: &str| (n == "gh2").then(|| PathBuf::from("/usr/bin/gh2"));
        let p = |n: &str| name_problem(n, |_| false, Some(data), Some(commands), found);
        // A program found: its path.
        assert_eq!(
            p("gh2").as_deref(),
            Some("gh2 is already a command: /usr/bin/gh2")
        );
        // A file of the commands folder: its path.
        std::fs::write(commands.join("claudinho"), "#!/bin/sh\n").unwrap();
        assert_eq!(
            p("claudinho"),
            Some(format!(
                "claudinho is already a command: {}",
                commands.join("claudinho").display()
            ))
        );
        // The command another data folder wrote: an installed Templum and a --dev one.
        let folder = Path::new("/x/accounts/claudio");
        let of = |id, data| Command {
            id,
            title: "Claude Code",
            program: "claude",
            var: "CLAUDE_CONFIG_DIR",
            folder,
            data,
            home: None,
        };
        write(commands, &of("claudio", Path::new("/x"))).unwrap();
        assert!(
            p("claudio")
                .unwrap()
                .starts_with("claudio is already a command: ")
        );
        // Its own, also when the program lookup finds that very file.
        write(commands, &of("claudia", data)).unwrap();
        let finds_file = |n: &str| Some(commands.join(n)).filter(|f| f.exists());
        assert_eq!(
            name_problem("claudia", |_| false, Some(data), Some(commands), finds_file),
            None
        );
        assert_eq!(p("claudia"), None);
        if cfg!(windows) {
            std::fs::write(commands.join("claudite.exe"), "").unwrap();
            assert!(p("claudite").is_some());
        }
    }

    #[test]
    fn a_name_whose_folder_is_still_kept_is_refused() {
        let data = tempfile::tempdir().unwrap();
        let kept = data.path().join("accounts").join("claudinho");
        std::fs::create_dir_all(&kept).unwrap();
        let problem = name_problem("claudinho", |_| false, Some(data.path()), None, |_| None);
        assert_eq!(
            problem,
            Some(format!(
                "Templum still keeps a folder for claudinho ({}): choose another name",
                tilde(&kept)
            ))
        );
        assert_eq!(
            name_problem("claudio", |_| false, Some(data.path()), None, |_| None),
            None
        );
    }

    #[test]
    fn an_older_accounts_id_may_be_a_command() {
        assert_eq!(command_problem("claude2", None, None, |_| None), None);
        assert!(command_problem("Work", None, None, |_| None).is_some());
        assert_eq!(
            command_problem("test", None, None, |_| None).as_deref(),
            Some("test is a word of your shell")
        );
    }

    #[test]
    fn the_commands_folder_is_the_tests_own() {
        assert_eq!(
            commands_dir(),
            None,
            "tests never reach the person's ~/.local/bin"
        );
        let dir = tempfile::tempdir().unwrap();
        COMMANDS.set(Some(dir.path().to_path_buf()));
        assert_eq!(commands_dir().as_deref(), Some(dir.path()));
    }

    #[test]
    fn a_default_title_isnt_written_as_a_name() {
        let list = agents("[agents]\nclaudinho = { of = \"claude\" }\n");
        assert_eq!(AccountDraft::of(by(&list, "claudinho")).name, "");
        let list = agents("[agents]\nclaude2 = { of = \"claude\", name = \"Claude Code 2\" }\n");
        assert_eq!(AccountDraft::of(by(&list, "claude2")).name, "Claude Code 2");
    }

    #[test]
    fn the_folder_found_names_the_account() {
        let homes = [Home {
            id: "claudinho".into(),
            agent: "claude".into(),
            state: PathBuf::from("/data/accounts/claudinho"),
        }];
        let of = |agent, folder| account_of(agent, Path::new(folder), &homes);
        assert_eq!(
            of("claude", "/data/accounts/claudinho/").as_deref(),
            Some("claudinho")
        );
        assert_eq!(of("claude", "/home/me/x"), None);
        assert_eq!(of("codex", "/data/accounts/claudinho"), None);
    }

    #[test]
    fn an_agent_with_accounts_is_found_with_its_variable() {
        let list = agents("[agents]\nmine = { name = \"Mine\", command = \"mine\" }\n");
        assert_eq!(
            by(&list, "claude").detect().var.as_deref(),
            Some("CLAUDE_CONFIG_DIR")
        );
        assert_eq!(by(&list, "mine").detect().var, None);
    }

    #[test]
    fn each_account_is_a_line_of_templum_accounts() {
        // `test` is a word of the shell, which would break it as a function; `tab`'s folder is a full
        // path on each system, with a tab in it.
        let tab = if cfg!(windows) {
            r"C:\\with\ttab"
        } else {
            r"/with\ttab"
        };
        let text = format!(
            "[agents]\n\
             claudinho = {{ of = \"claude\" }}\n\
             Work = {{ of = \"codex\" }}\n\
             test = {{ of = \"claude\" }}\n\
             tab = {{ of = \"claude\", folder = \"{tab}\" }}\n"
        );
        let list = agents(&text);
        // The folder as the account has it (on Windows its `/data` reads `\data`).
        let folder = by(&list, "claudinho")
            .var()
            .unwrap()
            .1
            .to_string_lossy()
            .into_owned();
        assert!(folder.ends_with("claudinho"));
        assert_eq!(
            accounts_env(&list),
            format!("claudinho\tclaude\tCLAUDE_CONFIG_DIR\t{folder}")
        );
        assert_eq!(accounts_env(&agents("")), "");
        let two =
            agents("[agents]\nclaudinho = { of = \"claude\" }\ncodex2 = { of = \"codex\" }\n");
        assert_eq!(accounts_env(&two).lines().count(), 2);
    }

    #[test]
    fn accounts_arent_looked_for_in_terminals() {
        let list = agents("[agents]\nclaude2 = { of = \"claude\" }\n");
        let ids: Vec<String> = detects(&list).into_iter().map(|d| d.id).collect();
        assert!(ids.contains(&"claude".to_string()));
        assert!(!ids.contains(&"claude2".to_string()));
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
    fn a_saved_session_is_resumed_by_its_id_unless_the_user_set_the_resume() {
        let list = agents("");
        let id = Some("0d5e5f4b-1c2d");
        assert_eq!(
            by(&list, "claude").resume_command_for(id, true),
            "claude --resume 0d5e5f4b-1c2d"
        );
        assert_eq!(
            by(&list, "codex").resume_command_for(id, true),
            "codex resume 0d5e5f4b-1c2d"
        );
        assert_eq!(
            by(&list, "claude").resume_command_for(None, true),
            "claude --continue"
        );
        assert_eq!(
            by(&list, "gemini").resume_command_for(id, true),
            "gemini --resume 0d5e5f4b-1c2d"
        );
        assert_eq!(
            by(&list, "grok").resume_command_for(id, true),
            "grok --resume 0d5e5f4b-1c2d"
        );
        // Pi's opens the session, or starts it when it has none.
        assert_eq!(
            by(&list, "pi").resume_command_for(id, true),
            "pi --session-id 0d5e5f4b-1c2d"
        );
        assert_eq!(
            by(&list, "kilo").resume_command_for(id, true),
            "kilo --continue"
        );
        // Gemini dropped it (no message was sent): a new one on that id.
        assert_eq!(
            by(&list, "gemini").resume_command_for(id, false),
            "gemini --session-id 0d5e5f4b-1c2d"
        );
        // Claude Code has no file for it (never written, or deleted): a clean new one.
        assert_eq!(by(&list, "claude").resume_command_for(id, false), "claude");
        let list = agents("[agents]\nclaude = { resume = \"claude -c --fast\" }\n");
        assert_eq!(
            by(&list, "claude").resume_command_for(id, true),
            "claude -c --fast"
        );
    }

    #[test]
    fn an_agent_that_takes_a_session_id_is_started_on_one() {
        let id = "019a1f2e-7b3c-7d4e-8f5a-6b7c8d9e0f1a";
        let list = agents("");
        assert_eq!(
            by(&list, "gemini").start_command(id),
            format!("gemini --session-id {id}")
        );
        assert_eq!(by(&list, "claude").start_command(id), "claude");
        assert_eq!(
            by(&list, "copilot").start_command(id),
            format!("copilot --session-id {id}")
        );
        let list = agents(concat!(
            "[agents]\n",
            "pi = { command = \"pi --model m\" }\n",
            "grok = { command = \"wrap-grok\" }\n",
        ));
        assert_eq!(
            by(&list, "pi").start_command(id),
            format!("pi --model m --session-id {id}")
        );
        assert_eq!(
            by(&list, "grok").start_command(id),
            "wrap-grok",
            "a wrapper"
        );
    }

    #[test]
    fn only_ours_take_a_model_and_an_effort() {
        let list = agents("[agents]\nmine = { name = \"Mine\", command = \"mine\" }\n");
        assert_eq!(
            by(&list, "claude").launch_args_with(Some("opus"), None, None),
            Ok(" --model opus".to_string())
        );
        assert_eq!(
            by(&list, "mine").launch_args_with(None, None, None),
            Ok(String::new())
        );
        let err = by(&list, "mine")
            .launch_args_with(Some("m"), None, None)
            .unwrap_err();
        assert!(err.contains("Mine"), "{err}");
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
            draft.to_config(Some(claude), &[]),
            None,
            "unchanged: nothing to keep"
        );
        draft.command = " claude --model opus ".into();
        draft.show = false;
        let kept = draft.to_config(Some(claude), &[]).unwrap();
        assert_eq!(kept.command.as_deref(), Some("claude --model opus"));
        assert_eq!(
            (kept.name, kept.resume, kept.show),
            (None, None, Some(false))
        );
        let value = to_value(&draft.to_config(Some(claude), &[]).unwrap()).to_string();
        assert!(
            value.contains("command = \"claude --model opus\""),
            "{value}"
        );
        // One of the user's keeps everything it needs.
        let mine = Draft {
            name: "Mine".into(),
            command: "mine".into(),
            show: true,
            ..Draft::default()
        };
        let kept = mine.to_config(None, &[]).unwrap();
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

    #[test]
    fn claude_and_codex_write_commit_messages_and_the_others_can() {
        let codex_list = ["gpt-a".to_string(), "gpt-b".to_string()];
        let config = parse(concat!(
            "[agents.gemini]\n",
            "generate = \"gemini -p -m {model}\"\n",
            "models = [\"flash\"]\n",
        ))
        .unwrap();
        let list = effective(&config.agents, &codex_list, None);
        let claude = by(&list, "claude");
        // No tools, and none of the person's settings (their hooks, plugins and MCP servers): a
        // one-shot answer, never a session that reads or runs anything in the repository.
        assert_eq!(
            claude.generate.as_deref(),
            Some("claude -p --model {model} --tools= --setting-sources=")
        );
        assert_eq!(claude.models, ["haiku", "sonnet", "opus"]);
        let codex = by(&list, "codex");
        assert_eq!(
            codex.generate.as_deref(),
            Some(
                "codex exec -m {model} --sandbox read-only --ephemeral --skip-git-repo-check \
                 -o {output_file} -"
            )
        );
        assert_eq!(codex.models, codex_list, "Codex's own list");
        let gemini = by(&list, "gemini");
        assert_eq!(gemini.generate.as_deref(), Some("gemini -p -m {model}"));
        assert_eq!(gemini.models, ["flash"]);
        assert!(gemini.changed);
        assert_eq!(by(&list, "copilot").generate, None);
        assert!(by(&list, "copilot").models.is_empty());
    }

    #[test]
    fn codexs_models_are_its_listed_ones_by_priority() {
        let json = r#"{"fetched_at":"x","models":[
            {"slug":"hidden","visibility":"hide","priority":1},
            {"slug":"second","visibility":"list","priority":9,"description":"…"},
            {"slug":"first","visibility":"list","priority":8}]}"#;
        assert_eq!(codex_list(json), ["first", "second"]);
        assert!(codex_list("not json").is_empty());
    }

    #[test]
    fn a_changed_generate_command_or_model_list_is_kept() {
        let claude = by(&agents(""), "claude").clone();
        let mut draft = Draft::of(&claude);
        assert_eq!(draft.models, "haiku\nsonnet\nopus");
        draft.generate = "claude -p --model {model} --verbose".into();
        draft.models = " sonnet \n\nhaiku\n".into();
        let kept = draft.to_config(Some(Agent::CLAUDE), &[]).unwrap();
        assert_eq!(
            kept.generate.as_deref(),
            Some("claude -p --model {model} --verbose")
        );
        assert_eq!(
            kept.models,
            Some(vec!["sonnet".to_string(), "haiku".to_string()])
        );
        // As config.toml holds it, read back.
        let text = format!("[agents]\nclaude = {}\n", to_value(&kept));
        assert_eq!(parse(&text).unwrap().agents["claude"], kept, "{text}");
        // Codex's list is its own: kept only when the person changed it.
        let codex_list = ["gpt-a".to_string()];
        let codex = by(&effective(&BTreeMap::new(), &codex_list, None), "codex").clone();
        assert_eq!(
            Draft::of(&codex).to_config(Some(Agent::CODEX), &codex_list),
            None
        );
        let edited = Draft {
            models: "gpt-b".into(),
            ..Draft::of(&codex)
        };
        assert_eq!(
            edited
                .to_config(Some(Agent::CODEX), &codex_list)
                .unwrap()
                .models,
            Some(vec!["gpt-b".to_string()])
        );
        // Emptied: back to Templum's own.
        let emptied = Draft {
            generate: " ".into(),
            models: String::new(),
            ..Draft::of(&claude)
        };
        assert_eq!(emptied.to_config(Some(Agent::CLAUDE), &[]), None);
        // One of the user's: its own, or none.
        let mine = Draft {
            name: "Mine".into(),
            command: "mine".into(),
            generate: "mine ask {model}".into(),
            models: "fast".into(),
            show: true,
            ..Draft::default()
        };
        let kept = mine.to_config(None, &[]).unwrap();
        assert_eq!(
            (kept.generate.as_deref(), kept.models),
            (Some("mine ask {model}"), Some(vec!["fast".to_string()]))
        );
    }

    #[test]
    fn generate_runs_the_chosen_agent_with_its_recommended_model() {
        let list = agents(concat!(
            "[agents.mine]\nname = \"Mine\"\ncommand = \"mine\"\n",
            "generate = \"/opt/mine/ask {model}\"\nmodels = [\"fast\", \"smart\"]\n",
            "[agents.bare]\nname = \"Bare\"\ncommand = \"bare\"\ngenerate = \"bare {model}\"\n",
            "[agents.plain]\nname = \"Plain\"\ncommand = \"plain\"\ngenerate = \"plain -q\"\n",
        ));
        let chose = |agent: &str, model: Option<&str>| CommitMessage {
            agent: Some(agent.to_string()),
            model: model.map(str::to_string),
            ..CommitMessage::default()
        };
        let first = commit_agent(&list, &CommitMessage::default(), |_| true).unwrap();
        assert_eq!(
            (first.id.as_str(), first.model.as_str()),
            ("claude", "haiku"),
            "none chosen: the first with a command, its recommended model"
        );
        let mine = commit_agent(&list, &chose("mine", None), |p| p == "/opt/mine/ask").unwrap();
        assert_eq!((mine.model.as_str(), mine.ready), ("fast", true));
        assert_eq!(mine.program(), "/opt/mine/ask");
        let smart = commit_agent(&list, &chose("mine", Some("smart")), |_| true).unwrap();
        assert_eq!(smart.model, "smart");
        assert!(
            !commit_agent(&list, &chose("mine", None), |_| false)
                .unwrap()
                .ready,
            "not installed"
        );
        assert!(
            !commit_agent(&list, &chose("bare", None), |_| true)
                .unwrap()
                .ready,
            "a command that needs a model, and none known"
        );
        assert!(
            commit_agent(&list, &chose("plain", None), |_| true)
                .unwrap()
                .ready
        );
        assert_eq!(
            commit_agent(&list, &chose("gone", None), |_| true),
            None,
            "never another agent than the one chosen"
        );
        assert_eq!(
            commit_agent(&list, &chose("copilot", None), |_| true),
            None,
            "no generate command"
        );
    }

    #[test]
    fn none_chosen_an_installed_agent_writes_the_message() {
        let list = agents("");
        let codex = commit_agent(&list, &CommitMessage::default(), |p| p == "codex").unwrap();
        assert_eq!(codex.id, "codex", "Codex alone on this computer");
        let none = commit_agent(&list, &CommitMessage::default(), |_| false).unwrap();
        assert_eq!(
            (none.id.as_str(), none.ready),
            ("claude", false),
            "none installed: the first with a command"
        );
    }
}
