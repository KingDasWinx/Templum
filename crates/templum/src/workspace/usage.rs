//! Agents' session usage and plan limits: the Claude Code status line Templum hands its relay,
//! reading what the agents write after each snapshot, and the notifications
//! (docs/specs/usage.md).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::component::notification::{Notification, NotificationDelivery, NotificationType};
use gpui_kit::{Context, SharedString, Window};
use templum_canvas::BlockId;
use templum_terminal::Agent;
use templum_usage::{Event, Limits, Proc, Session, Watch};

use super::{Pane, TryIt, Workspace, show_notification};
use crate::agents::{self, AgentEntry};
use crate::config::Edit;
use crate::notes;
use crate::usage::{TURN_SECS, limit_name, meter_agents, resets};

/// How long, in seconds, a plan's limits go unasked when nothing makes Templum ask (the meter turning
/// to them, the popover, a turn's end): Claude Code's `/usage` asks Anthropic, which limits how often.
pub(super) const CLAUDE_EVERY: u64 = 5 * 60;
/// Templum never asks the same account's `/usage` again within this many seconds.
pub(super) const CLAUDE_MIN: u64 = 60;
/// How often Templum looks for limits that are due.
pub(super) const CLAUDE_LOOK: Duration = Duration::from_secs(15);

/// What `/usage` is asked with: by whose limits they are, the Claude Code to run, the empty folder
/// to run it in and the account's folder (None: the agent's own).
pub(super) type Job = (String, PathBuf, PathBuf, Option<PathBuf>);

/// What a read found: each terminal's session, in the order asked, and each agent's limits.
pub(crate) type Read = (Vec<Option<Session>>, Vec<(String, Limits)>);

/// What Templum gives the Claude Code and Codex it opens or a person types (`Workspace::gives`):
/// its data folder and program, whether connections are on with a bus, whether Codex's flags
/// can be typed in the shell (not cmd nor Windows PowerShell 5.1), and whether `templum` is on
/// the terminal's PATH (Codex's hooks run it).
struct Gives {
    data: PathBuf,
    exe: PathBuf,
    connected: bool,
    codex: bool,
    on_path: bool,
}

/// Qwen Code's settings file with Templum's hooks, for its variable; None when its own system file
/// exists.
fn qwen_settings(data: &Path, windows: bool) -> Option<String> {
    let system = templum_usage::qwen::system_file();
    templum_usage::qwen::settings(data, windows, &system).map(|f| f.to_string_lossy().into_owned())
}

/// Templum's own environment has `name`, not empty: the person's, which an agent's variable
/// doesn't replace (`Workspace::agent_vars`).
pub(super) fn in_env(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

impl Workspace {
    /// The agent of `pane` as the last snapshot found it, for a read of its session: an account's
    /// is its agent, in the account's folder.
    pub(super) fn proc_of(&self, pane: &Pane) -> Option<Proc> {
        let seen = &pane.seen;
        let id = seen.agent.clone()?;
        let account = self
            .agents
            .iter()
            .find(|a| a.id == id)
            .and_then(|a| a.account.as_ref());
        Some(Proc {
            agent: account.map_or(id.clone(), |a| a.kind.agent.to_string()),
            pid: seen.pid?,
            started: seen.started?,
            cwd: seen.folder.clone(),
            terminal: pane.term.id().to_string(),
            home: account.map(|a| a.kind.state_dir(&a.folder)),
            account: account.map(|_| id),
        })
    }

    /// `command` as typed to start `agent` in a terminal running `shell` (a program; None: the
    /// default shell; docs/specs/connections.md, What Templum types into each agent): the Claude
    /// Code Templum opens gets its settings file (the status line of the plan's limits, and the
    /// hooks) and its MCP server; Codex gets the server and its hooks (those only with `templum`
    /// on the terminal's PATH), unless the shell is cmd (they go in single quotes) or Windows
    /// PowerShell 5.1 (it strips the `"` inside them; pwsh 7 doesn't). Nothing under WSL, where
    /// Templum's paths aren't the shell's, nor without a data folder.
    pub(super) fn typed_in(
        &self,
        shell: Option<&str>,
        agent: Option<&AgentEntry>,
        command: &str,
    ) -> String {
        let builtin = agent.and_then(|a| a.builtin);
        let Some(gives) = self.gives(shell) else {
            return command.to_string();
        };
        let windows = cfg!(windows);
        let mut line = command.to_string();
        if builtin == Some(Agent::CLAUDE) {
            let limits = self.config.usage.claude_limits;
            line.push_str(&claude_flags(
                &gives.data,
                &gives.exe,
                windows,
                limits,
                gives.connected,
            ));
        } else if builtin == Some(Agent::CODEX)
            && gives.codex
            && let Some(flags) =
                templum_mcp::codex_flags(&gives.exe.to_string_lossy(), windows, gives.on_path)
        {
            line.push_str(&flags);
        } else if let Some(words) = (gives.connected && gives.on_path)
            .then(|| match builtin {
                Some(Agent::COPILOT) => templum_usage::copilot::args(&gives.data, windows),
                Some(Agent::QWEN) => templum_usage::qwen::args(&gives.data, windows),
                Some(Agent::PI) => templum_usage::pi::args(&gives.data, windows),
                _ => None,
            })
            .flatten()
            .and_then(|w| typed_words(&w, windows))
        {
            line.push_str(&words);
        }
        line
    }

    /// What a new terminal's environment gets for the agents a person types there
    /// (docs/specs/connections.md, Agents typed in a terminal; Typed by hand: the shells): the
    /// words `typed_in` would append after each, unquoted, one per line, for the terminal's own
    /// `shell` (a program; None: the default). Each always, empty when there is nothing to give,
    /// so a Templum started from another's terminal doesn't pass that one's on.
    pub(super) fn agent_env(&self, shell: Option<&str>) -> Vec<(String, String)> {
        let windows = cfg!(windows);
        let gives = self.gives(shell);
        let claude = gives.as_ref().map(|g| {
            let limits = self.config.usage.claude_limits;
            claude_args(&g.data, &g.exe, windows, limits, g.connected)
        });
        let codex = gives
            .as_ref()
            .filter(|g| g.codex)
            .and_then(|g| templum_mcp::codex_args(&g.exe.to_string_lossy(), windows, g.on_path));
        // The others' files run `<data>/bin/templum`: only where that is on the PATH.
        let data = gives
            .as_ref()
            .filter(|g| g.connected && g.on_path)
            .map(|g| g.data.as_path());
        let qwen = data.and_then(|d| templum_usage::qwen::args(d, windows));
        let words = |args: Option<Vec<String>>| args.unwrap_or_default().join("\n");
        // The JSON `OPENCODE_CONFIG_CONTENT` and `KILO_CONFIG_CONTENT` take.
        let opencode = data
            .and_then(|d| templum_usage::opencode::config(d, windows))
            .unwrap_or_default();
        // Empty when Qwen Code's own system settings file exists.
        let qwen_settings = data
            .filter(|_| qwen.is_some())
            .and_then(|d| qwen_settings(d, windows))
            .unwrap_or_default();
        [
            ("TEMPLUM_CLAUDE_ARGS", words(claude)),
            ("TEMPLUM_CODEX_ARGS", words(codex)),
            (
                "TEMPLUM_COPILOT_ARGS",
                words(data.and_then(|d| templum_usage::copilot::args(d, windows))),
            ),
            ("TEMPLUM_OPENCODE_CONFIG", opencode.clone()),
            ("TEMPLUM_KILO_CONFIG", opencode),
            ("TEMPLUM_QWEN_ARGS", words(qwen)),
            ("TEMPLUM_QWEN_SETTINGS", qwen_settings),
            (
                "TEMPLUM_PI_ARGS",
                words(data.and_then(|d| templum_usage::pi::args(d, windows))),
            ),
            // A function per account in the shell, always set: empty with none.
            ("TEMPLUM_ACCOUNTS", agents::accounts_env(&self.agents)),
            // An OpenCode or a Pi run here reports for its block, and sets it for what it starts.
            ("TEMPLUM_HOOKED", String::new()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    }

    /// The variables the shell of a block Templum opens for `agent` gets: an account's folder
    /// first, whatever Templum's environment has (docs/specs/accounts.md); then
    /// (docs/specs/connections.md, OpenCode and Kilo Code) its server and plugin, for an agent
    /// that takes them only from its environment, as `typed_in` says, none where `set(name)` says
    /// the person's environment already has a non-empty one, which wins. `shell`: the program the
    /// block runs, as `typed_in`'s (None: the default shell); none of the server's under WSL.
    pub(super) fn agent_vars(
        &self,
        agent: Option<&AgentEntry>,
        shell: Option<&str>,
        set: &dyn Fn(&str) -> bool,
    ) -> Vec<(String, String)> {
        let account = agent
            .and_then(AgentEntry::var)
            .map(|(name, folder)| (name.to_string(), folder.to_string_lossy().into_owned()));
        account
            .into_iter()
            .chain(self.server_vars(agent, shell, set))
            .collect()
    }

    /// `agent_vars`' server and plugin.
    fn server_vars(
        &self,
        agent: Option<&AgentEntry>,
        shell: Option<&str>,
        set: &dyn Fn(&str) -> bool,
    ) -> Vec<(String, String)> {
        let builtin = agent.and_then(|a| a.builtin);
        let name = match builtin {
            Some(Agent::OPENCODE) => "OPENCODE_CONFIG_CONTENT",
            Some(Agent::KILO) => "KILO_CONFIG_CONTENT",
            Some(Agent::QWEN) => "QWEN_CODE_SYSTEM_SETTINGS_PATH",
            _ => return Vec::new(),
        };
        let Some(gives) = self.gives(shell).filter(|g| g.connected && g.on_path) else {
            return Vec::new();
        };
        if set(name) {
            return Vec::new();
        }
        let windows = cfg!(windows);
        let value = match builtin {
            Some(Agent::QWEN) => qwen_settings(&gives.data, windows),
            _ => templum_usage::opencode::config(&gives.data, windows),
        };
        value
            .map(|value| vec![(name.to_string(), value)])
            .unwrap_or_default()
    }

    /// What Templum gives the agents it opens or a person types in a terminal running `shell`
    /// (None: the default), as `typed_in` says; None without a data folder or a program, or under
    /// WSL, where Templum's paths aren't the shell's.
    fn gives(&self, shell: Option<&str>) -> Option<Gives> {
        let (Some(data), Some(exe), false) =
            (&self.data_dir, program(), self.shell_is(shell, "wsl"))
        else {
            return None;
        };
        let connected = self.config.connections.enabled && self.bus.is_some();
        Some(Gives {
            data: data.clone(),
            exe,
            connected,
            codex: connected && !self.shell_is(shell, "cmd") && !self.shell_is(shell, "powershell"),
            // `terminal_env` puts `<data>/bin`, where `templum` is, first on it.
            on_path: self.bus_env.iter().any(|(k, _)| k == "PATH"),
        })
    }

    /// A terminal running `shell` (a program; None: new terminals' default) runs the shell named
    /// `name` (its program's file name, in any case: `PowerShell.exe` is `powershell`): the
    /// terminal's own, else the configured one, else the system's default.
    pub(super) fn shell_is(&self, shell: Option<&str>, name: &str) -> bool {
        let program = shell.map(str::to_string).or_else(|| {
            self.config
                .shell
                .program
                .clone()
                .or_else(|| templum_terminal::default_shell().map(|s| s.program))
        });
        program.is_some_and(|p| templum_terminal::is_shell(&p, name))
    }

    /// Reads, in the background, what the agents in every workspace's terminals wrote, from the
    /// processes the last snapshot found. Without a data folder, nothing.
    pub(super) fn read_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(data) = self.data_dir.clone() else {
            return;
        };
        let mut keys = Vec::new();
        let mut procs = Vec::new();
        for space in self.spaces() {
            for (id, pane) in space.terminals() {
                if let Some(proc) = self.proc_of(pane) {
                    keys.push((space.id.clone(), id));
                    procs.push(proc);
                }
            }
        }
        let homes = agents::usage_homes(&self.agents);
        let dirs = agents::dirs(&data);
        // The next read replaces this one: a slow disk never piles reads up.
        self._usage = cx.spawn_in(window, async move |ws, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { templum_usage::read(&procs, &dirs, &homes, notes::now()) })
                .await;
            ws.update_in(cx, |ws, window, cx| ws.usage_done(keys, read, window, cx))
                .ok();
        });
    }

    /// Keeps what a read found for the terminals `keys` (workspace, block), then tells the user
    /// what the limits mean now.
    pub(crate) fn usage_done(
        &mut self,
        keys: Vec<(String, BlockId)>,
        (sessions, limits): Read,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut limits = limits;
        // What `/usage` found for an account that is gone, or no longer asked, goes with it.
        let asked: Vec<String> = self.claude_agents().map(|a| a.id.clone()).collect();
        self.claude_polled.retain(|id, _| asked.contains(id));
        self.claude_asked.retain(|id, _| asked.contains(id));
        for (id, polled) in &self.claude_polled {
            templum_usage::merge(&mut limits, id, polled);
        }
        // A Codex folder whose sessions another shares reads only its running processes': with
        // none running, its limits stay as last read.
        for home in agents::usage_homes(&self.agents)
            .iter()
            .filter(|h| h.sessions_shared)
        {
            if let Some((_, old)) = self.limits.iter().find(|(id, _)| *id == home.id) {
                templum_usage::merge(&mut limits, &home.id, old);
            }
        }
        let mut changed = self.limits != limits;
        self.limits = limits;
        // The meter's next agent, every few seconds, whose limits are asked at once.
        let now = notes::now();
        let shown: Vec<String> = meter_agents(self, now)
            .into_iter()
            .map(|(agent, _)| agent.to_string())
            .collect();
        if shown.len() > 1 && !self.meter_hovered && now >= self.meter_turned + TURN_SECS {
            self.meter_turn = self.meter_turn.wrapping_add(1);
            self.meter_turned = now;
            changed = true;
            self.ask_claude(Some(&shown[self.meter_turn % shown.len()]), CLAUDE_MIN, cx);
        }
        for space in std::iter::once(&mut self.space).chain(&mut self.others) {
            let ids: Vec<BlockId> = space.terminals().map(|(id, _)| id).collect();
            for id in ids {
                let found = keys
                    .iter()
                    .position(|(s, b)| *s == space.id && *b == id)
                    .and_then(|i| sessions.get(i).cloned().flatten());
                let pane = space.terminal_mut(id).expect("a terminal");
                if pane.usage != found {
                    pane.usage = found;
                    changed = true;
                }
            }
        }
        let usage = &self.config.usage;
        let events = self.watch.step(now, &self.limits, usage.reset, usage.warn);
        for event in events {
            self.tell(event, window, cx);
        }
        if changed {
            cx.notify();
        }
    }

    /// A notification about a plan limit; clicking it shows a terminal running that agent.
    fn tell(&self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        let now = notes::now();
        let (agent, title, message, kind) = match &event {
            Event::Warn { agent, window } => (
                agent,
                format!(
                    "{}: {:.0}% of the {} limit used",
                    self.agent_title(agent),
                    window.used_percent,
                    limit_name(window.minutes, window.scope.as_deref())
                ),
                format!("Resets {}", resets(window.resets_at, now, false)),
                NotificationType::Warning,
            ),
            Event::Reset {
                agent,
                minutes,
                scope,
                ..
            } => (
                agent,
                format!(
                    "{}'s {} limit has reset",
                    self.agent_title(agent),
                    limit_name(*minutes, scope.as_deref())
                ),
                "It can work again".to_string(),
                NotificationType::Success,
            ),
        };
        let at = self.running(agent);
        let ws = cx.entity().downgrade();
        let notification = Notification::new()
            .id1::<Watch>(SharedString::from(format!("usage-{agent}-{title}")))
            .with_type(kind)
            .title(title)
            .message(message)
            .delivery(self.delivery(window))
            .on_click(move |_, window, cx| {
                if let Some((space, block)) = &at {
                    ws.update(cx, |ws, cx| ws.reveal(space, *block, window, cx))
                        .ok();
                }
            });
        show_notification(window, cx, notification);
    }

    /// Asks `/usage` for the limits that are due: those not asked for `CLAUDE_EVERY` seconds, and
    /// the one agent the meter shows, when it shows one, every `CLAUDE_MIN`.
    pub(super) fn ask_claude_due(&mut self, cx: &mut Context<Self>) {
        self.ask_claude(None, CLAUDE_EVERY, cx);
        let shown: Vec<String> = meter_agents(self, notes::now())
            .into_iter()
            .map(|(agent, _)| agent.to_string())
            .collect();
        if let [agent] = shown.as_slice() {
            self.ask_claude(Some(agent), CLAUDE_MIN, cx);
        }
    }

    /// Asks `/usage` for the account `only` (None: each) if it wasn't asked for `older` seconds
    /// (at least `CLAUDE_MIN`): the probes run one at a time, `only` first. What one finds shows
    /// at once.
    pub(super) fn ask_claude(&mut self, only: Option<&str>, older: u64, cx: &mut Context<Self>) {
        for job in self.claude_due(notes::now(), only, older) {
            match only {
                Some(_) => self.claude_queue.push_front(job),
                None => self.claude_queue.push_back(job),
            }
        }
        self.next_probe(cx);
    }

    /// Runs the next waiting probe on a thread of its own, unless one runs: `poll` waits for
    /// Claude Code for up to 30 s, which the background pool's threads, few on a small machine,
    /// are not for. Tests never run the user's Claude Code.
    fn next_probe(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) {
            return;
        }
        let Some((id, program, dir, config_dir)) = self.claude_next(notes::now()) else {
            return;
        };
        cx.spawn(async move |ws, cx| {
            let (found, asked) = oneshot::channel();
            std::thread::spawn(move || {
                let polled = templum_usage::claude::poll(
                    &program,
                    &dir,
                    config_dir.as_deref(),
                    notes::now(),
                );
                let _ = found.send(polled);
            });
            let found = asked.await.ok().flatten();
            ws.update(cx, |ws, cx| ws.probe_done(id, found, cx)).ok();
        })
        .detach();
    }

    /// The next waiting probe of an account still asked about, now running, asked for at `now`;
    /// None while one runs, or when none waits.
    pub(super) fn claude_next(&mut self, now: u64) -> Option<Job> {
        if self.claude_probing.is_some() {
            return None;
        }
        while let Some(job) = self.claude_queue.pop_front() {
            if self.claude_agents().any(|a| a.id == job.0) {
                self.claude_asked.insert(job.0.clone(), now);
                self.claude_probing = Some(job.0.clone());
                return Some(job);
            }
        }
        None
    }

    /// The probe of `id` ended with `found`: a failed ask keeps what the last one found, and isn't
    /// asked again by a trigger for `CLAUDE_EVERY`. The next waiting one starts.
    pub(super) fn probe_done(&mut self, id: String, found: Option<Limits>, cx: &mut Context<Self>) {
        self.claude_probing = None;
        match found {
            Some(found) => self.claude_polled(id, found, cx),
            // An API key has no plan and a machine offline no answer: only the long look asks again.
            None => {
                let wait = CLAUDE_EVERY - CLAUDE_MIN;
                self.claude_asked.insert(id, notes::now() + wait);
            }
        }
        self.next_probe(cx);
    }

    /// The jobs of `claude_poll_jobs` that are due at `now`, marked as asked: `only` (None: each)
    /// if it wasn't asked for `older` seconds, and doesn't wait or run already. Nothing is looked
    /// up when none is.
    pub(super) fn claude_due(&mut self, now: u64, only: Option<&str>, older: u64) -> Vec<Job> {
        let due = |id: &str| {
            only.is_none_or(|only| only == id)
                && self.claude_probing.as_deref() != Some(id)
                && !self.claude_queue.iter().any(|(queued, ..)| queued == id)
                && self.claude_asked.get(id).is_none_or(|t| now >= t + older)
        };
        if !self.claude_agents().any(|a| due(&a.id)) {
            return Vec::new();
        }
        let jobs: Vec<Job> = self
            .claude_poll_jobs()
            .into_iter()
            .filter(|(id, ..)| due(id))
            .collect();
        for (id, ..) in &jobs {
            self.claude_asked.insert(id.clone(), now);
        }
        jobs
    }

    /// `/usage` found the limits `found` for the account `id`: they join the others now, not at
    /// the next read. Not for one that is gone, nor older than what it found before.
    pub(super) fn claude_polled(&mut self, id: String, found: Limits, cx: &mut Context<Self>) {
        if !self.claude_agents().any(|a| a.id == id)
            || self
                .claude_polled
                .get(&id)
                .is_some_and(|old| old.at > found.at)
        {
            return;
        }
        let mut limits = self.limits.clone();
        templum_usage::merge(&mut limits, &id, &found);
        if limits != self.limits {
            self.limits = limits;
            cx.notify();
        }
        self.claude_polled.insert(id, found);
    }

    /// The agents `/usage` is asked about: Claude Code, and each of its accounts that has a login
    /// (its `.claude.json`, which is in the account's folder on every OS, the Keychain's token
    /// or not), whether it ran since Templum started or not. None when the user turned it off, or
    /// without a data folder.
    fn claude_agents(&self) -> impl Iterator<Item = &AgentEntry> {
        let on = self.data_dir.is_some() && self.config.usage.claude_limits;
        self.agents.iter().filter(move |a| {
            on && match &a.account {
                None => a.builtin == Some(Agent::CLAUDE),
                Some(account) => {
                    account.kind.agent == "claude" && account.folder.join(".claude.json").is_file()
                }
            }
        })
    }

    /// What to ask for the plan's limits: for Claude Code's own folder and each account of
    /// `claude_agents`. None when it isn't installed.
    pub(super) fn claude_poll_jobs(&self) -> Vec<Job> {
        let Some(data) = self.data_dir.as_ref() else {
            return Vec::new();
        };
        let Some(claude) = self
            .claude_agents()
            .find(|a| a.builtin == Some(Agent::CLAUDE))
        else {
            return Vec::new();
        };
        let Some(program) = claude.command.split_whitespace().next() else {
            return Vec::new();
        };
        let program = if program.contains(['/', '\\']) {
            PathBuf::from(program)
        } else {
            let Some(found) = templum_terminal::locate(program) else {
                return Vec::new();
            };
            found
        };
        let probe = data.join("usage").join("probe");
        self.claude_agents()
            .map(|a| {
                let folder = a.account.as_ref().map(|account| account.folder.clone());
                (a.id.clone(), program.clone(), probe.clone(), folder)
            })
            .collect()
    }

    /// Shows `agent`'s limits in the title bar, or stops (Settings ▸ Usage).
    pub(crate) fn show_in_title_bar(&mut self, agent: &str, shown: bool, cx: &mut Context<Self>) {
        let current = self.pending.as_ref().unwrap_or(&self.config);
        let mut hide: Vec<String> = current
            .usage
            .hide
            .iter()
            .filter(|a| *a != agent)
            .cloned()
            .collect();
        if !shown {
            hide.push(agent.to_string());
        }
        let edit = if hide.is_empty() {
            Edit::remove("usage", "hide")
        } else {
            Edit::set(
                "usage",
                "hide",
                hide.iter()
                    .map(String::as_str)
                    .collect::<toml_edit::Array>(),
            )
        };
        self.edit(edit, move |c| c.usage.hide = hide, cx);
    }

    /// Opens or closes the title bar's usage popover.
    pub(crate) fn toggle_usage(&mut self, cx: &mut Context<Self>) {
        self.usage_open = !self.usage_open;
        if self.usage_open {
            self.ask_claude(None, CLAUDE_MIN, cx);
        }
        cx.notify();
    }

    /// Shows the terminal of a session listed in the usage popover, and closes it.
    pub(crate) fn show_session(
        &mut self,
        space: &str,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.usage_open = false;
        self.reveal(space, block, window, cx);
    }

    /// An agent's name in menus, from its id.
    pub(crate) fn agent_title(&self, id: &str) -> String {
        self.agents
            .iter()
            .find(|a| a.id == id)
            .map_or_else(|| id.to_string(), |a| a.title.clone())
    }

    /// A terminal running the agent `id`, in the active workspace first.
    fn running(&self, id: &str) -> Option<(String, BlockId)> {
        self.spaces().find_map(|space| {
            space
                .terminals()
                .find(|(_, pane)| pane.seen.agent.as_deref() == Some(id))
                .map(|(block, _)| (space.id.clone(), block))
        })
    }

    /// How news reaches the user: also as a system notification while Templum is in the
    /// background, unless turned off.
    pub(super) fn delivery(&self, window: &Window) -> NotificationDelivery {
        if self.config.terminal.notifications && !window.is_window_active() {
            NotificationDelivery::InAppAndSystem
        } else {
            NotificationDelivery::InApp
        }
    }

    /// A Gemini CLI just opened (`entry`) without Templum's extension, while connections are on:
    /// once per run, a notice whose click asks to install it.
    pub(super) fn tell_gemini(
        &mut self,
        entry: Option<&AgentEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = entry.filter(|e| e.builtin == Some(Agent::GEMINI)) else {
            return;
        };
        if self.gemini_told.contains(&entry.id) || !self.gives(None).is_some_and(|g| g.connected) {
            return;
        }
        let Some(data) = &self.data_dir else {
            return;
        };
        // An account's extensions are its folder's, or the folder it shares them with.
        let dir = match &entry.account {
            Some(a) => Some(a.kind.state_dir(&a.folder)),
            None => agents::dirs(data).gemini,
        };
        if dir.is_some_and(|dir| templum_usage::gemini::installed(&dir)) {
            return;
        }
        self.gemini_told.insert(entry.id.clone());
        let account = entry.account.is_some().then(|| entry.id.clone());
        let ws = cx.entity().downgrade();
        let notification = Notification::new()
            .with_type(NotificationType::Warning)
            .title("Gemini CLI has no Templum tools or status dot")
            .message(
                "Click to install Templum's extension: every Gemini CLI you open after gets them.",
            )
            .delivery(self.delivery(window))
            .on_click(move |_, window, cx| {
                let (ws, account) = (ws.clone(), account.clone());
                let (title, text, ok) = crate::settings::connections::INSTALL;
                crate::prompt::confirm(
                    title,
                    text,
                    ok,
                    move |_, cx| {
                        ws.update(cx, |ws, cx| ws.setup_gemini(true, account.as_deref(), cx))
                            .ok();
                    },
                    window,
                    cx,
                );
            });
        show_notification(window, cx, notification);
    }

    /// Settings ▸ Connections ▸ Gemini CLI: installs Templum's extension (`install`) or removes
    /// it, in the background, for Gemini CLI's own folder or the account `account`'s; Settings
    /// shows what went wrong, if anything did.
    pub(crate) fn setup_gemini(
        &mut self,
        install: bool,
        account: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let Some(data) = self.data_dir.clone() else {
            return;
        };
        let env = account
            .and_then(|id| self.agents.iter().find(|a| a.id == id))
            .and_then(AgentEntry::var)
            .map(|(name, folder)| (name.to_string(), folder.to_string_lossy().into_owned()));
        if matches!(self.gemini_job, Some(TryIt::Running(_))) {
            return;
        }
        self.gemini_job = Some(TryIt::Running(Arc::default()));
        cx.notify();
        cx.spawn(async move |ws, cx| {
            let done = cx
                .background_executor()
                .spawn(async move {
                    gemini_run(templum_terminal::locate("gemini"), &data, install, env)
                })
                .await;
            ws.update(cx, |ws, cx| {
                ws.gemini_job = done.err().map(TryIt::Failed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// Runs Gemini CLI (`gemini`, where Templum found it, if it did) to link Templum's extension,
/// written first (`install`), or to uninstall it, with `env` (an account's folder); what went
/// wrong, whole, if anything did.
// ponytail: no deadline; a Gemini CLI that hangs keeps the button spinning until Templum quits.
pub(super) fn gemini_run(
    gemini: Option<PathBuf>,
    data: &Path,
    install: bool,
    env: Option<(String, String)>,
) -> Result<(), String> {
    use templum_usage::gemini;
    let program = gemini.ok_or("Templum didn't find Gemini CLI (gemini) on this computer.")?;
    let args: Vec<OsString> = if install {
        let folder = gemini::extension(data, cfg!(windows)).ok_or(
            "Templum couldn't write its extension: its folder's path holds a ' or a line break, \
             or can't be written.",
        )?;
        gemini::link_args(&folder)
    } else {
        gemini::UNLINK.iter().map(OsString::from).collect()
    };
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .envs(env)
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = command
        .output()
        .map_err(|e| format!("Couldn't run Gemini CLI: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Err(format!(
        "Gemini CLI failed ({}):\n{}",
        out.status,
        said.trim()
    ))
}

/// What the Claude Code Templum opens gets after its command, given Templum's data folder and
/// program: the settings file (the status line when `limits`, the hooks when `connected`), then
/// the MCP server, only when that file carries the hooks: without them its dot would never move.
pub(super) fn claude_flags(
    data: &Path,
    exe: &Path,
    windows: bool,
    limits: bool,
    connected: bool,
) -> String {
    let mut flags = String::new();
    let settings = templum_usage::claude::settings_arg(data, exe, windows, limits, connected);
    if let Some(arg) = &settings {
        flags.push_str(&format!(" --settings {arg}"));
    }
    // Last: it takes every word after it.
    if connected
        && settings.is_some()
        && let Some(arg) = templum_usage::claude::mcp_arg(data, exe, windows)
    {
        flags.push_str(&format!(" --mcp-config {arg}"));
    }
    flags
}

/// `words`, each typed as a shell takes it (`typed_path`), each after a space; None when one can't
/// be.
fn typed_words(words: &[String], windows: bool) -> Option<String> {
    words
        .iter()
        .map(|w| {
            Some(format!(
                " {}",
                templum_usage::claude::typed_path(w, windows)?
            ))
        })
        .collect()
}

/// `claude_flags` as the words a program gets, unquoted, Windows' `\` as `/`; the MCP config as
/// one word, `--mcp-config=<path>`: alone, `--mcp-config` would take the person's words after it.
pub(super) fn claude_args(
    data: &Path,
    exe: &Path,
    windows: bool,
    limits: bool,
    connected: bool,
) -> Vec<String> {
    let word = |path: PathBuf| {
        let path = path.to_string_lossy().into_owned();
        if windows {
            path.replace('\\', "/")
        } else {
            path
        }
    };
    let mut args = Vec::new();
    let Some(settings) =
        templum_usage::claude::settings_file(data, exe, windows, limits, connected)
    else {
        return args;
    };
    args.extend(["--settings".to_string(), word(settings)]);
    if connected && let Some(mcp) = templum_usage::claude::mcp_file(data, exe, windows) {
        args.push(format!("--mcp-config={}", word(mcp)));
    }
    args
}

/// Templum's program as an agent should run it: the AppImage when it runs from one (its mount
/// goes away when it quits), else the running executable, without the " (deleted)" Linux adds
/// to one an upgrade replaced.
pub(super) fn program() -> Option<PathBuf> {
    if let Some(image) = std::env::var_os("APPIMAGE") {
        return Some(image.into());
    }
    let exe = std::env::current_exe().ok()?;
    Some(
        match exe.to_str().and_then(|e| e.strip_suffix(" (deleted)")) {
            Some(path) => PathBuf::from(path),
            None => exe,
        },
    )
}
