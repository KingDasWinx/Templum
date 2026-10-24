//! Settings ▸ Agents ▸ Accounts: a row per account, "Add an account", and the dialog that adds or
//! edits one (docs/specs/accounts.md, Settings ▸ Agents ▸ Accounts).

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::dialog::Confirm;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Icon, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, Entity, FontWeight, Hsla, SharedString,
    TestSupportExt, Window, div, px,
};
use templum_terminal::ShellOption;
use templum_usage::accounts::{Found, Kind};

use super::agents::shell_choices;
use super::row::{Row, menu_button};
use crate::agents::{self, AccountDraft, AgentEntry, own_state, tilde};
use crate::kanban::{danger, pill, segment, strip};
use crate::theme::Ui;
use crate::workspace::{Workspace, show_notification};

/// The dialog's width, and the space at its sides: the card dialog's language.
const DIALOG_W: f32 = 480.0;
const SIDE: f32 = 20.0;

/// The Accounts heading, its rows and "Add an account", under the agents.
pub(super) fn accounts(
    ws: &Workspace,
    entity: &Entity<Workspace>,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let rows: Vec<Row> = ws
        .agents
        .iter()
        .filter(|a| a.account.is_some())
        .map(|a| account_row(ws, entity, a))
        .collect();
    // The commands folder off the person's PATH: said once, for the first account with a command.
    let with_command = ws
        .commands
        .iter()
        .find_map(|(id, state)| state.is_ok().then_some(id));
    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .mt(px(14.0))
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Accounts"),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(ui.title_dim)
                .child("Another login of an agent, opened by its own command."),
        )
        .children(with_command.and_then(|id| path_note(ui, id)))
        .when(!rows.is_empty(), |el| el.child(super::card(ui, rows)))
        .child(
            div()
                .id("add-account")
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(14.0))
                .py(px(10.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(ui.hairline)
                .text_size(px(13.0))
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .active(move |s| s.bg(ui.pressed))
                .child(Icon::new(IconName::Plus).size(px(14.0)))
                .child("Add an account")
                .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                    account_dialog(ws, None, window, cx)
                }))
                .test_support(),
        )
        .into_any_element()
}

/// What to do when the commands folder isn't on the PATH the person's shell gets, so `id` works
/// only in Templum's terminals (docs/specs/accounts.md, Its command); None when it is.
fn path_note(ui: Ui, id: &str) -> Option<AnyElement> {
    let dir = agents::commands_dir()?;
    // The shell's PATH is still being read: what it lacks may only be unread.
    if templum_terminal::shell_path_pending()
        || templum_terminal::on_path(&dir, &templum_terminal::shell_path_dirs())
    {
        return None;
    }
    let note = |text: String| {
        div()
            .text_size(px(11.0))
            .text_color(ui.title_dim)
            .child(text)
    };
    Some(if cfg!(windows) {
        note(format!(
            "Add %USERPROFILE%\\.local\\bin to your Path to use {id} outside Templum."
        ))
        .into_any_element()
    } else {
        div()
            .id("account-path-note")
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(note(format!(
                "Add this to your shell's startup file to use {id} outside Templum:"
            )))
            .child(
                div().text_size(px(11.0)).child(
                    TextView::markdown(
                        "account-path-line",
                        "```\nexport PATH=\"$HOME/.local/bin:$PATH\"\n```",
                    )
                    .style(crate::markdown::style(1.0))
                    .selectable(true),
                ),
            )
            .test_support()
            .into_any_element()
    })
}

/// The agent an account is of, by its title ("Claude Code").
fn agent_title(agents: &[AgentEntry], kind: &Kind) -> String {
    agents
        .iter()
        .find(|a| a.id == kind.agent)
        .map_or_else(|| kind.agent.to_string(), |a| a.title.clone())
}

/// One account: its agent and folder; whether the canvas's menu lists it; Edit.
fn account_row(ws: &Workspace, entity: &Entity<Workspace>, entry: &AgentEntry) -> Row {
    let ui = ws.ui;
    let id = entry.id.clone();
    let account = entry.account.as_ref().expect("an account");
    let agent = agent_title(&ws.agents, account.kind);
    let description = match ws.commands.get(&id) {
        Some(Ok(_)) => format!("{agent} · type {id} in any terminal"),
        Some(Err(why)) => format!("{agent} · no command: {why}"),
        None => agent,
    };
    let description = super::agents::with_shell(
        description,
        entry.shell.as_deref(),
        ws.agent_shell(entry).is_some(),
    );
    let show = {
        let (e, entry) = (entity.clone(), entry.clone());
        Switch::new(ElementId::Name(format!("show-account-{id}").into()))
            .checked(entry.show)
            .on_click(move |on: &bool, window, cx| {
                let draft = AccountDraft {
                    show: *on,
                    ..AccountDraft::of(&entry)
                };
                let id = entry.id.clone();
                e.update(cx, |ws, cx| ws.save_account(&id, &draft, window, cx).ok());
            })
    };
    let pill = |name: String, label: &'static str| {
        div()
            .id(ElementId::Name(name.into()))
            .px(px(10.0))
            .py(px(3.0))
            .rounded_full()
            .border_1()
            .border_color(ui.hairline)
            .text_size(px(12.0))
            .cursor_pointer()
            .hover(move |s| s.bg(ui.hover))
            .active(move |s| s.bg(ui.pressed))
            .child(label)
    };
    let log_in = {
        let (e, id) = (entity.clone(), id.clone());
        pill(format!("log-in-{id}"), "Log in")
            .on_click(move |_: &ClickEvent, window, cx| {
                let id = id.clone();
                e.update(cx, |ws, cx| {
                    ws.close_settings(window, cx);
                    ws.log_in(&id, window, cx);
                })
            })
            .test_support()
    };
    let edit = {
        let (e, entry) = (entity.clone(), entry.clone());
        pill(format!("edit-account-{id}"), "Edit")
            .on_click(move |_: &ClickEvent, window, cx| {
                let entry = entry.clone();
                e.update(cx, |ws, cx| account_dialog(ws, Some(entry), window, cx))
            })
            .test_support()
    };
    Row::new(format!("accounts.{id}"), entry.title.clone())
        .description(description)
        .control(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(show)
                .child(log_in)
                .child(edit),
        )
}

/// A line for each variable that wins over every account's login that Templum's environment
/// has (`set`): the person's shell may set one too.
fn override_notes(kind: &Kind, agent: &str, set: impl Fn(&str) -> bool) -> Vec<String> {
    kind.overrides
        .iter()
        .filter(|name| set(name))
        .map(|name| {
            format!(
                "{name} is set in your environment: {agent} uses it whatever this account's login."
            )
        })
        .collect()
}

/// The agents an account can be of, installed first: their ids and titles.
pub(crate) fn agent_choices(agents: &[AgentEntry]) -> Vec<(String, String)> {
    let mut choices: Vec<(bool, String, String)> = templum_usage::accounts::kinds()
        .iter()
        .filter_map(|kind| {
            let agent = agents.iter().find(|a| a.id == kind.agent)?;
            let installed = agents::locate(agent.program()).is_some();
            Some((!installed, agent.id.clone(), agent.title.clone()))
        })
        .collect();
    choices.sort_by_key(|(missing, ..)| *missing);
    choices
        .into_iter()
        .map(|(_, id, title)| (id, title))
        .collect()
}

/// How an account's items are shared (docs/specs/accounts.md, The dialog).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Setup {
    /// Every item it can share is its agent's own.
    Same,
    /// Nothing shared.
    Fresh,
    /// Item by item.
    Custom,
}

/// What Same shares: each item of `kind` with its agent's own folder, but the files where the OS
/// won't link them (Windows without Developer Mode).
fn same_share(kind: &Kind) -> BTreeMap<String, String> {
    let files = templum_usage::accounts::files_share();
    kind.items
        .iter()
        .filter(|item| files || item.paths[0].ends_with('/'))
        .map(|item| (item.key.to_string(), kind.agent.to_string()))
        .collect()
}

/// The setup `share` is: nothing is Fresh, what Same makes is Same, anything else Custom.
fn setup_of(kind: &Kind, share: &BTreeMap<String, String>) -> Setup {
    if share.is_empty() {
        Setup::Fresh
    } else if *share == same_share(kind) {
        Setup::Same
    } else {
        Setup::Custom
    }
}

/// The agent a new account starts as: Claude Code when it is installed, else the first agent that
/// is, else Claude Code.
fn default_agent(agents: &[AgentEntry]) -> Option<&'static Kind> {
    let installed = |id: &str| {
        agents
            .iter()
            .find(|a| a.id == id)
            .is_some_and(|a| agents::locate(a.program()).is_some())
    };
    let first = agent_choices(agents).into_iter().next()?.0;
    let id = if installed("claude") || !installed(&first) {
        "claude"
    } else {
        first.as_str()
    };
    templum_usage::accounts::kind(id)
}

/// The dialog's state: what it was opened with, and the choices made in it.
pub(crate) struct Form {
    /// The account edited; None: a new one.
    entry: Option<AgentEntry>,
    agents: Vec<AgentEntry>,
    data: Option<PathBuf>,
    /// The ids `[agents]` holds, with what Settings hasn't applied yet, and those in effect.
    taken: Vec<String>,
    ui: Ui,
    /// The theme's red, for a rule the name breaks.
    red: Hsla,
    kind: Option<&'static Kind>,
    /// The agents a new account can be of (`agent_choices`), read when the dialog opens.
    choices: Vec<(String, String)>,
    /// A new account's name, and the first rule it breaks, read at each key (not each frame: it
    /// looks for a program of that name).
    name: Entity<InputState>,
    problem: Option<String>,
    setup: Setup,
    /// The items shared, by key: the agent's id or another account of it.
    share: BTreeMap<String, String>,
    /// Items holding a link the person made, and where it points.
    theirs: BTreeMap<&'static str, PathBuf>,
    /// An edited account's command: its file, or why it has none.
    command: Option<Result<PathBuf, String>>,
    /// The label of the shell it opens in; None: the default. The shells found and the default's
    /// name, for the list.
    shell: Option<String>,
    shells: Vec<ShellOption>,
    default_shell: String,
}

impl Form {
    fn new(
        ws: &Workspace,
        entry: Option<&AgentEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("claudinho"));
        let account = entry.and_then(|e| e.account.as_ref());
        let kind = match account {
            Some(account) => Some(account.kind),
            None => default_agent(&ws.agents),
        };
        let mut form = Form {
            entry: entry.cloned(),
            agents: ws.agents.clone(),
            data: ws.data_dir.clone(),
            taken: ws.taken_ids(),
            ui: ws.ui,
            red: crate::grid::hsla(ws.theme.palette.ansi[1], 1.0),
            kind,
            choices: if entry.is_none() {
                agent_choices(&ws.agents)
            } else {
                Vec::new()
            },
            name,
            problem: None,
            setup: Setup::Same,
            share: BTreeMap::new(),
            theirs: BTreeMap::new(),
            command: entry.and_then(|e| ws.commands.get(&e.id).cloned()),
            shell: entry.and_then(|e| e.shell.clone()),
            shells: ws.shells.clone(),
            default_shell: ws.default_shell_name(),
        };
        match account {
            Some(account) => {
                form.share = account.share.clone();
                form.read_links();
                form.setup = kind.map_or(Setup::Fresh, |k| setup_of(k, &form.share));
            }
            None => form.reset_setup(Setup::Same),
        }
        form
    }

    /// Reads `problem` again for the name typed: the first rule it breaks (docs/specs/accounts.md,
    /// Its name); None while it is empty or free.
    fn check_name(&mut self, cx: &App) {
        let name = self.name.read(cx).value();
        let commands = agents::commands_dir();
        self.problem = (!name.is_empty())
            .then(|| {
                agents::name_problem(
                    &name,
                    |x| self.taken.iter().any(|t| t.eq_ignore_ascii_case(x)),
                    self.data.as_deref(),
                    commands.as_deref(),
                    agents::locate,
                )
            })
            .flatten();
    }

    /// Create works: a new account's name is typed and free.
    fn can_create(&self, cx: &App) -> bool {
        self.kind.is_some() && !self.name.read(cx).value().is_empty() && self.problem.is_none()
    }

    /// Who an item may be shared with: its agent's own folder, then its other accounts, by id,
    /// with their state folders.
    fn sources(&self) -> Vec<(String, Option<PathBuf>)> {
        let Some(kind) = self.kind else {
            return Vec::new();
        };
        let me = self.entry.as_ref().map(|e| &e.id);
        let accounts = self.agents.iter().filter_map(|a| {
            let account = a.account.as_ref()?;
            (account.kind.agent == kind.agent && Some(&a.id) != me)
                .then(|| (a.id.clone(), Some(account.kind.state_dir(&account.folder))))
        });
        std::iter::once((kind.agent.to_string(), own_state(kind, &self.agents)))
            .chain(accounts)
            .collect()
    }

    /// The choices an edited account's folder already holds: a link to the same item of one of
    /// its agent's folders becomes that choice, one elsewhere is the person's. Only links add
    /// choices: a folder without one changes none.
    fn read_links(&mut self) {
        let (Some(kind), Some(account)) = (
            self.kind,
            self.entry.as_ref().and_then(|e| e.account.as_ref()),
        ) else {
            return;
        };
        self.theirs.clear();
        if !account.folder.is_dir() {
            return;
        }
        let sources = self.sources();
        let paths: Vec<PathBuf> = sources.iter().filter_map(|(_, p)| p.clone()).collect();
        for (key, found) in templum_usage::accounts::read(kind, &account.folder, &paths) {
            match found {
                Found::Theirs(target) => {
                    self.theirs.insert(key, target);
                }
                Found::Linked(state) => {
                    if let Some((id, _)) = sources.iter().find(|(_, p)| p.as_ref() == Some(&state))
                    {
                        self.share.insert(key.to_string(), id.clone());
                    }
                }
                Found::Own => {}
            }
        }
    }

    /// The setup chosen: Same and Fresh set the choices, Custom starts from the last ones.
    fn reset_setup(&mut self, setup: Setup) {
        match (setup, self.kind) {
            (Setup::Same, Some(kind)) => self.share = same_share(kind),
            (Setup::Fresh, _) => self.share.clear(),
            _ => {}
        }
        self.setup = setup;
    }

    /// The agent of a new account.
    fn choose(&mut self, agent: &str, cx: &mut Context<Self>) {
        let Some(kind) = templum_usage::accounts::kind(agent) else {
            return;
        };
        if self.kind == Some(kind) {
            return;
        }
        self.kind = Some(kind);
        self.reset_setup(Setup::Same);
        cx.notify();
    }

    /// What Create or Save writes, and its id: None while a new account's name is missing or breaks
    /// a rule.
    fn to_save(&self, cx: &App) -> Option<(String, AccountDraft)> {
        let kind = self.kind?;
        match &self.entry {
            Some(entry) => Some((
                entry.id.clone(),
                AccountDraft {
                    share: self.share.clone(),
                    shell: self.shell.clone(),
                    ..AccountDraft::of(entry)
                },
            )),
            None => self.can_create(cx).then(|| {
                let draft = AccountDraft {
                    of: kind.agent.to_string(),
                    show: true,
                    share: self.share.clone(),
                    shell: self.shell.clone(),
                    ..AccountDraft::default()
                };
                (self.name.read(cx).value().to_string(), draft)
            }),
        }
    }
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = self.ui;
        let label = |text: &str| {
            div()
                .text_size(px(12.0))
                .text_color(ui.title)
                .child(text.to_string())
        };
        let note = |text: String| {
            div()
                .text_size(px(11.0))
                .text_color(ui.title_dim)
                .child(text)
        };
        let form = cx.entity();
        let mut column = div().flex().flex_col().gap(px(18.0));
        let Some(kind) = self.kind else {
            return column;
        };
        let agent = agent_title(&self.agents, kind);
        match &self.entry {
            None => {
                let f = form.clone();
                let options = self
                    .choices
                    .iter()
                    .map(|(id, title)| (id.clone().into(), title.clone().into()))
                    .collect();
                column = column
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(label("Agent"))
                            .child(menu_button(
                                "account-agent",
                                options,
                                SharedString::from(kind.agent),
                                false,
                                move |id, cx| f.update(cx, |f, cx| f.choose(&id, cx)),
                            )),
                    )
                    .child(self.name_field(&agent, cx))
                    .child(self.shell_field(&form));
            }
            Some(entry) => {
                let command = match &self.command {
                    Some(Ok(path)) => {
                        format!("Type {} in any terminal ({})", entry.id, tilde(path))
                    }
                    Some(Err(why)) => format!("No command: {why}"),
                    None => "No command".to_string(),
                };
                let folder = entry
                    .account
                    .as_ref()
                    .map(|a| tilde(&a.folder))
                    .unwrap_or_default();
                column = column
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(label("Command"))
                            .child(
                                div()
                                    .id("account-command")
                                    .text_size(px(13.0))
                                    .child(command)
                                    .test_support(),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(label("Folder"))
                            .child(div().text_size(px(13.0)).child(folder)),
                    )
                    .child(self.shell_field(&form));
            }
        }
        if !kind.items.is_empty() {
            column = column.child(self.setup_section(kind, &agent, cx));
        }
        // The notes, each when it applies.
        let mut notes = div().flex().flex_col().gap(px(6.0));
        let command_id = match &self.entry {
            Some(entry) => matches!(self.command, Some(Ok(_))).then(|| entry.id.clone()),
            None => Some(
                Some(self.name.read(cx).value().to_string())
                    .filter(|n| !n.is_empty() && self.problem.is_none())
                    .unwrap_or_else(|| "claudinho".to_string()),
            ),
        };
        if let Some(path) = command_id.and_then(|id| path_note(ui, &id)) {
            notes = notes.child(path);
        }
        let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        for text in override_notes(kind, &agent, set) {
            notes = notes.child(note(text));
        }
        column.child(notes)
    }
}

impl Form {
    /// The Shell list: Default, then the shells found without WSL, an account never running there
    /// (docs/specs/shells.md, What works in WSL).
    fn shell_field(&self, form: &Entity<Form>) -> Div {
        let f = form.clone();
        let options = shell_choices(
            &self.default_shell,
            &self.shells,
            true,
            self.shell.as_deref(),
        );
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(self.ui.title)
                    .child("Shell"),
            )
            .child(menu_button(
                "menu-account-shell",
                options,
                self.shell.clone().unwrap_or_default().into(),
                true,
                move |value, cx| {
                    f.update(cx, |f, cx| {
                        f.shell = Some(value.to_string()).filter(|v| !v.is_empty());
                        cx.notify();
                    })
                },
            ))
    }

    /// The name's label, field and the line under it: nothing while empty, the first rule it breaks
    /// in the theme's red, else what to type.
    fn name_field(&self, agent: &str, cx: &mut Context<Self>) -> Div {
        let ui = self.ui;
        let typed = self.name.read(cx).value().to_string();
        let red = self.red;
        let under = if typed.is_empty() {
            None
        } else if let Some(problem) = &self.problem {
            Some(div().text_color(red).child(problem.clone()))
        } else {
            Some(
                div()
                    .flex()
                    .flex_wrap()
                    .text_color(ui.title_dim)
                    .child("Type ")
                    .child(div().text_color(ui.accent).child(typed))
                    .child(format!(" in any terminal to open {agent} on this account.")),
            )
        };
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(div().text_size(px(12.0)).text_color(ui.title).child("Name"))
            .child(
                div()
                    .id("account-name")
                    // Enter creates it (below): it mustn't also confirm (close) the dialog.
                    .on_action(|_: &Confirm, _, cx| cx.stop_propagation())
                    .child(Styled::h(
                        Input::new(&self.name).text_size(px(15.0)).px(px(12.0)),
                        px(38.0),
                    ))
                    .test_support(),
            )
            .children(under.map(|under| {
                div()
                    .id("account-name-note")
                    .text_size(px(12.0))
                    .child(under)
                    .test_support()
            }))
    }

    /// The Setup label, its three segments, the line under them and, for Custom, the items.
    fn setup_section(&self, kind: &'static Kind, agent: &str, cx: &mut Context<Self>) -> Div {
        let ui = self.ui;
        let form = cx.entity();
        let files_share = templum_usage::accounts::files_share();
        let segments = [
            (
                Setup::Same,
                "account-setup-same",
                format!("Same as {agent}"),
            ),
            (Setup::Fresh, "account-setup-fresh", "Fresh".to_string()),
            (Setup::Custom, "account-setup-custom", "Custom".to_string()),
        ]
        .into_iter()
        .map(|(setup, id, text)| {
            let f = form.clone();
            segment(ui, id, self.setup == setup)
                .child(text)
                .on_click(move |_: &ClickEvent, _, cx| {
                    f.update(cx, |f, cx| {
                        f.reset_setup(setup);
                        cx.notify();
                    })
                })
                .test_support()
        });
        let explains = match self.setup {
            Setup::Same => {
                let own = tilde(&own_state(kind, &self.agents).unwrap_or_default());
                let mut text = format!(
                    "Everything but the login is your {agent}'s ({own}): settings, skills, plugins, sessions…"
                );
                if !files_share && kind.items.iter().any(|i| !i.paths[0].ends_with('/')) {
                    text.push_str(
                        " Files stay its own: Windows shares them only with Developer Mode.",
                    );
                }
                text
            }
            Setup::Fresh => format!("It starts empty: nothing shared with your {agent}."),
            Setup::Custom => "Choose item by item.".to_string(),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(ui.title)
                    .child("Setup"),
            )
            .child(strip(ui).children(segments))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(ui.title_dim)
                    .child(explains),
            )
            .when(self.setup == Setup::Custom, |el| {
                el.child(self.items_panel(kind, agent, &form))
            })
    }

    /// Custom's faint panel: Share everything with, and a row per item, Own or shared.
    fn items_panel(&self, kind: &'static Kind, agent: &str, form: &Entity<Form>) -> Div {
        let ui = self.ui;
        let note = |text: String| {
            div()
                .text_size(px(11.0))
                .text_color(ui.title_dim)
                .child(text)
        };
        let sources = self.sources();
        let source_label = |id: &str| {
            if id == kind.agent {
                format!("{agent}: its own folder")
            } else {
                self.agents
                    .iter()
                    .find(|a| a.id == id)
                    .map_or_else(|| id.to_string(), |a| a.title.clone())
            }
        };
        let everyone: Vec<(SharedString, SharedString)> = sources
            .iter()
            .map(|(id, _)| (id.clone().into(), source_label(id).into()))
            .collect();
        let all = {
            let f = form.clone();
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(div().text_size(px(13.0)).child("Share everything with"))
                .child(menu_button(
                    "account-share-all",
                    everyone.clone(),
                    "Choose…".into(),
                    false,
                    move |to, cx| {
                        f.update(cx, |f, cx| {
                            let Some(kind) = f.kind else { return };
                            for item in kind.items {
                                f.share.insert(item.key.to_string(), to.to_string());
                            }
                            cx.notify();
                        })
                    },
                ))
        };
        let files_share = templum_usage::accounts::files_share();
        let mut items = div()
            .id("account-items")
            .flex()
            .flex_col()
            .gap(px(6.0))
            .max_h(px(260.0))
            .overflow_y_scroll();
        for item in kind.items {
            let paths = item.paths.join(", ");
            let file = !item.paths[0].ends_with('/');
            let current = self.share.get(item.key).cloned().unwrap_or_default();
            // Why it can't be chosen goes under its paths, with the room of the row (beside them it
            // took all of it: the item's words broke at each letter); else its list, at the right.
            let (why, choice) = if let Some(target) = self.theirs.get(item.key) {
                (Some(format!("Linked by you to {}", tilde(target))), None)
            } else if file && !files_share {
                (
                    Some(templum_usage::accounts::NO_FILE_LINKS.to_string()),
                    None,
                )
            } else {
                let f = form.clone();
                let key = item.key;
                let options = std::iter::once(("".into(), "Own".into()))
                    .chain(everyone.iter().cloned())
                    .collect();
                let list = menu_button(
                    ElementId::Name(format!("account-share-{key}").into()),
                    options,
                    current.clone().into(),
                    false,
                    move |to, cx| {
                        f.update(cx, |f, cx| {
                            if to.is_empty() {
                                f.share.remove(key);
                            } else {
                                f.share.insert(key.to_string(), to.to_string());
                            }
                            cx.notify();
                        })
                    },
                );
                (None, Some(list))
            };
            // A file the source hasn't written yet is linked once it has.
            let waiting = file
                && !current.is_empty()
                && sources
                    .iter()
                    .find(|(id, _)| *id == current)
                    .and_then(|(_, dir)| dir.as_ref())
                    .is_some_and(|dir| !dir.join(item.paths[0]).exists());
            items = items.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .child(
                        div()
                            .id(ElementId::Name(format!("account-item-{}", item.key).into()))
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(div().text_size(px(13.0)).child(item.label))
                            .child(note(if waiting {
                                format!("{paths} · Shared once {agent} writes one")
                            } else {
                                paths
                            }))
                            .children(why.map(note))
                            .test_support(),
                    )
                    .children(choice.map(|list| div().flex_shrink_0().child(list))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(12.0))
            .rounded(px(8.0))
            .bg(ui.card)
            .child(all)
            .child(items)
    }
}

/// Asks before deleting the account `id` (docs/specs/accounts.md, Deleting an account), then
/// deletes it and closes its dialog, or says why it can't.
fn ask_delete(ws: &gpui_kit::WeakEntity<Workspace>, id: &str, window: &mut Window, cx: &mut App) {
    let Some(entry) = ws
        .upgrade()
        .and_then(|ws| ws.read(cx).agents.iter().find(|a| a.id == id).cloned())
    else {
        return;
    };
    let Some(account) = &entry.account else {
        return;
    };
    let mut lines = vec![if account.chosen {
        format!("Its folder {} stays.", tilde(&account.folder))
    } else {
        "Its folder is deleted: its login and everything it doesn't share. What it shares stays \
         where it is."
            .to_string()
    }];
    let keychain = match account.kind.keychain {
        templum_usage::accounts::Keychain::Always => true,
        templum_usage::accounts::Keychain::MacOs => cfg!(target_os = "macos"),
        templum_usage::accounts::Keychain::Never => false,
    };
    if keychain {
        lines.push(
            "Its login stays in the system's keychain: run /logout in it first to remove it."
                .to_string(),
        );
    }
    if ws
        .upgrade()
        .is_some_and(|ws| ws.read(cx).command_removed(id))
    {
        lines.push(format!("Its command {id} is removed."));
    }
    let (ws, id) = (ws.clone(), id.to_string());
    crate::prompt::confirm(
        format!("Delete {}?", entry.title),
        lines.join(" "),
        "Delete",
        move |window, cx| {
            let deleted = ws.update(cx, |ws, cx| ws.delete_account(&id, cx));
            match deleted {
                Ok(Ok(())) => window.close_all_dialogs(cx),
                Ok(Err(why)) => show_notification(window, cx, Notification::warning(why)),
                Err(_) => {}
            }
        },
        window,
        cx,
    );
}

/// Create or Save: saves the account, then closes the dialog; a new one also closes Settings and
/// opens its Log in. A problem `save_account` finds is a notification, the dialog left open.
fn save(form: &Entity<Form>, ws: &Entity<Workspace>, window: &mut Window, cx: &mut App) {
    let Some((id, draft)) = form.read(cx).to_save(cx) else {
        return;
    };
    let new = form.read(cx).entry.is_none();
    match ws.update(cx, |ws, cx| ws.save_account(&id, &draft, window, cx)) {
        Err(problem) => show_notification(window, cx, Notification::warning(problem)),
        Ok(id) => {
            window.close_dialog(cx);
            if new {
                ws.update(cx, |ws, cx| {
                    // In effect now: Log in opens what is in effect.
                    ws.apply_and_save(window, cx);
                    ws.close_settings(window, cx);
                    ws.log_in(&id, window, cx);
                });
            }
        }
    }
}

/// Editing an account (`entry`), or adding one (None), in a dialog in the card dialog's language;
/// Create or Save writes its entry and applies its choices to its folder.
pub(crate) fn account_dialog(
    ws: &Workspace,
    entry: Option<AgentEntry>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let form = cx.new(|cx| Form::new(ws, entry.as_ref(), window, cx));
    let entity = cx.entity();
    let red = crate::grid::hsla(ws.theme.palette.ansi[1], 1.0);
    // The name's keys redraw the line under it and Create's label; Enter creates.
    let name = form.read(cx).name.clone();
    {
        let (form, entity) = (form.clone(), entity.clone());
        App::subscribe(cx, &name, move |_, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                form.update(cx, |form, cx| {
                    form.check_name(cx);
                    cx.notify();
                });
                entity.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
    }
    {
        let (form, entity) = (form.clone(), entity.clone());
        window
            .subscribe(&name, cx, move |_, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    save(&form, &entity, window, cx);
                }
            })
            .detach();
    }
    let focus = name.clone();
    let new = entry.is_none();
    let edited = entry.map(|e| (e.id.clone(), e.account.map(|a| a.kind)));
    let agent_name = form
        .read(cx)
        .kind
        .map(|kind| agent_title(&form.read(cx).agents, kind));
    window.open_alert_dialog(cx, move |alert, _, cx| {
        let f = form.read(cx);
        let ui = f.ui;
        let header = div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(SIDE))
            .pt(px(16.0))
            .pb(px(14.0))
            .border_b_1()
            .border_color(ui.hairline)
            .map(|el| match &edited {
                None => el
                    .flex_col()
                    .items_start()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Add an account"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(ui.title_dim)
                            .child("Another login of an agent, opened by its own command."),
                    ),
                Some((id, _)) => el
                    .child(
                        div()
                            .id("account-id")
                            .px(px(8.0))
                            .py(px(3.0))
                            .rounded(px(6.0))
                            .bg(ui.accent.opacity(0.16))
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ui.accent)
                            .child(id.clone())
                            .test_support(),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(ui.title)
                            .child(format!(
                                "Account of {}",
                                agent_name.clone().unwrap_or_default()
                            )),
                    ),
            });
        let save_button = {
            let (form, entity) = (form.clone(), entity.clone());
            let can = !new || f.can_create(cx);
            let text = if new {
                match f.name.read(cx).value() {
                    name if can => format!("Create {name}"),
                    _ => "Create".to_string(),
                }
            } else {
                "Save".to_string()
            };
            let (bg, color) = if can {
                (ui.accent, ui.surface)
            } else {
                (ui.hover, ui.title_dim)
            };
            pill("account-save", text, None, bg, color)
                .when(!can, |el| el.cursor_default())
                .on_click(move |_: &ClickEvent, window, cx| {
                    save(&form, &entity, window, cx);
                })
                .test_support()
        };
        let delete = edited.as_ref().map(|(id, _)| {
            let (ws, id) = (entity.downgrade(), id.clone());
            danger("delete-account", "Delete account", red)
                .on_click(move |_: &ClickEvent, window, cx| ask_delete(&ws, &id, window, cx))
                .test_support()
        });
        let footer = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px(px(SIDE))
            .py(px(12.0))
            .border_t_1()
            .border_color(ui.hairline)
            .child(div().children(delete))
            .child(save_button);
        alert
            .title(header)
            .width(px(DIALOG_W))
            .close_button(true)
            // A block's own color, not the canvas's: the fields and the faint fills read on it.
            .bg(ui.surface)
            .pt(px(0.0))
            .pb(px(0.0))
            .pl(px(0.0))
            .pr(px(0.0))
            .footer(footer)
            .child(
                div()
                    .px(px(SIDE))
                    .pt(px(18.0))
                    .pb(px(18.0))
                    .child(form.clone()),
            )
    });
    // After opening: the name takes the focus.
    if new {
        focus.update(cx, |input, cx| input.focus(window, cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variable_that_wins_over_the_login_is_named() {
        let copilot = templum_usage::accounts::kind("copilot").unwrap();
        let notes = override_notes(copilot, "Copilot CLI", |name| name == "GH_TOKEN");
        assert_eq!(
            notes,
            [
                "GH_TOKEN is set in your environment: Copilot CLI uses it whatever this account's login."
            ]
        );
        let claude = templum_usage::accounts::kind("claude").unwrap();
        assert!(override_notes(claude, "Claude Code", |_| false).is_empty());
        assert_eq!(
            override_notes(claude, "Claude Code", |_| true).len(),
            claude.overrides.len()
        );
    }

    #[test]
    fn a_setup_is_read_from_the_shares() {
        let claude = templum_usage::accounts::kind("claude").unwrap();
        assert_eq!(setup_of(claude, &BTreeMap::new()), Setup::Fresh);
        assert_eq!(setup_of(claude, &same_share(claude)), Setup::Same);
        let mut one = BTreeMap::new();
        one.insert("skills".to_string(), "claude".to_string());
        assert_eq!(setup_of(claude, &one), Setup::Custom);
        one.insert("skills".to_string(), "claude3".to_string());
        assert_eq!(setup_of(claude, &one), Setup::Custom);
        // An agent with no items has nothing to share.
        let crush = templum_usage::accounts::kind("crush").unwrap();
        assert!(same_share(crush).is_empty());
    }
}
