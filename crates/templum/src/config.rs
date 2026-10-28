//! config.toml: every setting, typed and validated; polling for changes; saving edits.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use templum_terminal::{CursorShape, CursorStyle, TermOptions};
pub(crate) use templum_usage::Reset;

use toml_edit::{DocumentMut, TomlError};

use crate::keys::{Keymap, KeysConfig};
use crate::theme::{DEFAULT_DARK, DEFAULT_LIGHT};

pub(crate) const FILE: &str = "config.toml";

/// How often config.toml is checked for changes.
pub(crate) const POLL: Duration = Duration::from_millis(500);

/// Everything config.toml can set. Every table and key is optional: an empty file is the
/// defaults.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    pub font: Font,
    /// `[window]` held the glass options, now removed: a file that still has it loads.
    #[serde(rename = "window")]
    pub _window: serde::de::IgnoredAny,
    pub theme: ThemeConfig,
    pub shell: Shell,
    pub cursor: Cursor,
    pub terminal: TerminalConfig,
    pub keys: KeysConfig,
    pub git: GitConfig,
    pub usage: UsageConfig,
    pub updates: UpdatesConfig,
    pub connections: ConnectionsConfig,
    pub templum_agent: TemplumAgentConfig,
    pub canvas: CanvasConfig,
    /// `[agents.<id>]`: changes to one of Templum's AI agents (only what differs), or an agent of
    /// the user's own.
    pub agents: BTreeMap<String, AgentConfig>,
    /// `[roles.<id>]`: ways to open an agent `spawn_agent` takes by name.
    pub roles: BTreeMap<String, Role>,
}

/// A saved way to open an agent (docs/specs/agent-tools.md, Roles): which agent, its model and
/// effort, words for its command line (its own flags: the tools it may use), and instructions
/// typed with its prompt.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Role {
    /// An id of Settings ▸ Agents, an account too.
    pub agent: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// One line, typed after its model and effort, as an `[agents]` command is.
    pub args: Option<String>,
    pub instructions: Option<String>,
}

impl Role {
    /// `spawn` opened as the role `id`: the call's own agent, model and effort win over the
    /// role's; its instructions go with the prompt, which they need; its words ride along.
    pub fn spawn(
        &self,
        id: &str,
        mut spawn: templum_mcp::tools::Spawn,
    ) -> Result<templum_mcp::tools::Spawn, String> {
        spawn.agent = spawn.agent.or_else(|| Some(self.agent.clone()));
        spawn.model = spawn.model.or_else(|| self.model.clone());
        spawn.effort = spawn.effort.or_else(|| self.effort.clone());
        let instructions = self.instructions.as_deref().map(str::trim);
        if let Some(instructions) = instructions.filter(|i| !i.is_empty()) {
            let Some(prompt) = spawn.prompt.as_deref() else {
                return Err(format!(
                    "The role {id}'s instructions go with a prompt: give one."
                ));
            };
            spawn.prompt = Some(format!(
                "Your role, {id}:\n{instructions}\n\nYour task:\n{prompt}"
            ));
        }
        spawn.args = self.args.clone().filter(|a| !a.trim().is_empty());
        Ok(spawn)
    }
}

/// One AI agent's settings. For Templum's own, each missing key keeps its default; one of the
/// user's needs a name and a command.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AgentConfig {
    pub name: Option<String>,
    /// Typed into the shell to start it.
    pub command: Option<String>,
    /// Typed to bring it back when the canvas is restored; empty: a new session.
    pub resume: Option<String>,
    /// Listed in the canvas's menu.
    pub show: Option<bool>,
    /// Runs it without a terminal to write a commit message: `{model}`, `{prompt_file}` (else
    /// the request is its standard input), `{output_file}` (else its standard output is the
    /// answer). See docs/specs/git.md, section 7.
    pub generate: Option<String>,
    /// Its models for commit messages, the recommended one first.
    pub models: Option<Vec<String>>,
    /// The label of the shell it opens in (docs/specs/shells.md, A shell for each agent);
    /// missing: the default shell.
    pub shell: Option<String>,
    /// An account: the agent it is another folder of (docs/specs/accounts.md).
    pub of: Option<String>,
    /// An account's folder; missing: `<data>/accounts/<id>`.
    pub folder: Option<String>,
    /// An account's items shared, by key: the agent's id or another account of it.
    pub share: Option<BTreeMap<String, String>>,
}

/// Letters, digits, `-` and `_`: an agent's id is a key in the saved canvas and in this file.
pub(crate) fn valid_agent_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Font {
    /// None: the first installed font of the built-in monospace list.
    pub family: Option<String>,
    /// Points at zoom 1.
    pub size: f32,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: None,
            size: 14.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ThemeConfig {
    pub mode: Mode,
    pub light: String,
    pub dark: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            mode: Mode::System,
            light: DEFAULT_LIGHT.into(),
            dark: DEFAULT_DARK.into(),
        }
    }
}

impl ThemeConfig {
    /// The theme name for light or dark mode.
    pub fn name(&self, dark: bool) -> &str {
        if dark { &self.dark } else { &self.light }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    #[default]
    System,
    Light,
    Dark,
}

impl Mode {
    /// Whether the dark theme is in effect, given the system's appearance.
    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            Mode::System => system_dark,
            Mode::Light => false,
            Mode::Dark => true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Shell {
    /// None: the default shell: $SHELL; on Windows, Git Bash, else PowerShell 7, else Windows
    /// PowerShell (`templum_terminal::default_shell`).
    pub program: Option<String>,
    /// Used with `program` only.
    pub args: Vec<String>,
    pub working_directory: WorkingDirectory,
    /// bash, zsh, fish and PowerShell load Templum's shell integration: they tell when commands
    /// start and end, and their folder.
    pub integration: bool,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            program: None,
            args: Vec::new(),
            working_directory: WorkingDirectory::default(),
            integration: true,
        }
    }
}

/// Where new terminals start.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(from = "String")]
pub(crate) enum WorkingDirectory {
    /// The focused block's folder (home when it has none): see `Body::folder`.
    #[default]
    Inherit,
    Home,
    Path(PathBuf),
}

impl From<String> for WorkingDirectory {
    fn from(s: String) -> Self {
        match s.as_str() {
            "inherit" => Self::Inherit,
            "~" => Self::Home,
            _ => match (s.strip_prefix("~/"), std::env::home_dir()) {
                (Some(rest), Some(home)) => Self::Path(home.join(rest)),
                _ => Self::Path(PathBuf::from(s)),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Cursor {
    pub shape: Shape,
    pub blink: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Shape {
    #[default]
    Block,
    Bar,
    Underline,
}

impl Cursor {
    pub fn style(&self) -> CursorStyle {
        CursorStyle {
            shape: match self.shape {
                Shape::Block => CursorShape::Block,
                Shape::Bar => CursorShape::Beam,
                Shape::Underline => CursorShape::Underline,
            },
            blinking: self.blink,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct TerminalConfig {
    /// Lines of history per terminal.
    pub scrollback: usize,
    /// The system's notifications too, when Templum is in the background and a long command ends
    /// or a terminal asks for attention.
    pub notifications: bool,
    /// A selection the person makes with the mouse goes to the clipboard as the button is let go,
    /// no copy shortcut needed (docs/specs/walking-skeleton.md).
    pub copy_on_select: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            scrollback: 10_000,
            notifications: true,
            copy_on_select: true,
        }
    }
}

/// `[usage]`: what Templum tells about the agents' plan limits.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct UsageConfig {
    /// When a limit's reset is notified.
    pub reset: Reset,
    /// A limit's use, in percent, that is notified once per window: 0 (never), 80, 90 or 95.
    pub warn: u8,
    /// Templum asks Claude Code for the plan's limits, and the Claude Code it opens gets
    /// Templum's status line.
    pub claude_limits: bool,
    /// The agents (ids) whose limits the title bar doesn't show.
    pub hide: Vec<String>,
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self {
            reset: Reset::default(),
            warn: 90,
            claude_limits: true,
            hide: Vec::new(),
        }
    }
}

/// `[updates]`: new versions from GitHub Releases (docs/specs/updates.md).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct UpdatesConfig {
    /// Look for a new version at start and once a day, and download it.
    pub check: bool,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self { check: true }
    }
}

/// `[connections]`: Templum's MCP server and hooks for the agents it opens or that are typed in
/// its terminals (docs/specs/connections.md).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ConnectionsConfig {
    /// The agents Templum opens, or that are typed in its terminals, get its MCP server and hooks;
    /// off, the bus answers no agent (docs/specs/connections.md, The switch reaches every agent).
    pub enabled: bool,
    /// How long an agent's question waits for its answer, minutes, 0 to 1440; 0: until answered
    /// (docs/specs/ask-user.md).
    pub question_timeout: u64,
    /// A short sound when an agent asks the person something.
    pub sound: bool,
}

impl Default for ConnectionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            question_timeout: 0,
            sound: true,
        }
    }
}

/// `[templum_agent]`: the agent the chat at the bottom of the canvas runs on
/// (docs/specs/templum-agent.md#config).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct TemplumAgentConfig {
    /// The engine's id: `claude`.
    pub agent: String,
    /// Its model; empty is the agent's own default.
    pub model: String,
    /// Its effort, one of the agent's; empty is its default.
    pub effort: String,
    /// The prompt pill at the bottom of the canvas; off, the shortcut still opens it.
    pub pill: bool,
    /// How many of its tasks may run at once.
    pub max_agents: u32,
}

impl Default for TemplumAgentConfig {
    fn default() -> Self {
        Self {
            agent: "claude".into(),
            model: String::new(),
            effort: String::new(),
            pill: true,
            max_agents: 4,
        }
    }
}

/// `[canvas]`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct CanvasConfig {
    /// The block under the pointer takes the keyboard once the pointer moves, no click needed
    /// (docs/specs/navigation.md). Grid mode's tiles always do.
    pub focus_follows_pointer: bool,
}

impl Default for CanvasConfig {
    fn default() -> Self {
        Self {
            focus_follows_pointer: true,
        }
    }
}

/// The values `[usage] warn` takes: what Settings offers.
pub(crate) const WARN_AT: [u8; 4] = [0, 80, 90, 95];

/// `[git]`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct GitConfig {
    /// The program the Git block runs: a name on PATH, or a path.
    pub program: String,
    /// `[git.commit_message]`: Generate ✨.
    pub commit_message: CommitMessage,
    /// `[git.repositories."<root>"]`: one repository's own settings, by its root as
    /// `git rev-parse --show-toplevel` prints it (`/` on every OS).
    pub repositories: BTreeMap<String, RepoConfig>,
}

impl GitConfig {
    /// The program to run: the setting without the spaces around it ("git" when empty).
    pub fn program(&self) -> &str {
        match self.program.trim() {
            "" => "git",
            program => program,
        }
    }
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            program: "git".to_string(),
            commit_message: CommitMessage::default(),
            repositories: BTreeMap::new(),
        }
    }
}

/// `[git.commit_message]`: how Generate ✨ asks an agent for a commit message.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct CommitMessage {
    /// An agent's id; None: the first agent with a generate command, an installed one first.
    pub agent: Option<String>,
    /// None: the agent's recommended model, the first of its list.
    pub model: Option<String>,
    /// Seconds the agent has to answer.
    pub timeout: u64,
    /// The diff sent is cut past this many KB.
    pub max_diff_kb: u64,
    /// The user's commit rules, sent with each request.
    pub instructions: String,
    /// The first-use question was answered: the diff may be sent.
    pub confirmed: bool,
    /// The request's template; None: `templum_git::message::PROMPT`.
    pub prompt: Option<String>,
}

impl Default for CommitMessage {
    fn default() -> Self {
        Self {
            agent: None,
            model: None,
            timeout: 60,
            max_diff_kb: 60,
            instructions: String::new(),
            confirmed: false,
            prompt: None,
        }
    }
}

/// `[git.repositories."<root>"]`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct RepoConfig {
    /// Wins over `[git.commit_message].instructions` in this repository.
    pub commit_instructions: Option<String>,
}

/// The table holding one repository's settings, `git.repositories."<key>"`, as an `Edit` names
/// it: the key quoted as TOML needs (a root has slashes, dots, spaces).
pub(crate) fn repository_table(key: &str) -> String {
    format!(
        "git.repositories.{}",
        toml_edit::Key::new(key).display_repr()
    )
}

impl Config {
    pub fn term_options(&self) -> TermOptions {
        TermOptions {
            scrollback: self.terminal.scrollback,
            cursor: self.cursor.style(),
        }
    }
}

/// `parse_in` without Templum's data folder.
#[cfg(test)]
pub(crate) fn parse(text: &str) -> Result<Config, String> {
    parse_in(text, None)
}

/// Parses and validates config.toml, with Templum's data folder `data` (accounts' folders are
/// in it). The error is ready to show to the user.
pub(crate) fn parse_in(text: &str, data: Option<&Path>) -> Result<Config, String> {
    let config: Config = toml::from_str(text).map_err(|e| {
        let (line, column) = e.span().map_or((1, 1), |s| line_col(text, s.start));
        format!("{FILE}, line {line}, column {column}: {}", e.message())
    })?;
    let range = |key: &str, ok: bool, limits: &str, value: String| {
        if ok {
            Ok(())
        } else {
            Err(format!(
                "{FILE}: {key} must be between {limits} (it is {value})"
            ))
        }
    };
    let size = config.font.size;
    range(
        "font.size",
        (6.0..=72.0).contains(&size),
        "6 and 72",
        size.to_string(),
    )?;
    let scrollback = config.terminal.scrollback;
    range(
        "terminal.scrollback",
        scrollback <= 100_000,
        "0 and 100000",
        scrollback.to_string(),
    )?;
    let question = config.connections.question_timeout;
    range(
        "connections.question_timeout",
        question <= 1440,
        "0 and 1440",
        question.to_string(),
    )?;
    let agent = &config.templum_agent;
    if agent.agent != "claude" {
        return Err(format!(
            "{FILE}: templum_agent.agent is claude (it is {:?})",
            agent.agent
        ));
    }
    let claude = templum_terminal::Agent::from_name("claude").expect("Claude Code");
    fn blank(v: &str) -> Option<&str> {
        (!v.is_empty()).then_some(v)
    }
    if let Err(why) = claude.launch_args(blank(&agent.model), None) {
        return Err(format!("{FILE}: templum_agent.model: {why}"));
    }
    if let Err(why) = claude.launch_args(None, blank(&agent.effort)) {
        return Err(format!("{FILE}: templum_agent.effort: {why}"));
    }
    range(
        "templum_agent.max_agents",
        (1..=16).contains(&agent.max_agents),
        "1 and 16",
        agent.max_agents.to_string(),
    )?;
    let message = &config.git.commit_message;
    range(
        "git.commit_message.timeout",
        (5..=600).contains(&message.timeout),
        "5 and 600",
        message.timeout.to_string(),
    )?;
    range(
        "git.commit_message.max_diff_kb",
        (4..=1024).contains(&message.max_diff_kb),
        "4 and 1024",
        message.max_diff_kb.to_string(),
    )?;
    let warn = config.usage.warn;
    if !WARN_AT.contains(&warn) {
        return Err(format!(
            "{FILE}: usage.warn must be 0, 80, 90 or 95 (it is {warn})"
        ));
    }
    for (id, role) in &config.roles {
        if !valid_agent_id(id) {
            return Err(format!(
                "{FILE}: roles.{id:?}: a role's id is letters, digits, - and _"
            ));
        }
        if role.agent.trim().is_empty() {
            return Err(format!(
                "{FILE}: roles.{id}.agent is needed (the agent it opens)"
            ));
        }
        if role
            .args
            .as_deref()
            .is_some_and(|a| a.contains(['\n', '\r']))
        {
            return Err(format!("{FILE}: roles.{id}.args is one line"));
        }
    }
    // Shortcuts are part of the file's validity: an invalid one keeps the last valid config.
    Keymap::new(&config.keys, cfg!(target_os = "macos"))?;
    for (id, agent) in &config.agents {
        if !valid_agent_id(id) {
            return Err(format!(
                "{FILE}: agents.{id:?}: an agent's id is letters, digits, - and _"
            ));
        }
        let ours = templum_terminal::Agent::from_name(id).is_some();
        if agent.of.is_some() || agent.folder.is_some() || agent.share.is_some() {
            check_account(id, &config.agents, data)?;
            continue;
        }
        let blank = |v: &Option<String>| v.as_deref().is_none_or(|v| v.trim().is_empty());
        if (!ours || agent.command.is_some()) && blank(&agent.command) {
            return Err(format!(
                "{FILE}: agents.{id}.command is needed (what starts it)"
            ));
        }
        if !ours && blank(&agent.name) {
            return Err(format!(
                "{FILE}: agents.{id}.name is needed (what menus show)"
            ));
        }
        if agent
            .generate
            .as_deref()
            .is_some_and(|g| g.trim().is_empty())
        {
            return Err(format!(
                "{FILE}: agents.{id}.generate can't be empty (remove it for none)"
            ));
        }
    }
    Ok(config)
}

/// The account `id` of `agents` (an entry with `of`), checked: its agent has accounts, no other
/// account's id is its own but for case, it takes nothing its agent has, each share names an item
/// and one of its agent's folders without coming back to it, and its folder is no agent's own,
/// not the home folder, apart from every other account's, and neither in nor around Templum's
/// folder for accounts (`<data>/accounts`, whose folders are deleted with their account). Folders
/// are compared with their links followed (`resolved`).
pub(crate) fn check_account(
    id: &str,
    agents: &BTreeMap<String, AgentConfig>,
    data: Option<&Path>,
) -> Result<(), String> {
    let account = &agents[id];
    let of = account.of.as_deref().unwrap_or_default();
    let kind = templum_usage::accounts::kind(of)
        .filter(|_| templum_terminal::Agent::from_name(id).is_none());
    let Some(kind) = kind else {
        let ids: Vec<&str> = templum_usage::accounts::kinds()
            .iter()
            .map(|k| k.agent)
            .collect();
        return Err(format!(
            "{FILE}: agents.{id}.of is an agent with accounts ({}), and an account's id isn't one of Templum's agents",
            ids.join(", ")
        ));
    };
    // One folder on macOS and Windows: deleting one would delete the other's login.
    let twin = agents
        .iter()
        .find(|(other, a)| *other != id && other.eq_ignore_ascii_case(id) && a.of.is_some());
    if let Some((other, _)) = twin {
        return Err(format!(
            "{FILE}: agents.{id}: {other} is the same id but for case, one folder on macOS and Windows: rename one"
        ));
    }
    for (key, set) in [
        ("command", account.command.is_some()),
        ("resume", account.resume.is_some()),
        ("generate", account.generate.is_some()),
        ("models", account.models.is_some()),
    ] {
        if set {
            return Err(format!(
                "{FILE}: agents.{id}.{key}: an account runs its agent's command"
            ));
        }
    }
    let of_same = |other: &str| {
        agents
            .get(other)
            .is_some_and(|a| a.of.as_deref() == Some(of))
    };
    for (key, target) in account.share.iter().flatten() {
        if kind.item(key).is_none() {
            let keys: Vec<&str> = kind.items.iter().map(|i| i.key).collect();
            return Err(format!(
                "{FILE}: agents.{id}.share.{key}: not one of {of}'s items ({})",
                keys.join(", ")
            ));
        }
        if target != of && !of_same(target) {
            return Err(format!(
                "{FILE}: agents.{id}.share.{key}: {target} is neither {of} nor one of its accounts"
            ));
        }
        // Following the key from account to account ends at the agent's own folder.
        let mut seen = vec![id];
        let mut at = target.as_str();
        while at != of {
            if seen.contains(&at) {
                return Err(format!(
                    "{FILE}: agents.{id}.share.{key}: {target} shares it back"
                ));
            }
            seen.push(at);
            match agents[at].share.as_ref().and_then(|s| s.get(key)) {
                Some(next) if next == of || of_same(next) => at = next,
                _ => break,
            }
        }
    }
    if let Some(folder) = account.folder.as_deref().filter(|f| !f.trim().is_empty()) {
        let path = crate::launcher::expand(folder);
        // A relative one would be each block's working folder's.
        if !path.is_absolute() {
            let start = if cfg!(windows) {
                "C:\\ or ~\\"
            } else {
                "/ or ~/"
            };
            return Err(format!(
                "{FILE}: agents.{id}.folder: {folder} isn't a full path: start it with {start}"
            ));
        }
        let refuse = |why: String| Err(format!("{FILE}: agents.{id}.folder: {folder} {why}"));
        let real = resolved(&path);
        let home = std::env::home_dir();
        if home.as_deref().is_some_and(|home| resolved(home) == real) {
            return refuse("is your home folder: choose a folder of its own".to_string());
        }
        if let Some(accounts) = data.map(|data| resolved(&data.join("accounts"))) {
            if real.starts_with(&accounts) {
                return refuse("is Templum's folder for accounts: leave Folder empty".to_string());
            }
            if accounts.starts_with(&real) {
                return refuse(
                    "holds Templum's folder for accounts: choose a folder of its own".to_string(),
                );
            }
        }
        let state = resolved(&kind.state_dir(&path));
        for other in templum_usage::accounts::kinds() {
            let own = other.own_folder(std::env::var_os(other.var), home.as_deref());
            if own.is_some_and(|own| resolved(&other.state_dir(&own)) == state) {
                return refuse(format!("is {}'s own folder", other.agent));
            }
        }
        for (other, a) in agents
            .iter()
            .filter(|(other, a)| *other != id && a.of.is_some())
        {
            let Some(theirs) = a.folder.as_deref().filter(|f| !f.trim().is_empty()) else {
                continue;
            };
            let theirs = resolved(&crate::launcher::expand(theirs));
            if real == theirs {
                return refuse(format!("is {other}'s folder"));
            }
            if real.starts_with(&theirs) {
                return refuse(format!("is in {other}'s folder: choose one apart"));
            }
            if theirs.starts_with(&real) {
                return refuse(format!("holds {other}'s folder: choose one apart"));
            }
        }
    }
    Ok(())
}

/// `path` with its links followed as far as it exists (the rest as written), to compare folders:
/// a link's target and the link, a case variant on macOS and Windows, a `\\?\` path. A `..` after
/// a folder that isn't there is the folder before it, as once it is made.
pub(crate) fn resolved(path: &Path) -> PathBuf {
    let follow = |path: &Path| {
        path.ancestors()
            .find_map(|a| Some(a.canonicalize().ok()?.join(path.strip_prefix(a).ok()?)))
            .unwrap_or_else(|| path.to_path_buf())
    };
    // What exists is followed first, so a `..` after a link is the OS's.
    let mut plain = PathBuf::new();
    for part in follow(path).components() {
        match part {
            Component::ParentDir => {
                plain.pop();
            }
            Component::CurDir => {}
            part => plain.push(part),
        }
    }
    // Back up, it may be an existing folder again (`<data>/nothing/../accounts`).
    follow(&plain)
}

/// `<dir>/config.toml`, with Templum's data folder `data`. No folder or no file: the defaults.
pub(crate) fn load(dir: Option<&Path>, data: Option<&Path>) -> Result<Config, String> {
    let Some(path) = dir.map(|d| d.join(FILE)) else {
        return Ok(Config::default());
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_in(&text, data),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
    }
}

/// What polling compares: config.toml's modification time and size (None: no file). Metadata
/// follows symlinks, so a config.toml linked from a dotfiles repository works too.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Stamp(Option<(SystemTime, u64)>);

impl Stamp {
    pub fn of(dir: Option<&Path>) -> Stamp {
        dir.map_or(Stamp(None), |d| Stamp::file(&d.join(FILE)))
    }

    /// The same for any file: a theme's.
    pub fn file(path: &Path) -> Stamp {
        Stamp(
            std::fs::metadata(path)
                .ok()
                .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len())),
        )
    }
}

/// Templum's config folder: `$XDG_CONFIG_HOME/templum` or `~/.config/templum` on Linux and macOS
/// (the convention of terminal apps), `%APPDATA%\templum` on Windows.
pub(crate) fn dir() -> Option<PathBuf> {
    dir_from(
        cfg!(windows),
        std::env::var_os("APPDATA"),
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::home_dir(),
    )
}

fn dir_from(
    windows: bool,
    appdata: Option<OsString>,
    xdg: Option<OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let base = if windows {
        appdata.map(PathBuf::from)
    } else {
        xdg.filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))
    };
    base.map(|b| b.join("templum"))
}

/// The 1-based line a parse error points at.
pub(crate) fn line_of(text: &str, err: &TomlError) -> usize {
    line_col(text, err.span().map_or(0, |s| s.start)).0
}

/// 1-based line and column of byte `at`.
fn line_col(text: &str, at: usize) -> (usize, usize) {
    let before = &text.as_bytes()[..at.min(text.len())];
    let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
    let column = before.iter().rev().take_while(|&&b| b != b'\n').count() + 1;
    (line, column)
}

#[derive(Debug)]
pub(crate) enum SaveError {
    /// No config folder could be found (no HOME, no APPDATA).
    NoDir,
    /// The file doesn't parse: it is left alone.
    Broken {
        line: usize,
    },
    /// `table` exists in the file but isn't a table (`theme = "Dracula"`), or a key on its path
    /// isn't.
    NotATable(String),
    Io(io::Error),
}

impl SaveError {
    /// The message, naming the file written (`file`: config.toml, a theme's file).
    pub fn describe(&self, file: &str) -> String {
        match self {
            Self::NoDir => "no config folder".to_string(),
            Self::Broken { line } => format!("{file} has an error on line {line}"),
            Self::NotATable(table) => format!("`{table}` in {file} is not a table"),
            Self::Io(e) => e.to_string(),
        }
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe(FILE))
    }
}

/// One change to config.toml: `table.key` set to a value, or removed (back to its default).
#[derive(Clone, Debug)]
pub(crate) struct Edit {
    /// A TOML key: `font`, or a table inside another, `git.commit_message`,
    /// `git.repositories."/home/u/proj"` (see `repository_table`).
    pub table: Cow<'static, str>,
    /// Fixed for a setting; an agent's id under `[agents]`.
    pub key: Cow<'static, str>,
    pub value: Option<toml_edit::Value>,
}

impl Edit {
    pub fn set(
        table: impl Into<Cow<'static, str>>,
        key: impl Into<Cow<'static, str>>,
        value: impl Into<toml_edit::Value>,
    ) -> Self {
        Self {
            table: table.into(),
            key: key.into(),
            value: Some(value.into()),
        }
    }

    pub fn remove(table: impl Into<Cow<'static, str>>, key: impl Into<Cow<'static, str>>) -> Self {
        Self {
            table: table.into(),
            key: key.into(),
            value: None,
        }
    }
}

/// The table `path` names (a TOML key, see `Edit::table`), made when missing with `make` (a
/// parent made on the way is implicit: `[git.commit_message]` gets no empty `[git]` line before
/// it), else None.
fn table_at<'d>(
    doc: &'d mut DocumentMut,
    path: &str,
    make: bool,
) -> Result<Option<&'d mut dyn toml_edit::TableLike>, SaveError> {
    let not_a_table = || SaveError::NotATable(path.to_string());
    let keys = toml_edit::Key::parse(path).map_err(|_| not_a_table())?;
    let mut table: &'d mut dyn toml_edit::TableLike = doc.as_table_mut();
    for (i, key) in keys.iter().enumerate() {
        let name = key.get();
        if !table.contains_key(name) {
            if !make {
                return Ok(None);
            }
            let mut made = toml_edit::Table::new();
            made.set_implicit(i + 1 < keys.len());
            table.insert(name, toml_edit::Item::Table(made));
        }
        table = table
            .get_mut(name)
            .and_then(|item| item.as_table_like_mut())
            .ok_or_else(not_a_table)?;
    }
    Ok(Some(table))
}

/// Applies `edits` to `<dir>/config.toml`, keeping everything else (comments, other keys, line
/// endings, a byte-order mark). Creates the folder and the file; never writes over a file that
/// doesn't parse.
pub(crate) fn save_edits(dir: &Path, edits: &[Edit]) -> Result<(), SaveError> {
    save_edits_to(&dir.join(FILE), edits)
}

/// `save_edits` for any TOML file: a theme's.
pub(crate) fn save_edits_to(path: &Path, edits: &[Edit]) -> Result<(), SaveError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(SaveError::Io(e)),
    };
    let out = apply_edits(&text, edits)?;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(SaveError::Io)?;
    }
    write_whole(path, &out).map_err(SaveError::Io)
}

/// `text` with `edits` made, everything else kept; an error when `text` doesn't parse.
pub(crate) fn apply_edits(text: &str, edits: &[Edit]) -> Result<String, SaveError> {
    let crlf = text.contains("\r\n");
    let bom = text.starts_with('\u{feff}');
    let mut doc: DocumentMut = text.parse().map_err(|e| SaveError::Broken {
        line: line_of(text, &e),
    })?;
    for edit in edits {
        match &edit.value {
            Some(value) => {
                let table = table_at(&mut doc, &edit.table, true)?
                    .ok_or_else(|| SaveError::NotATable(edit.table.to_string()))?;
                // A new value in place keeps the key's comment lines and its `# …` on the line.
                match table
                    .get_mut(&edit.key)
                    .and_then(|item| item.as_value_mut())
                {
                    Some(old) => {
                        let mut value = value.clone();
                        *value.decor_mut() = old.decor().clone();
                        *old = value;
                    }
                    None => {
                        table.insert(&edit.key, toml_edit::Item::Value(value.clone()));
                    }
                }
            }
            None => remove_key(&mut doc, &edit.table, &edit.key)?,
        }
    }
    // toml_edit writes LF and drops a byte-order mark: put the file's own back.
    let mut out = doc.to_string();
    if crlf {
        out = out.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    if bom && !out.starts_with('\u{feff}') {
        out.insert(0, '\u{feff}');
    }
    Ok(out)
}

/// Writes `text` to `path` whole: a temporary file beside it, renamed over it, so a crash
/// mid-write never leaves half a file. A linked file (from a dotfiles repository) stays linked: the file it points at is the
/// one replaced, with its permissions. A read-only file is left alone, as a plain write would.
pub(crate) fn write_whole(path: &Path, text: &str) -> io::Result<()> {
    let target = match std::fs::canonicalize(path) {
        Ok(target) => target,
        // A link to a file not made yet: make it where the link points.
        Err(_) => match std::fs::read_link(path) {
            Ok(link) => path.with_file_name("").join(link),
            Err(_) => path.to_path_buf(),
        },
    };
    let name = target
        .file_name()
        .map_or_else(|| FILE.to_string(), |n| n.to_string_lossy().into_owned());
    let old = std::fs::metadata(&target).ok();
    if old.as_ref().is_some_and(|m| m.permissions().readonly()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{name} is read-only"),
        ));
    }
    let tmp = target.with_file_name(format!(".{name}.tmp"));
    let written = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        io::Write::write_all(&mut file, text.as_bytes())?;
        file.sync_all()?;
        if let Some(old) = &old {
            std::fs::set_permissions(&tmp, old.permissions())?;
        }
        std::fs::rename(&tmp, &target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Removes `table.key`. The comment lines above a key belong to it in toml_edit: they move to
/// whatever follows (the next key, the next table's header, or the end of the file).
fn remove_key(doc: &mut DocumentMut, table: &str, key: &str) -> Result<(), SaveError> {
    let Some(t) = table_at(doc, table, false)? else {
        return Ok(());
    };
    let comments = t
        .key(key)
        .and_then(|k| k.leaf_decor().prefix())
        .and_then(|p| p.as_str())
        .filter(|p| p.contains('#'))
        .map(str::to_owned);
    let next = t
        .iter()
        .map(|(k, _)| k.to_owned())
        .skip_while(|k| k != key)
        .nth(1);
    t.remove(key);
    let Some(comments) = comments else {
        return Ok(());
    };
    if let Some(mut next) = next.and_then(|k| t.key_mut(&k)) {
        let decor = next.leaf_decor_mut();
        let rest = decor.prefix().and_then(|p| p.as_str()).unwrap_or("");
        decor.set_prefix(format!("{comments}{rest}"));
        return Ok(());
    }
    // The last key of its table: before the next table's header, or at the end.
    // ponytail: the next header is looked for among the top-level tables only: the comments
    // above a nested table's last key may move before a later top-level table rather than a
    // nested one in between. Walk every table if someone's comments end up there.
    let at = toml_edit::Key::parse(table)
        .ok()
        .and_then(|keys| {
            keys.iter()
                .try_fold(doc.as_item(), |item, k| item.get(k.get()))
        })
        .and_then(|item| item.as_table())
        .and_then(|t| t.position());
    let next_table = doc
        .iter_mut()
        .filter_map(|(_, item)| item.as_table_mut())
        .filter(|t| t.position() > at)
        .min_by_key(|t| t.position());
    match next_table {
        Some(next) => {
            let decor = next.decor_mut();
            let rest = decor.prefix().and_then(|p| p.as_str()).unwrap_or("");
            decor.set_prefix(format!("{comments}{rest}"));
        }
        None => {
            let rest = doc.trailing().as_str().unwrap_or("").to_owned();
            doc.set_trailing(format!("{comments}{rest}"));
        }
    }
    Ok(())
}

/// Sets `theme.dark` or `theme.light` (the theme picker's choice).
pub(crate) fn save_theme(dir: &Path, dark: bool, name: &str) -> Result<(), SaveError> {
    save_edits(
        dir,
        &[Edit::set(
            "theme",
            if dark { "dark" } else { "light" },
            name,
        )],
    )
}

/// The commented config.toml that `open_config` writes when there is none. Uncommented, every
/// `# key = value` line is the default; prose lines start with "##".
pub(crate) fn template(macos: bool) -> String {
    use crate::keys::Command;
    let mut t = String::from(concat!(
        "## Templum's configuration. Remove the \"# \" before a line (or \"## \" before an example)\n",
        "## and save: it applies at once.\n",
        "## All options: https://github.com/KingDasWinx/Templum#configuration\n\n",
        "[font]\n",
        "## Default: the first installed font of a built-in list. For example:\n",
        "## family = \"JetBrains Mono\"\n",
        "# size = 14   # 6 to 72\n\n",
        "[theme]\n",
        "# mode = \"system\"   # \"system\" | \"light\" | \"dark\"\n",
        "# light = \"Catppuccin Latte\"\n",
        "# dark = \"Catppuccin Mocha\"\n\n",
        "[shell]\n",
        "## Default: $SHELL. On Windows: Git Bash, else PowerShell 7, else Windows PowerShell.\n",
        "## For example:\n",
        "## program = \"fish\"\n",
        "## args = [\"--login\"]\n",
        "# working_directory = \"inherit\"   # \"inherit\" | \"~\" | a path\n",
        "## bash, zsh, fish and PowerShell tell Templum when commands end, and their folder.\n",
        "# integration = true\n\n",
        "[cursor]\n",
        "# shape = \"block\"   # \"block\" | \"bar\" | \"underline\"\n",
        "# blink = false\n\n",
        "[terminal]\n",
        "# scrollback = 10000   # 0 to 100000\n",
        "## A system notification when Templum is in the background and a long command ends or\n",
        "## a terminal asks for attention.\n",
        "# notifications = true\n",
        "## Text selected with the mouse goes to the clipboard as the button is let go.\n",
        "# copy_on_select = true\n\n",
        "[canvas]\n",
        "## The block under the pointer takes the keyboard once the pointer moves, no click needed.\n",
        "# focus_follows_pointer = true\n\n",
        "[usage]\n",
        "## When a plan limit's reset is notified: \"off\", \"reached\" (95% or more of it was used),\n",
        "## \"half\" or \"always\".\n",
        "# reset = \"reached\"\n",
        "# warn = 90   # notify once at this much of a limit: 0 (never), 80, 90 or 95\n",
        "## Templum asks Claude Code for its limits (`claude /usage`) every 5 minutes.\n",
        "# claude_limits = true\n",
        "## Agents whose limits the title bar doesn't show, as [\"codex\"].\n",
        "# hide = []\n\n",
        "[updates]\n",
        "## Look for a new version on GitHub at start and once a day, and download it: Templum\n",
        "## asks before restarting into it.\n",
        "# check = true\n\n",
        "[connections]\n",
        "## The agents Templum opens, or that are typed in its terminals, get its MCP server and\n",
        "## hooks: they can read and type into the blocks wired to them (docs/specs/connections.md).\n",
        "# enabled = true\n",
        "## How long an agent's question waits for your answer, in minutes; 0 waits until you answer.\n",
        "# question_timeout = 0\n",
        "## A short sound when an agent asks you something.\n",
        "# sound = true\n\n",
        "[roles]\n",
        "## Ways spawn_agent opens an agent by name (docs/specs/agent-tools.md#roles): its agent, and\n",
        "## optionally its model, effort, args (the agent's own flags) and instructions. For example:\n",
        "## reviewer = { agent = \"claude\", model = \"opus\", effort = \"high\", args = \"--tools Read,Grep,Glob,Bash\", instructions = \"You review code and report each finding as a card. Never edit files.\" }\n\n",
        "[templum_agent]\n",
        "## The agent the Templum Agent runs on (docs/specs/templum-agent.md): claude.\n",
        "# agent = \"claude\"\n",
        "## Its model; empty is the agent's own default.\n",
        "# model = \"\"\n",
        "## Its effort: low, medium, high, xhigh or max for Claude Code; empty is the agent's default.\n",
        "# effort = \"\"\n",
        "## The prompt pill at the bottom of the canvas; off, Ctrl+Shift+Space still opens it.\n",
        "# pill = true\n",
        "## How many of its tasks may run at once.\n",
        "# max_agents = 4\n\n",
        "[git]\n",
        "# program = \"git\"   # a name on PATH, or a path\n\n",
        "[keys]\n",
    ));
    for command in Command::ALL {
        let quoted: Vec<String> = command
            .defaults(macos)
            .iter()
            .map(|k| format!("\"{k}\""))
            .collect();
        let value = match quoted.as_slice() {
            [one] => one.clone(),
            many => format!("[{}]", many.join(", ")),
        };
        t.push_str(&format!("# {} = {value}\n", command.name()));
    }
    t
}

/// `<dir>/config.toml`, written from the template first when it doesn't exist.
pub(crate) fn ensure_file(dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join(FILE);
    if !path.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&path, template(cfg!(target_os = "macos")))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_git_program_is_read_and_defaults_to_git() {
        assert_eq!(Config::default().git.program, "git");
        let config = parse("[git]\nprogram = \"/opt/git/bin/git\"\n").unwrap();
        assert_eq!(config.git.program, "/opt/git/bin/git");
        assert!(template(false).contains("[git]\n# program = \"git\""));
    }

    fn write(dir: &Path, text: &str) {
        std::fs::write(dir.join(FILE), text).unwrap();
    }

    #[test]
    fn line_of_points_at_the_error() {
        let text = "[theme]\nlight = \"x\"\ndark = @nope\n";
        let err = text.parse::<toml_edit::DocumentMut>().unwrap_err();
        assert_eq!(line_of(text, &err), 3);
    }

    #[test]
    fn dir_follows_the_platform_conventions() {
        let xdg = Some(std::ffi::OsString::from("/x"));
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(
            dir_from(false, None, xdg, home.clone()),
            Some(PathBuf::from("/x").join("templum"))
        );
        assert_eq!(
            dir_from(false, None, Some("".into()), home.clone()),
            Some(PathBuf::from("/home/u").join(".config").join("templum"))
        );
        assert_eq!(dir_from(false, None, None, None), None);
        let appdata = Some(std::ffi::OsString::from("C:/Users/u/AppData/Roaming"));
        assert_eq!(
            dir_from(true, appdata, None, home),
            Some(PathBuf::from("C:/Users/u/AppData/Roaming").join("templum"))
        );
    }

    #[test]
    fn saving_keeps_comments_and_other_tables() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "# my settings\n[theme]\nlight = \"One Light\" # sunny\n\n[font]\nsize = 16\n",
        );
        save_theme(d.path(), true, "One Dark").unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("# my settings"), "{text}");
        assert!(text.contains("light = \"One Light\" # sunny"), "{text}");
        assert!(text.contains("[font]\nsize = 16"), "{text}");
        let c = load(Some(d.path()), None).unwrap();
        assert_eq!(
            (c.theme.light.as_str(), c.theme.dark.as_str()),
            ("One Light", "One Dark")
        );
    }

    #[test]
    fn saving_creates_the_folder_and_the_file() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("templum");
        save_theme(&dir, false, "Catppuccin Frappé").unwrap();
        assert_eq!(
            load(Some(&dir), None).unwrap().theme.light,
            "Catppuccin Frappé"
        );
    }

    #[test]
    fn saving_never_overwrites_a_broken_file() {
        let d = tempfile::tempdir().unwrap();
        let broken = "[theme]\nlight = \"One Light\"\ndark = @nope\n";
        write(d.path(), broken);
        let err = save_theme(d.path(), true, "One Dark").unwrap_err();
        assert_eq!(err.to_string(), "config.toml has an error on line 3");
        assert_eq!(
            std::fs::read_to_string(d.path().join(FILE)).unwrap(),
            broken
        );
    }

    #[test]
    fn save_edits_to_keeps_a_theme_files_comments() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("themes").join("Mine.toml");
        std::fs::create_dir(d.path().join("themes")).unwrap();
        let text = "# Mine\r\n[colors.normal]\n# Errors.\nred = \"#aa0000\" # on the line\ngreen = \"#00aa00\"\n\n[templum]\n# accent = \"#000000\"\n";
        std::fs::write(&path, text).unwrap();
        save_edits_to(
            &path,
            &[
                Edit::set("colors.normal", "red", "#ff0000"),
                Edit::set("templum", "canvas", "#101010"),
            ],
        )
        .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(
            saved.contains("# Errors.\r\nred = \"#ff0000\" # on the line"),
            "{saved}"
        );
        assert!(saved.contains("green = \"#00aa00\""));
        assert!(saved.contains("canvas = \"#101010\""));
        assert!(saved.contains("# accent = \"#000000\""), "{saved}");
        save_edits_to(&path, &[Edit::remove("templum", "canvas")]).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("canvas"));
    }

    #[test]
    fn describe_names_the_file() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("Mine.toml");
        std::fs::write(&path, "[colors\n").unwrap();
        let err =
            save_edits_to(&path, &[Edit::set("colors.normal", "red", "#ff0000")]).unwrap_err();
        assert_eq!(
            err.describe("Mine.toml"),
            "Mine.toml has an error on line 1"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[colors\n",
            "left alone"
        );
    }

    #[test]
    fn saving_refuses_a_non_table_theme() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "theme = \"Dracula\"\n");
        let err = save_theme(d.path(), false, "One Light").unwrap_err();
        assert_eq!(err.to_string(), "`theme` in config.toml is not a table");
        assert_eq!(
            std::fs::read_to_string(d.path().join(FILE)).unwrap(),
            "theme = \"Dracula\"\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_save_replaces_the_file_in_one_step() {
        use std::os::unix::fs::MetadataExt as _;
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "[font]\nsize = 16\n");
        let before = std::fs::metadata(d.path().join(FILE)).unwrap().ino();
        save_theme(d.path(), true, "One Dark").unwrap();
        let after = std::fs::metadata(d.path().join(FILE)).unwrap().ino();
        assert_ne!(before, after, "written beside it, then renamed over it");
        let names: Vec<_> = std::fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(names.len(), 1, "no temporary file left: {names:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_save_keeps_a_linked_file_linked_and_its_permissions() {
        use std::os::unix::fs::PermissionsExt as _;
        let d = tempfile::tempdir().unwrap();
        let dotfiles = d.path().join("dotfiles");
        std::fs::create_dir(&dotfiles).unwrap();
        let real = dotfiles.join(FILE);
        std::fs::write(&real, "[font]\nsize = 16\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        let dir = d.path().join("templum");
        std::fs::create_dir(&dir).unwrap();
        std::os::unix::fs::symlink(&real, dir.join(FILE)).unwrap();
        save_theme(&dir, true, "One Dark").unwrap();
        let link = std::fs::symlink_metadata(dir.join(FILE)).unwrap();
        assert!(link.file_type().is_symlink(), "still a link");
        assert!(std::fs::read_to_string(&real).unwrap().contains("One Dark"));
        let mode = std::fs::metadata(&real).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_file_not_made_yet_makes_it() {
        let d = tempfile::tempdir().unwrap();
        let dotfiles = d.path().join("dotfiles");
        std::fs::create_dir(&dotfiles).unwrap();
        let real = dotfiles.join(FILE);
        let dir = d.path().join("templum");
        std::fs::create_dir(&dir).unwrap();
        std::os::unix::fs::symlink(&real, dir.join(FILE)).unwrap();
        save_theme(&dir, true, "One Dark").unwrap();
        let link = std::fs::symlink_metadata(dir.join(FILE)).unwrap();
        assert!(link.file_type().is_symlink(), "still a link");
        assert!(std::fs::read_to_string(&real).unwrap().contains("One Dark"));
    }

    #[test]
    fn any_file_is_written_whole_and_named_in_errors() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("default.json");
        write_whole(&path, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        let err = write_whole(&path, "[]").unwrap_err();
        assert_eq!(err.to_string(), "default.json is read-only");
    }

    #[test]
    fn a_read_only_file_is_left_alone() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "[font]\nsize = 16\n");
        let path = d.path().join(FILE);
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        let err = save_theme(d.path(), true, "One Dark").unwrap_err();
        assert_eq!(err.to_string(), "config.toml is read-only");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[font]\nsize = 16\n"
        );
    }

    use templum_terminal::{CursorShape, CursorStyle};

    #[test]
    fn an_empty_file_is_the_defaults() {
        let c = parse("").unwrap();
        assert_eq!(c, Config::default());
        assert_eq!(c.font.size, 14.0);
        assert_eq!(c.theme.mode, Mode::System);
        assert_eq!(c.theme.name(false), "Catppuccin Latte");
        assert_eq!(c.theme.name(true), "Catppuccin Mocha");
        assert_eq!(c.shell.working_directory, WorkingDirectory::Inherit);
        assert_eq!(c.terminal.scrollback, 10_000);
        assert_eq!(c.cursor.style(), CursorStyle::default());
    }

    #[test]
    fn reads_every_option() {
        let c = parse(
            r#"
[font]
family = "JetBrains Mono"
size = 16
[theme]
mode = "dark"
light = "One Light"
dark = "One Dark"
[shell]
program = "fish"
args = ["--login"]
working_directory = "~"
[cursor]
shape = "bar"
blink = true
[terminal]
scrollback = 500
"#,
        )
        .unwrap();
        assert_eq!(c.font.family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(c.font.size, 16.0);
        assert!(c.theme.mode.dark(false), "dark is pinned");
        assert_eq!(c.theme.name(true), "One Dark");
        assert_eq!(c.shell.program.as_deref(), Some("fish"));
        assert_eq!(c.shell.args, ["--login"]);
        assert_eq!(c.shell.working_directory, WorkingDirectory::Home);
        let options = c.term_options();
        assert_eq!(options.scrollback, 500);
        assert_eq!(
            options.cursor,
            CursorStyle {
                shape: CursorShape::Beam,
                blinking: true
            }
        );
    }

    #[test]
    fn a_window_table_is_ignored() {
        // It held the glass options, now removed: a file written before still loads.
        assert_eq!(
            parse("[window]\nblur = false\nopacity = 0.9\n").unwrap(),
            Config::default()
        );
        assert!(parse("[window]\nanything = \"else\"\n").is_ok());
    }

    #[test]
    fn mode_light_pins_light() {
        let c = parse("[theme]\nmode = \"light\"").unwrap();
        assert!(!c.theme.mode.dark(true));
        assert!(Mode::System.dark(true) && !Mode::System.dark(false));
    }

    #[test]
    fn working_directory_forms() {
        let home = std::env::home_dir().unwrap();
        let wd = |s: &str| {
            parse(&format!("[shell]\nworking_directory = {s}"))
                .unwrap()
                .shell
                .working_directory
        };
        assert_eq!(wd("\"inherit\""), WorkingDirectory::Inherit);
        assert_eq!(wd("\"~\""), WorkingDirectory::Home);
        assert_eq!(wd("\"~/code\""), WorkingDirectory::Path(home.join("code")));
        assert_eq!(
            wd("\"/srv/x\""),
            WorkingDirectory::Path(PathBuf::from("/srv/x"))
        );
        // A Windows path as a TOML literal string: no escapes needed.
        assert_eq!(
            wd(r"'C:\Users\x'"),
            WorkingDirectory::Path(PathBuf::from(r"C:\Users\x"))
        );
    }

    #[test]
    fn unknown_keys_fail_with_line_and_column() {
        let err = parse("[font]\nsize = 14\n\nfnot = 1\n").unwrap_err();
        assert!(err.starts_with("config.toml, line 4, column 1:"), "{err}");
        assert!(err.contains("fnot"), "{err}");
    }

    #[test]
    fn out_of_range_values_fail_naming_the_key() {
        let err = parse("[font]\nsize = 100").unwrap_err();
        assert!(
            err.contains("font.size") && err.contains("6 and 72"),
            "{err}"
        );
        let err = parse("[terminal]\nscrollback = 200000").unwrap_err();
        assert!(
            err.contains("terminal.scrollback") && err.contains("100000"),
            "{err}"
        );
    }

    #[test]
    fn load_gives_defaults_without_a_file_and_errors_for_a_bad_one() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load(Some(d.path()), None).unwrap(), Config::default());
        assert_eq!(load(None, None).unwrap(), Config::default());
        write(d.path(), "[font\n");
        assert!(load(Some(d.path()), None).unwrap_err().contains("line 1"));
    }

    #[test]
    fn stamp_notices_edits_creation_and_deletion() {
        let d = tempfile::tempdir().unwrap();
        let none = Stamp::of(Some(d.path()));
        assert_eq!(none, Stamp::default());
        write(d.path(), "[font]\n");
        let created = Stamp::of(Some(d.path()));
        assert_ne!(created, none);
        write(d.path(), "[font]\nsize = 15\n");
        assert_ne!(Stamp::of(Some(d.path())), created);
        std::fs::remove_file(d.path().join(FILE)).unwrap();
        assert_eq!(Stamp::of(Some(d.path())), none);
    }

    #[test]
    fn stamp_notices_a_replace_by_rename() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "[font]\n");
        let before = Stamp::of(Some(d.path()));
        // How vim and VS Code save: write elsewhere, then rename over the file.
        let tmp = d.path().join("config.toml.tmp");
        std::fs::write(&tmp, "[font]\nsize = 16\n").unwrap();
        std::fs::rename(&tmp, d.path().join(FILE)).unwrap();
        assert_ne!(Stamp::of(Some(d.path())), before);
    }

    #[test]
    fn keys_are_read_and_validated() {
        let c = parse("[keys]\nnew_terminal = \"ctrl-alt-n\"\nfit_all = [\"f11\"]\n").unwrap();
        assert_eq!(
            c.keys.new_terminal,
            Some(crate::keys::Keys::One("ctrl-alt-n".into()))
        );
        let err = parse("[keys]\nnew_terminal = \"ctrl-shft-t\"\n").unwrap_err();
        assert!(err.contains("new_terminal"), "{err}");
        let err = parse("[keys]\nnew_tab = \"ctrl-t\"\n").unwrap_err();
        assert!(err.contains("new_tab"), "unknown action: {err}");
    }

    /// The template with every `# key = value` line uncommented (prose lines start with "##").
    fn uncomment(text: &str) -> String {
        text.lines()
            .map(|l| l.strip_prefix("# ").unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_template_is_the_defaults_commented_or_not() {
        for macos in [false, true] {
            let t = template(macos);
            assert_eq!(parse(&t).unwrap(), Config::default(), "all commented");
            let mut c = parse(&uncomment(&t)).unwrap();
            let keys = std::mem::take(&mut c.keys);
            assert_eq!(c, Config::default(), "uncommented");
            let (from_template, _) = Keymap::new(&keys, macos).unwrap();
            let defaults = Keymap::new(&KeysConfig::default(), macos).unwrap().0;
            for command in crate::keys::Command::ALL {
                assert_eq!(from_template.first(command), defaults.first(command));
            }
        }
    }

    #[test]
    fn ensure_file_writes_the_template_once() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("templum");
        let path = ensure_file(&dir).unwrap();
        assert_eq!(path, dir.join(FILE));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            template(cfg!(target_os = "macos"))
        );
        std::fs::write(&path, "[font]\nsize = 20\n").unwrap();
        ensure_file(&dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[font]\nsize = 20\n",
            "kept"
        );
    }

    #[test]
    fn saving_keeps_crlf_and_the_bom() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "\u{feff}# mine\r\n[theme]\r\nlight = \"One Light\"\r\n",
        );
        save_theme(d.path(), true, "One Dark").unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.starts_with('\u{feff}'), "{text:?}");
        assert!(
            !text.replace("\r\n", "").contains('\n'),
            "LF only line: {text:?}"
        );
        assert!(text.contains("dark = \"One Dark\""), "{text:?}");
    }

    #[test]
    fn usage_notifications_have_defaults_and_bad_values_fail() {
        let c = parse("").unwrap();
        assert_eq!(c.usage.reset, Reset::Reached);
        assert_eq!(c.usage.warn, 90);
        assert!(c.usage.claude_limits);
        let c = parse(
            "[usage]\nreset = \"always\"\nwarn = 0\nclaude_limits = false\nhide = [\"codex\"]\n",
        )
        .unwrap();
        assert_eq!(
            c.usage,
            UsageConfig {
                reset: Reset::Always,
                warn: 0,
                claude_limits: false,
                hide: vec!["codex".into()],
            }
        );
        let err = parse("[usage]\nwarn = 85\n").unwrap_err();
        assert!(err.contains("usage.warn"), "{err}");
        assert!(parse("[usage]\nreset = \"sometimes\"\n").is_err());
    }

    #[test]
    fn connections_are_on_unless_turned_off() {
        assert!(parse("").unwrap().connections.enabled);
        assert!(
            !parse("[connections]\nenabled = false\n")
                .unwrap()
                .connections
                .enabled
        );
        let err = parse("[connections]\nmcp = true\n").unwrap_err();
        assert!(err.contains("mcp"), "{err}");
    }

    #[test]
    fn the_templum_agent_runs_on_claude_with_its_pill_unless_set() {
        let agent = parse("").unwrap().templum_agent;
        assert_eq!(
            (
                agent.agent.as_str(),
                agent.model.as_str(),
                agent.effort.as_str()
            ),
            ("claude", "", "")
        );
        assert!(agent.pill);
        assert_eq!(agent.max_agents, 4);
        let set =
            parse("[templum_agent]\nmodel = \"opus\"\neffort = \"high\"\npill = false\n").unwrap();
        assert_eq!(
            (
                set.templum_agent.model.as_str(),
                set.templum_agent.effort.as_str()
            ),
            ("opus", "high")
        );
        assert!(!set.templum_agent.pill);
        let err = parse("[templum_agent]\nagent = \"gemini\"\n").unwrap_err();
        assert!(err.contains("templum_agent.agent is claude"), "{err}");
        let err = parse("[templum_agent]\neffort = \"ultra\"\n").unwrap_err();
        assert!(
            err.contains("templum_agent.effort") && err.contains("low, medium, high, xhigh or max"),
            "{err}"
        );
        let err = parse("[templum_agent]\nmodel = \"opus; rm -rf /\"\n").unwrap_err();
        assert!(err.contains("templum_agent.model"), "{err}");
        let err = parse("[templum_agent]\nmax_agents = 17\n").unwrap_err();
        assert!(
            err.contains("templum_agent.max_agents must be between 1 and 16"),
            "{err}"
        );
        assert!(parse("[templum_agent]\nmax_agents = 0\n").is_err());
        let t = template(false);
        assert!(t.contains("[templum_agent]\n"), "{t}");
        for key in ["agent", "model", "effort", "pill", "max_agents"] {
            assert!(t.contains(&format!("\n# {key} = ")), "{key}");
        }
    }

    #[test]
    fn questions_wait_until_answered_and_make_a_sound_unless_set() {
        let connections = parse("").unwrap().connections;
        assert_eq!(connections.question_timeout, 0);
        assert!(connections.sound);
        let set = parse("[connections]\nquestion_timeout = 1440\nsound = false\n").unwrap();
        assert_eq!(set.connections.question_timeout, 1440);
        assert!(!set.connections.sound);
        let err = parse("[connections]\nquestion_timeout = 1441\n").unwrap_err();
        assert!(
            err.contains("connections.question_timeout must be between 0 and 1440"),
            "{err}"
        );
        assert!(parse("[connections]\nquestion_timeout = -1\n").is_err());
    }

    #[test]
    fn updates_are_checked_unless_turned_off() {
        assert!(parse("").unwrap().updates.check);
        assert!(!parse("[updates]\ncheck = false\n").unwrap().updates.check);
        let err = parse("[updates]\nchannel = \"beta\"\n").unwrap_err();
        assert!(err.contains("channel"), "{err}");
    }

    #[test]
    fn shell_integration_and_notifications_are_on_unless_turned_off() {
        let c = parse("").unwrap();
        assert!(c.shell.integration && c.terminal.notifications);
        let c = parse("[shell]\nintegration = false\n[terminal]\nnotifications = false\n").unwrap();
        assert!(!c.shell.integration && !c.terminal.notifications);
    }

    #[test]
    fn a_selection_is_copied_unless_turned_off() {
        assert!(parse("").unwrap().terminal.copy_on_select);
        let c = parse("[terminal]\ncopy_on_select = false\n").unwrap();
        assert!(!c.terminal.copy_on_select);
    }

    #[test]
    fn focus_follows_the_pointer_unless_turned_off() {
        assert!(parse("").unwrap().canvas.focus_follows_pointer);
        let c = parse("[canvas]\nfocus_follows_pointer = false\n").unwrap();
        assert!(!c.canvas.focus_follows_pointer);
    }

    /// `[roles.<id>]`: a way to open an agent, read whole; a role breaking the rules is a config
    /// error naming it (docs/specs/agent-tools.md, Roles).
    #[test]
    fn a_role_reads_back_and_a_bad_one_is_named() {
        let c = parse(
            "[roles.reviewer]\nagent = \"claude\"\nmodel = \"opus\"\neffort = \"high\"\n\
             args = \"--tools Read,Grep\"\ninstructions = \"\"\"\nNever edit files.\n\"\"\"\n",
        )
        .unwrap();
        assert_eq!(
            c.roles["reviewer"],
            Role {
                agent: "claude".into(),
                model: Some("opus".into()),
                effort: Some("high".into()),
                args: Some("--tools Read,Grep".into()),
                instructions: Some("Never edit files.\n".into()),
            }
        );
        for (text, names) in [
            ("[roles.reviewer]\nmodel = \"opus\"\n", "roles.reviewer"),
            ("[roles.reviewer]\nagent = \" \"\n", "roles.reviewer"),
            (
                "[roles.reviewer]\nagent = \"claude\"\nargs = \"a\\nb\"\n",
                "roles.reviewer",
            ),
            (
                "[roles.\"re view\"]\nagent = \"claude\"\n",
                "roles.\"re view\"",
            ),
        ] {
            let err = parse(text).unwrap_err();
            assert!(err.contains(names), "{text}: {err}");
        }
    }

    #[test]
    fn the_templates_example_lines_work_once_uncommented() {
        // "## key = value" example lines (font family, shell) become settings without "## ".
        let t = template(false);
        let text: String = t
            .lines()
            .map(|l| {
                let l = l
                    .strip_prefix("## ")
                    .filter(|rest| rest.contains(" = "))
                    .unwrap_or(l);
                l.strip_prefix("# ").unwrap_or(l)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let c = parse(&text).unwrap();
        assert_eq!(c.font.family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(c.shell.program.as_deref(), Some("fish"));
        assert_eq!(c.shell.args, ["--login"]);
        assert_eq!(c.roles["reviewer"].agent, "claude");
    }

    #[test]
    fn edits_set_create_and_remove_keys() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "# mine\n[font]\nsize = 16\nfamily = \"Iosevka\"\n\n[cursor]\nblink = true # on\n",
        );
        save_edits(
            d.path(),
            &[
                Edit::set("font", "size", 20_i64),
                Edit::remove("font", "family"),
                Edit::set("terminal", "scrollback", 500_i64),
                Edit::remove("keys", "copy"),
            ],
        )
        .unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("# mine"), "{text}");
        assert!(text.contains("size = 20"), "{text}");
        assert!(!text.contains("family"), "{text}");
        assert!(text.contains("blink = true # on"), "{text}");
        assert!(text.contains("[terminal]"), "{text}");
        assert!(text.contains("scrollback = 500"), "{text}");
        assert!(
            !text.contains("[keys]"),
            "removing from a missing table adds nothing: {text}"
        );
        let c = load(Some(d.path()), None).unwrap();
        assert_eq!((c.font.size, c.font.family), (20.0, None));
        assert_eq!(c.terminal.scrollback, 500);
    }

    #[test]
    fn an_edit_to_a_non_table_names_it() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "font = 3\n");
        let err = save_edits(d.path(), &[Edit::set("font", "size", 20_i64)]).unwrap_err();
        assert_eq!(err.to_string(), "`font` in config.toml is not a table");
        let err = save_edits(d.path(), &[Edit::remove("font", "size")]).unwrap_err();
        assert_eq!(err.to_string(), "`font` in config.toml is not a table");
        assert_eq!(
            std::fs::read_to_string(d.path().join(FILE)).unwrap(),
            "font = 3\n"
        );
    }

    #[test]
    fn agents_change_ours_and_add_the_users() {
        let c = parse(concat!(
            "[agents.claude]\n",
            "command = \"claude --model opus\"\n",
            "[agents]\n",
            "codex = { show = false }\n",
            "mine = { name = \"Mine\", command = \"mine --tui\", resume = \"\" }\n",
        ))
        .unwrap();
        let claude = &c.agents["claude"];
        assert_eq!(claude.command.as_deref(), Some("claude --model opus"));
        assert_eq!(claude.name, None, "unchanged: the default");
        assert_eq!(c.agents["codex"].show, Some(false));
        let mine = &c.agents["mine"];
        assert_eq!(mine.name.as_deref(), Some("Mine"));
        assert_eq!(
            mine.resume.as_deref(),
            Some(""),
            "empty: a new session on restore"
        );
    }

    #[test]
    fn an_agent_of_the_users_needs_a_name_and_a_command() {
        let err = parse("[agents.mine]\nname = \"Mine\"\n").unwrap_err();
        assert!(err.contains("agents.mine.command"), "{err}");
        let err = parse("[agents.mine]\ncommand = \"mine\"\n").unwrap_err();
        assert!(err.contains("agents.mine.name"), "{err}");
        let err = parse("[agents.claude]\ncommand = \"  \"\n").unwrap_err();
        assert!(err.contains("agents.claude.command"), "{err}");
        let err = parse("[agents.\"my agent\"]\nname = \"x\"\ncommand = \"x\"\n").unwrap_err();
        assert!(err.contains("my agent"), "{err}");
        let err = parse("[agents.claude]\nmodel = \"x\"\n").unwrap_err();
        assert!(err.contains("model"), "unknown keys still fail: {err}");
    }

    const ACCOUNTS: &str = concat!(
        "[agents.claude2]\n",
        "of = \"claude\"\n",
        "name = \"Claude Code 2\"\n",
        "show = true\n",
        "share = { settings = \"claude\", instructions = \"claude\", skills = \"claude\", plugins = \"claude\", sessions = \"claude\" }\n",
        "[agents.claude3]\n",
        "of = \"claude\"\n",
        "[agents.claude4]\n",
        "of = \"claude\"\n",
        "share = { settings = \"claude3\", skills = \"claude3\", output-styles = \"claude3\", history = \"claude3\" }\n",
    );

    #[test]
    fn accounts_parse_with_their_shares() {
        let c = parse(ACCOUNTS).unwrap();
        let claude2 = &c.agents["claude2"];
        assert_eq!(claude2.of.as_deref(), Some("claude"));
        assert_eq!(claude2.share.as_ref().unwrap()["skills"], "claude");
        assert_eq!(c.agents["claude3"].share, None, "everything its own");
        assert_eq!(
            c.agents["claude4"].share.as_ref().unwrap()["skills"],
            "claude3"
        );
    }

    #[test]
    fn an_account_is_refused_what_it_inherits() {
        for key in [
            "command = \"x\"",
            "resume = \"\"",
            "generate = \"x\"",
            "models = [\"a\"]",
        ] {
            let err = parse(&format!("[agents.claude2]\nof = \"claude\"\n{key}\n")).unwrap_err();
            let name = key.split(' ').next().unwrap();
            assert!(
                err.contains(&format!(
                    "agents.claude2.{name}: an account runs its agent's command"
                )),
                "{err}"
            );
        }
    }

    #[test]
    fn an_accounts_shares_are_checked() {
        let cases = [
            (
                "[agents.claude2]\nof = \"claude\"\nshare = { nope = \"claude\" }\n",
                "agents.claude2.share.nope",
            ),
            (
                "[agents.claude2]\nof = \"claude\"\nshare = { skills = \"codex\" }\n",
                "agents.claude2.share.skills",
            ),
            (
                "[agents.codex2]\nof = \"codex\"\n[agents.claude2]\nof = \"claude\"\nshare = { skills = \"codex2\" }\n",
                "agents.claude2.share.skills",
            ),
            (
                "[agents.claude3]\nof = \"claude\"\nshare = { skills = \"claude4\" }\n[agents.claude4]\nof = \"claude\"\nshare = { skills = \"claude3\" }\n",
                "share.skills: claude",
            ),
            (
                "[agents.claude2]\nof = \"claude\"\nshare = { skills = \"claude2\" }\n",
                "agents.claude2.share.skills",
            ),
        ];
        for (text, needle) in cases {
            let err = parse(text).unwrap_err();
            assert!(err.contains(needle), "{text}\n{err}");
        }
        let err = parse(cases[3].0).unwrap_err();
        assert!(err.contains("shares it back"), "{err}");
    }

    #[test]
    fn two_accounts_cant_share_a_folder() {
        let err = parse(concat!(
            "[agents.claude2]\nof = \"claude\"\nfolder = \"~/srv/work\"\n",
            "[agents.claude3]\nof = \"claude\"\nfolder = \"~/srv/work/\"\n",
        ))
        .unwrap_err();
        assert!(err.contains(".folder: ~/srv/work"), "{err}");
        parse("[agents.claude2]\nof = \"claude\"\nfolder = \"~/srv/work\"\n").unwrap();
    }

    #[test]
    fn an_account_folder_isnt_in_templums_folder_for_accounts() {
        let data = std::env::home_dir().unwrap().join("templum-data");
        for folder in [
            "~/templum-data/accounts",
            "~/templum-data/accounts/claude3/x",
        ] {
            let text = format!("[agents.claude2]\nof = \"claude\"\nfolder = \"{folder}\"\n");
            let err = parse_in(&text, Some(&data)).unwrap_err();
            assert!(
                err.contains(&format!(
                    "agents.claude2.folder: {folder} is Templum's folder for accounts"
                )),
                "{err}"
            );
        }
        let text = "[agents.claude2]\nof = \"claude\"\nfolder = \"~/templum-data/w\"\n";
        parse_in(text, Some(&data)).unwrap();
    }

    #[test]
    fn an_account_folder_isnt_the_home_folder() {
        for folder in ["~", "~/"] {
            let text = format!("[agents.claude2]\nof = \"claude\"\nfolder = \"{folder}\"\n");
            let err = parse(&text).unwrap_err();
            assert!(
                err.contains(&format!(
                    "agents.claude2.folder: {folder} is your home folder"
                )),
                "{err}"
            );
        }
    }

    /// Every agent's own folder (another agent's files: Codex's account on ~/.claude), however
    /// its variable sets it.
    #[test]
    fn an_account_folder_is_no_agents_own() {
        let home = std::env::home_dir();
        for (of, owner) in [
            ("claude", "claude"),
            ("codex", "claude"),
            ("pi", "codex"),
            ("claude", "gemini"),
        ] {
            let kind = templum_usage::accounts::kind(owner).unwrap();
            let own = kind
                .own_folder(std::env::var_os(kind.var), home.as_deref())
                .unwrap();
            let folder = kind.state_dir(&own);
            let text = format!(
                "[agents.{of}9]\nof = \"{of}\"\nfolder = '{}'\n",
                folder.display()
            );
            let err = parse(&text).unwrap_err();
            assert!(
                err.contains(&format!("is {owner}'s own folder")),
                "{text}\n{err}"
            );
        }
    }

    /// Folders are compared with their links followed: one in another, around Templum's folder
    /// for accounts, or a link to either, is refused.
    #[cfg(unix)]
    #[test]
    fn an_account_folder_is_apart_from_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let (w, data) = (dir.path().join("w"), dir.path().join("data"));
        std::fs::create_dir_all(w.join("skills")).unwrap();
        std::fs::create_dir_all(data.join("accounts/claude3")).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&w, &link).unwrap();
        let to_data = dir.path().join("to-data");
        std::os::unix::fs::symlink(&data, &to_data).unwrap();
        let two = |a: &Path, b: &Path| {
            let text = format!(
                "[agents.claude2]\nof = \"claude\"\nfolder = '{}'\n[agents.claude3]\nof = \"claude\"\nfolder = '{}'\n",
                a.display(),
                b.display()
            );
            parse_in(&text, Some(&data)).map(|_| ()).unwrap_err()
        };
        assert!(two(&w, &w.join("skills")).contains("holds claude3's folder"));
        assert!(two(&w.join("skills"), &w).contains("is in claude3's folder"));
        assert!(
            two(&link, &w).contains("is claude3's folder"),
            "through a link"
        );
        assert!(
            two(&link.join("new"), &w).contains("is in claude3's folder"),
            "not made yet"
        );
        let one = |a: &Path| {
            let text = format!(
                "[agents.claude2]\nof = \"claude\"\nfolder = '{}'\n",
                a.display()
            );
            parse_in(&text, Some(&data)).map(|_| ())
        };
        let err = one(&to_data.join("accounts/claude3")).unwrap_err();
        assert!(err.contains("is Templum's folder for accounts"), "{err}");
        let err = one(&to_data).unwrap_err();
        assert!(err.contains("holds Templum's folder for accounts"), "{err}");
        let err = one(&data.join("nothing/../accounts/claude3")).unwrap_err();
        assert!(err.contains("is Templum's folder for accounts"), "{err}");
        let err = one(&w.join("new/../../data/accounts")).unwrap_err();
        assert!(err.contains("is Templum's folder for accounts"), "{err}");
        one(&w.join("skills")).unwrap();
        one(&dir.path().join("apart")).unwrap();
    }

    /// `claude2` and `Claude2` would be one folder on macOS and Windows.
    #[test]
    fn account_ids_differ_more_than_by_case() {
        let err = parse("[agents.Claude2]\nof = \"claude\"\n[agents.claude2]\nof = \"claude\"\n")
            .unwrap_err();
        assert!(err.contains("is the same id but for case"), "{err}");
        parse("[agents.Claude2]\nof = \"claude\"\n[agents.claude3]\nof = \"claude\"\n").unwrap();
    }

    #[test]
    fn an_account_folder_is_a_full_path() {
        for folder in ["claude-work", ".claude", "~bob/x", "C:foo"] {
            let err = parse(&format!(
                "[agents.claude2]\nof = \"claude\"\nfolder = \"{folder}\"\n"
            ))
            .unwrap_err();
            assert!(
                err.contains(&format!(
                    "agents.claude2.folder: {folder} isn't a full path"
                )),
                "{err}"
            );
        }
        parse("[agents.claude2]\nof = \"claude\"\nfolder = \"~/w\"\n").unwrap();
        parse("[agents.claude2]\nof = \"claude\"\nfolder = \"\"\n").unwrap();
    }

    #[test]
    fn of_must_have_accounts() {
        let err = parse("[agents.aider2]\nof = \"aider\"\n").unwrap_err();
        assert!(err.contains("agents.aider2.of"), "{err}");
        assert!(err.contains("claude, codex"), "{err}");
        let err = parse("[agents.codex]\nof = \"claude\"\n").unwrap_err();
        assert!(
            err.contains("agents.codex.of"),
            "one of Templum's agents isn't an account: {err}"
        );
    }

    #[test]
    fn an_agent_is_saved_and_removed_whole() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "[agents]\n# mine\ncodex = { show = false }\n");
        let mut value = toml_edit::InlineTable::new();
        value.insert("command", "claude --model opus".into());
        save_edits(
            d.path(),
            &[Edit::set("agents", "claude".to_string(), value)],
        )
        .unwrap();
        let c = load(Some(d.path()), None).unwrap();
        assert_eq!(
            c.agents["claude"].command.as_deref(),
            Some("claude --model opus")
        );
        assert_eq!(c.agents["codex"].show, Some(false), "the others stay");
        save_edits(d.path(), &[Edit::remove("agents", "claude".to_string())]).unwrap();
        let c = load(Some(d.path()), None).unwrap();
        assert!(!c.agents.contains_key("claude"), "back to the default");
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("# mine"), "{text}");
    }

    #[test]
    fn edits_keep_the_comments_around_a_key() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            concat!(
                "[font]\n",
                "# family = \"JetBrains Mono\"\n",
                "size = 16   # 6 to 72\n",
                "\n",
                "[theme]\n",
                "# mode = \"system\"\n",
                "light = \"One Light\"   # sunny\n",
            ),
        );
        save_edits(d.path(), &[Edit::set("font", "size", 18_i64)]).unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(
            text.contains(concat!(
                "# family = \"JetBrains Mono\"\n",
                "size = 18   # 6 to 72\n",
            )),
            "{text}"
        );
        save_edits(
            d.path(),
            &[Edit::remove("font", "size"), Edit::remove("theme", "light")],
        )
        .unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("# family = \"JetBrains Mono\""), "{text}");
        assert!(text.contains("# mode = \"system\""), "{text}");
        assert!(!text.contains("size") && !text.contains("light"), "{text}");
        assert_eq!(load(Some(d.path()), None).unwrap(), Config::default());
    }

    #[test]
    fn commit_message_settings_are_read_and_checked() {
        let c = parse(concat!(
            "[git.commit_message]\n",
            "agent = \"claude\"\n",
            "model = \"sonnet\"\n",
            "timeout = 90\n",
            "max_diff_kb = 120\n",
            "instructions = \"Conventional Commits.\"\n",
            "confirmed = true\n",
            "prompt = \"{diff}\"\n",
            "[git.repositories.\"C:/Users/u/my.proj\"]\n",
            "commit_instructions = \"Ticket id first.\"\n",
            "[agents.mine]\n",
            "name = \"Mine\"\n",
            "command = \"mine\"\n",
            "generate = \"mine ask -m {model}\"\n",
            "models = [\"fast\", \"smart\"]\n",
        ))
        .unwrap();
        let m = &c.git.commit_message;
        assert_eq!(
            (m.agent.as_deref(), m.model.as_deref()),
            (Some("claude"), Some("sonnet"))
        );
        assert_eq!((m.timeout, m.max_diff_kb, m.confirmed), (90, 120, true));
        assert_eq!(
            (m.instructions.as_str(), m.prompt.as_deref()),
            ("Conventional Commits.", Some("{diff}"))
        );
        assert_eq!(
            c.git.repositories["C:/Users/u/my.proj"]
                .commit_instructions
                .as_deref(),
            Some("Ticket id first.")
        );
        let mine = &c.agents["mine"];
        assert_eq!(mine.generate.as_deref(), Some("mine ask -m {model}"));
        assert_eq!(
            mine.models.as_deref(),
            Some(["fast".to_string(), "smart".to_string()].as_slice())
        );
        let defaults = Config::default().git.commit_message;
        assert_eq!(
            (defaults.timeout, defaults.max_diff_kb, defaults.confirmed),
            (60, 60, false)
        );
        assert_eq!(
            (defaults.agent, defaults.model, defaults.prompt),
            (None, None, None)
        );
        assert_eq!(defaults.instructions, "");
        for (text, key) in [
            (
                "[git.commit_message]\ntimeout = 4\n",
                "git.commit_message.timeout",
            ),
            (
                "[git.commit_message]\ntimeout = 601\n",
                "git.commit_message.timeout",
            ),
            (
                "[git.commit_message]\nmax_diff_kb = 3\n",
                "git.commit_message.max_diff_kb",
            ),
            (
                "[git.commit_message]\nmax_diff_kb = 1025\n",
                "git.commit_message.max_diff_kb",
            ),
            (
                "[agents.claude]\ngenerate = \" \"\n",
                "agents.claude.generate",
            ),
        ] {
            let err = parse(text).unwrap_err();
            assert!(err.contains(key), "{text}: {err}");
        }
        assert!(
            parse("[git.commit_message]\nmodle = \"x\"\n").is_err(),
            "unknown keys still fail"
        );
    }

    #[test]
    fn an_edit_writes_into_a_nested_table() {
        let d = tempfile::tempdir().unwrap();
        save_edits(
            d.path(),
            &[Edit::set("git.commit_message", "model", "smart")],
        )
        .unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(
            text.contains("[git.commit_message]") && text.contains("model = \"smart\""),
            "{text}"
        );
        assert!(
            !text.contains("[git]"),
            "a parent made on the way has no line: {text}"
        );
        // An existing [git] keeps its keys and their comments.
        write(d.path(), "[git]\nprogram = \"/opt/git\" # mine\n");
        save_edits(
            d.path(),
            &[Edit::set("git.commit_message", "timeout", 90_i64)],
        )
        .unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("program = \"/opt/git\" # mine"), "{text}");
        let c = load(Some(d.path()), None).unwrap();
        assert_eq!(
            (c.git.program.as_str(), c.git.commit_message.timeout),
            ("/opt/git", 90)
        );
        save_edits(d.path(), &[Edit::remove("git.commit_message", "timeout")]).unwrap();
        assert_eq!(
            load(Some(d.path()), None)
                .unwrap()
                .git
                .commit_message
                .timeout,
            60
        );
        // Removing from a table that isn't there adds nothing.
        save_edits(d.path(), &[Edit::remove("git.nothing", "x")]).unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(!text.contains("nothing"), "{text}");
        // A path through a value that isn't a table is named whole.
        write(d.path(), "git = 3\n");
        let err =
            save_edits(d.path(), &[Edit::set("git.commit_message", "model", "x")]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "`git.commit_message` in config.toml is not a table"
        );
    }

    #[test]
    fn a_repositorys_table_is_one_quoted_key() {
        let d = tempfile::tempdir().unwrap();
        let roots = ["/home/u/my.proj", "C:/Users/u/my proj"];
        let edits: Vec<Edit> = roots
            .iter()
            .map(|r| {
                Edit::set(
                    repository_table(r),
                    "commit_instructions",
                    format!("for {r}"),
                )
            })
            .collect();
        save_edits(d.path(), &edits).unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(
            text.contains("[git.repositories.\"/home/u/my.proj\"]"),
            "{text}"
        );
        let c = load(Some(d.path()), None).unwrap();
        for r in roots {
            assert_eq!(
                c.git.repositories[r].commit_instructions.as_deref(),
                Some(format!("for {r}").as_str())
            );
        }
        save_edits(
            d.path(),
            &[Edit::remove(
                repository_table(roots[0]),
                "commit_instructions",
            )],
        )
        .unwrap();
        let c = load(Some(d.path()), None).unwrap();
        assert_eq!(
            c.git
                .repositories
                .get(roots[0])
                .and_then(|r| r.commit_instructions.clone()),
            None
        );
        assert!(
            c.git.repositories[roots[1]].commit_instructions.is_some(),
            "the other stays"
        );
    }
}
