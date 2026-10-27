//! The Agents view's model (docs/specs/agents-view.md): each agent's state as its block's dot
//! tells it, in words.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Instant;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    App, AppContext as _, Context, Focusable as _, KeyDownEvent, ScrollStrategy, Window,
};
use templum_canvas::BlockId;

use super::inbox::cards_of;
use super::{AgentState, Pane, Workspace};
use crate::alert::Alert;
use crate::automations::Heard;
use crate::keys::Command;

/// What an agent does, as its row says it. Never inferred anew: `Workspace::doing` follows the
/// dot of its block.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Doing {
    /// What it asks: a request's summary, an alert's text, its dialog's words; may be empty.
    NeedsYou(String),
    /// Its turn ended while these subagents work or owe a report (`Pane::subagents.len()`).
    AwaitsSubagents(usize),
    /// At work, with how many of its subagents work too.
    Working(usize),
    Done,
    Idle,
    Starting,
}

impl Doing {
    /// The row's words.
    pub(crate) fn label(&self) -> String {
        match self {
            Doing::NeedsYou(_) => "Needs you".into(),
            Doing::AwaitsSubagents(1) => "Waiting for 1 subagent".into(),
            Doing::AwaitsSubagents(n) => format!("Waiting for {n} subagents"),
            Doing::Working(0) => "Working".into(),
            Doing::Working(1) => "Working · 1 subagent".into(),
            Doing::Working(n) => format!("Working · {n} subagents"),
            Doing::Done => "Done".into(),
            Doing::Idle => "Idle".into(),
            Doing::Starting => "Starting".into(),
        }
    }

    /// Where it sorts within its workspace: the lower first.
    pub(crate) fn rank(&self) -> u8 {
        match self {
            Doing::NeedsYou(_) => 0,
            Doing::Done => 1,
            Doing::AwaitsSubagents(_) => 2,
            Doing::Working(_) => 3,
            Doing::Starting | Doing::Idle => 4,
        }
    }

    /// Its agent is at work: a turn's end from here is a finished turn.
    pub(crate) fn works(&self) -> bool {
        matches!(self, Doing::Working(_) | Doing::AwaitsSubagents(_))
    }
}

/// The filter chips of the list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Show {
    #[default]
    All,
    NeedsYou,
    Working,
    Done,
    Idle,
}

impl Show {
    pub(crate) const ALL: [Show; 5] = [
        Show::All,
        Show::NeedsYou,
        Show::Working,
        Show::Done,
        Show::Idle,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Show::All => "All",
            Show::NeedsYou => "Needs you",
            Show::Working => "Working",
            Show::Done => "Done",
            Show::Idle => "Idle",
        }
    }

    /// The chip lists an agent doing `doing`: Working has the ones that wait for subagents too,
    /// Idle the ones that start.
    pub(crate) fn shows(self, doing: &Doing) -> bool {
        match self {
            Show::All => true,
            Show::NeedsYou => matches!(doing, Doing::NeedsYou(_)),
            Show::Working => doing.works(),
            Show::Done => matches!(doing, Doing::Done),
            Show::Idle => matches!(doing, Doing::Idle | Doing::Starting),
        }
    }
}

impl Workspace {
    /// What the agent in `pane` does, in the dot's precedence (`block::status_dot`). Never `Done`:
    /// that is the watch's.
    pub(crate) fn doing(&self, pane: &Pane) -> Doing {
        // A remote workspace's agent is what home says, not what this computer's hooks hear.
        if let Some(remote) = &pane.remote {
            return remote
                .agent
                .as_ref()
                .map_or(Doing::Idle, |agent| match agent.state.as_str() {
                    "needs_you" => Doing::NeedsYou(agent.asks.clone().unwrap_or_default()),
                    "starting" => Doing::Starting,
                    "waiting_subagents" => Doing::AwaitsSubagents(agent.subagents),
                    "working" => Doing::Working(agent.subagents),
                    // Done is this computer's own mark (`Watch`): the person looking here clears
                    // it, and home's own is no business of this watch.
                    _ => Doing::Idle,
                });
        }
        // A request waits: whatever news the block has, the person is asked.
        if let Some((wait, _)) = self.asking(pane.term.id()) {
            return Doing::NeedsYou(wait.summary());
        }
        if let Some(alert @ Alert::Attention(_)) = &pane.alert {
            return Doing::NeedsYou(alert.text());
        }
        // An agent the last snapshot no longer sees says nothing of its turn.
        match pane.state.as_ref().filter(|_| pane.seen.agent.is_some()) {
            Some(AgentState::NeedsYou(asks)) => Doing::NeedsYou(asks.clone().unwrap_or_default()),
            _ if pane.starting.is_some() => Doing::Starting,
            Some(AgentState::Working) if pane.awaits_subagents && !pane.subagents.is_empty() => {
                Doing::AwaitsSubagents(pane.subagents.len())
            }
            Some(AgentState::Working) => Doing::Working(pane.subagents_at_work()),
            Some(AgentState::Idle) | None => Doing::Idle,
        }
    }
}

/// What the Agents view keeps of a terminal's agent between frames; in memory only.
#[derive(Default)]
pub(crate) struct Watch {
    /// Its state as last seen (never Done), and since when its kind holds.
    pub was: Option<(Doing, Instant)>,
    /// Its turn ended since the person last looked at its block.
    pub done: bool,
}

impl Watch {
    /// The agent does `doing` at `now`: its age restarts when the kind changes, and a turn that
    /// ends marks it done until it works again or its person looks. What an automation hears:
    /// `Idle` when the turn ended, `NeedsYou` when a wait began (not on the first sight).
    pub(super) fn see(&mut self, doing: Doing, now: Instant) -> Option<Heard> {
        let mut heard = None;
        let since = match &self.was {
            Some((was, since)) if std::mem::discriminant(was) == std::mem::discriminant(&doing) => {
                *since
            }
            Some((was, _)) => {
                if was.works() && doing == Doing::Idle {
                    self.done = true;
                    heard = Some(Heard::Idle);
                }
                if doing.works() {
                    self.done = false;
                }
                if matches!(doing, Doing::NeedsYou(_)) {
                    heard = Some(Heard::NeedsYou);
                }
                now
            }
            None => now,
        };
        self.was = Some((doing, since));
        heard
    }

    /// Its turn ended unseen and it has not worked since.
    pub(crate) fn is_done(&self) -> bool {
        self.done && matches!(self.was, Some((Doing::Idle, _)))
    }

    /// The state its row shows: `Done` when `done` and idle; None before the first watch.
    pub(crate) fn shown(&self) -> Option<(Doing, Instant)> {
        let (doing, since) = self.was.clone()?;
        Some(if self.is_done() {
            (Doing::Done, since)
        } else {
            (doing, since)
        })
    }
}

impl Workspace {
    /// Notes what every agent does at `now`, wherever Templum hears a change: a frame, a message
    /// of the bus, a look at the screens. `active`: the window is in front, so the focused block
    /// of the workspace shown is looked at (alert.rs's rule).
    pub(super) fn watch_agents(&mut self, active: bool, now: Instant) {
        let this = &*self;
        let doings: Vec<(usize, BlockId, Option<Doing>)> = this
            .spaces()
            .enumerate()
            .filter(|(_, space)| !space.stopped)
            .flat_map(|(i, space)| {
                space.watched_terminals().map(move |(id, pane)| {
                    let agent = pane.agent().is_some() && pane.follows.is_none();
                    (i, id, agent.then(|| this.doing(pane)))
                })
            })
            .collect();
        let focused = self.space.focused;
        // Empty, so free, on the frames where nothing changed.
        let mut heard: Vec<(String, Heard)> = Vec::new();
        for (i, id, doing) in doings {
            let space = match i {
                0 => &mut self.space,
                i => &mut self.others[i - 1],
            };
            let Some(pane) = space.terminal_mut(id) else {
                continue;
            };
            let said = match doing {
                None => {
                    pane.watch = Watch::default();
                    None
                }
                Some(doing) => pane.watch.see(doing, now),
            };
            if active && i == 0 && focused == Some(id) {
                pane.watch.done = false;
            }
            // A remote agent is never heard by this computer's event rules.
            if let Some(said) = said
                && !space.is_remote()
                && let Some(uid) = space.uids.get(&id)
            {
                heard.push((uid.clone(), said));
            }
        }
        // Noted only while an event rule is on to hear it.
        if !heard.is_empty() && self.automations_listen() {
            self.automation_heard.extend(heard);
        }
    }

    /// The workspace and block of the terminal `terminal`, a remote workspace's viewer too: the
    /// Agents view's rows name them (`caller` is for what acts on a local terminal).
    fn watched_caller(&self, terminal: &str) -> Option<(String, BlockId)> {
        self.spaces().find_map(|space| {
            space
                .watched_terminals()
                .find(|(_, pane)| pane.term.id() == terminal)
                .map(|(id, _)| (space.id.clone(), id))
        })
    }

    /// How many agents of the workspaces that run finished since their person looked.
    pub(crate) fn done_count(&self) -> usize {
        self.spaces()
            .filter(|space| !space.stopped)
            .flat_map(|space| space.watched_terminals())
            .filter(|(_, pane)| pane.watch.is_done())
            .count()
    }

    /// Mark all seen: no agent is Done any more.
    pub(crate) fn mark_all_seen(&mut self, cx: &mut Context<Self>) {
        for space in std::iter::once(&mut self.space).chain(&mut self.others) {
            for body in space.blocks.values_mut() {
                if let Some(pane) = body.terminal_mut() {
                    pane.watch.done = false;
                }
            }
        }
        cx.notify();
    }
}

/// One agent of the list: everything a row and the details say.
#[derive(Clone, Debug)]
pub(crate) struct AgentRow {
    /// Its workspace's id and name.
    pub space: String,
    pub space_name: String,
    /// Its terminal (`TEMPLUM_TERMINAL`): what names the row.
    pub terminal: String,
    pub title: String,
    /// Its agent's or account's name; empty when unknown.
    pub agent: String,
    /// `Watch::shown`, and since when.
    pub doing: Doing,
    pub since: Instant,
    /// The card it works on (`Workspace::cards_by_block`).
    pub card: Option<(u64, String)>,
    /// The task it was handed, whole.
    pub task: Option<String>,
    pub folder: Option<String>,
    /// The context used, as the usage chip says it.
    pub context: Option<String>,
    /// Its workspace is a remote one that isn't live: the row is drawn dim.
    pub dim: bool,
}

/// The order within a workspace's group: the state's rank, then for Needs you the longest waiting
/// first, for Done the newest first; then the title (lowercased), then the terminal.
pub(crate) fn by_state(a: &AgentRow, b: &AgentRow) -> Ordering {
    let rank = a.doing.rank();
    let waited = match rank {
        0 => a.since.cmp(&b.since),
        1 => b.since.cmp(&a.since),
        _ => Ordering::Equal,
    };
    rank.cmp(&b.doing.rank())
        .then(waited)
        .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        .then_with(|| a.terminal.cmp(&b.terminal))
}

/// The row matches `query` (lowercased, not empty): in its title, agent, folder, workspace, card
/// ("#152 Attachments") or task, whatever the case.
pub(crate) fn matches(row: &AgentRow, query: &str) -> bool {
    let card = row
        .card
        .as_ref()
        .map(|(id, title)| format!("#{id} {title}"));
    [
        Some(&row.title),
        Some(&row.agent),
        row.folder.as_ref(),
        Some(&row.space_name),
        card.as_ref(),
        row.task.as_ref(),
    ]
    .into_iter()
    .flatten()
    .any(|text| text.to_lowercase().contains(query))
}

/// `rows` in the order `held` (terminals) names, each workspace's group together: the rows it
/// doesn't name go to their group's end, in `rows`' order.
pub(crate) fn hold(rows: Vec<AgentRow>, held: &[String]) -> Vec<AgentRow> {
    let place: HashMap<&str, usize> = held
        .iter()
        .enumerate()
        .map(|(i, t)| (t.as_str(), i))
        .collect();
    let mut kept = Vec::with_capacity(rows.len());
    let mut rows = rows.into_iter().peekable();
    while let Some(first) = rows.next() {
        let mut group = vec![first];
        while let Some(next) = rows.next_if(|row| row.space == group[0].space) {
            group.push(next);
        }
        group.sort_by_key(|row| {
            place
                .get(row.terminal.as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
        kept.extend(group);
    }
    kept
}

impl Workspace {
    /// Every agent watched, grouped by workspace (the one shown, then the sidebar's order),
    /// each group in `by_state` order. Stopped workspaces, subagents' blocks and terminals with
    /// no agent have none.
    pub(crate) fn agent_rows(&self) -> Vec<AgentRow> {
        let cards = self.cards_by_block();
        let spaces = std::iter::once(&self.space)
            .chain(self.order().into_iter().filter(|s| s.id != self.space.id))
            .filter(|space| !space.stopped);
        let mut rows = Vec::new();
        for space in spaces {
            let start = rows.len();
            let remote = space.remote.as_deref();
            let remote_cards = remote.map(|r| cards_of(&r.boards));
            for (id, pane) in space
                .watched_terminals()
                .filter(|(_, p)| p.follows.is_none())
            {
                let Some((doing, since)) = pane.watch.shown() else {
                    continue;
                };
                // Another computer's agent: what home says of it, under its computer's name.
                if let (Some(remote), Some(home)) = (remote, &pane.remote) {
                    let agent = home.agent.as_ref();
                    rows.push(AgentRow {
                        space: space.id.clone(),
                        space_name: format!(
                            "{} · {}",
                            space.name,
                            crate::mirror::computer_of(remote)
                        ),
                        terminal: pane.term.id().to_string(),
                        title: home.title.clone(),
                        agent: agent
                            .map(|a| self.home_kind_title(&space.id, &a.agent))
                            .unwrap_or_default(),
                        doing,
                        since,
                        card: remote_cards
                            .as_ref()
                            .and_then(|c| c.get(&home.block))
                            .cloned(),
                        task: agent
                            .and_then(|a| a.task.as_ref())
                            .map(|t| t.text.clone())
                            .filter(|text| !text.is_empty()),
                        folder: None,
                        context: pane.usage.as_ref().and_then(crate::usage::context_text),
                        dim: remote.dim(),
                    });
                    continue;
                }
                rows.push(AgentRow {
                    space: space.id.clone(),
                    space_name: space.name.clone(),
                    terminal: pane.term.id().to_string(),
                    title: pane.term.title(),
                    agent: self.agent_name(pane).unwrap_or_default().to_string(),
                    doing,
                    since,
                    card: space.uids.get(&id).and_then(|uid| cards.get(uid)).cloned(),
                    task: pane
                        .handed
                        .as_ref()
                        .map(|h| h.text.clone())
                        .filter(|text| !text.is_empty()),
                    // As last read: `term.cwd()` inspects the process, too dear for every row.
                    folder: pane.seen.folder.as_ref().map(|f| f.display().to_string()),
                    context: pane.usage.as_ref().and_then(crate::usage::context_text),
                    dim: false,
                });
            }
            rows[start..].sort_by(by_state);
        }
        rows
    }

    /// The rows the popup lists: `agent_rows` the chip and the search let through; while the
    /// pointer is over the list they keep the order they were last listed in.
    pub(crate) fn listed_agents(&self) -> Vec<AgentRow> {
        self.listed_in(&self.agent_rows())
    }

    /// `listed_agents` of the rows `all` (`agent_rows`, made once by the caller).
    pub(crate) fn listed_in(&self, all: &[AgentRow]) -> Vec<AgentRow> {
        let query = self.agents_query.to_lowercase();
        let rows = all
            .iter()
            .filter(|row| {
                self.agents_show.shows(&row.doing) && (query.is_empty() || matches(row, &query))
            })
            .cloned();
        if self.agents_hover {
            return hold(rows.collect(), &self.agents_held.borrow());
        }
        let rows: Vec<AgentRow> = rows.collect();
        *self.agents_held.borrow_mut() = rows.iter().map(|row| row.terminal.clone()).collect();
        rows
    }

    /// How many agents each chip (`Show::ALL`) would list, of those the search matches, among
    /// the rows `all` (`agent_rows`, made once by the caller).
    pub(crate) fn counts_in(&self, all: &[AgentRow]) -> [usize; 5] {
        let query = self.agents_query.to_lowercase();
        let rows: Vec<&AgentRow> = all
            .iter()
            .filter(|row| query.is_empty() || matches(row, &query))
            .collect();
        Show::ALL.map(|show| rows.iter().filter(|row| show.shows(&row.doing)).count())
    }
}

/// What the list draws, one height each: a header before each workspace's rows.
pub(crate) enum Item {
    /// A group's header; `dim` for a remote that isn't live.
    Header {
        space: String,
        name: String,
        dim: bool,
    },
    Row(Box<AgentRow>),
}

/// `rows` (grouped, as `agent_rows` makes them) with a header before each group.
pub(crate) fn items(rows: Vec<AgentRow>) -> Vec<Item> {
    let mut items = Vec::with_capacity(rows.len() + 1);
    for row in rows {
        let new = !matches!(items.last(), Some(Item::Row(last)) if last.space == row.space);
        if new {
            items.push(Item::Header {
                space: row.space.clone(),
                name: row.space_name.clone(),
                dim: row.dim,
            });
        }
        items.push(Item::Row(Box::new(row)));
    }
    items
}

/// Where the list has the row of `terminal`.
pub(crate) fn item_of(items: &[Item], terminal: &str) -> Option<usize> {
    items
        .iter()
        .position(|item| matches!(item, Item::Row(row) if row.terminal == terminal))
}

impl Workspace {
    /// Where the popup has its selection in `listed`: the agent the person chose (`agents_at`) if
    /// it is there, else the one shown in its stead (`agents_pinned`), else the first row.
    pub(crate) fn selected_in(&self, listed: &[AgentRow]) -> Option<usize> {
        let find = |terminal: &Option<String>| {
            let terminal = terminal.as_ref()?;
            listed.iter().position(|row| &row.terminal == terminal)
        };
        find(&self.agents_at)
            .or_else(|| find(&self.agents_pinned))
            .or_else(|| (!listed.is_empty()).then_some(0))
    }

    /// The agent the list has selected (`selected_in`): the choice, else the pinned row, else the
    /// first.
    pub(crate) fn agents_selected(&self) -> Option<AgentRow> {
        let rows = self.listed_agents();
        let at = self.selected_in(&rows)?;
        rows.into_iter().nth(at)
    }

    /// The agent shown selected stays the one shown in the choice's stead as the order changes
    /// (`agents_pinned`), each frame the popup is open. The person's choice stays theirs while the
    /// filter or the search hides it, and goes once its agent is gone.
    pub(crate) fn pin_agent(&mut self, all: &[AgentRow], listed: &[AgentRow]) {
        let running = |terminal: &String| all.iter().any(|row| &row.terminal == terminal);
        if self.agents_at.as_ref().is_some_and(|at| !running(at)) {
            self.agents_at = None;
        }
        self.agents_pinned = self
            .selected_in(listed)
            .map(|at| listed[at].terminal.clone());
    }

    /// Opens the popup with the keyboard in its search; the Git popup and the inbox close.
    pub(crate) fn open_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.agents_open = true;
        self.inbox_open = false;
        self.git_open = false;
        self.automations_open = false;
        // The list was gone when the pointer left it, if it was over it as the popup closed.
        self.agents_hover = false;
        let search = match self.agents_search.clone() {
            Some(search) => search,
            None => {
                let search = cx.new(|cx| InputState::new(window, cx).placeholder("Find an agent"));
                self._agents_search_sub =
                    Some(cx.subscribe(&search, |ws, field, event: &InputEvent, cx| {
                        if let InputEvent::Change = event {
                            ws.agents_query = field.read(cx).value().trim().to_string();
                            cx.notify();
                        }
                    }));
                self.agents_search = Some(search.clone());
                search
            }
        };
        search.update(cx, |field, cx| field.focus(window, cx));
        let all = self.agent_rows();
        let rows = self.listed_in(&all);
        self.pin_agent(&all, &rows);
        if let Some(terminal) = self.agents_pinned.clone() {
            self.show_agent(rows, &terminal, false);
        }
        cx.notify();
    }

    /// Closes the popup; the keyboard goes back to the canvas only if the popup had it.
    pub(crate) fn close_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keys = self.agents_keys(window, cx);
        self.agents_open = false;
        if keys {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    /// The sidebar's button, and the shortcut.
    pub(crate) fn toggle_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agents_open {
            self.close_agents(window, cx);
        } else {
            self.open_agents(window, cx);
        }
    }

    /// The popup has the keyboard: its search field holds it.
    pub(crate) fn agents_keys(&self, window: &Window, cx: &App) -> bool {
        self.agents_open
            && self
                .agents_search
                .as_ref()
                .is_some_and(|field| field.read(cx).focus_handle(cx).is_focused(window))
    }

    /// The list scrolls, once drawn, to show the row of `terminal`; going up, the header above
    /// the first row of a group shows too.
    fn show_agent(&self, listed: Vec<AgentRow>, terminal: &str, up: bool) {
        let items = items(listed);
        if let Some(ix) = item_of(&items, terminal) {
            let ix = match ix {
                ix if up && ix > 0 && matches!(items[ix - 1], Item::Header { .. }) => ix - 1,
                ix => ix,
            };
            self.agents_list.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }

    /// Selects the agent of `terminal`, and gives the popup the keyboard.
    pub(crate) fn select_agent(
        &mut self,
        terminal: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agents_at = Some(terminal.to_string());
        self.show_agent(self.listed_agents(), terminal, false);
        if let Some(field) = &self.agents_search {
            field.update(cx, |field, cx| field.focus(window, cx));
        }
        cx.notify();
    }

    /// Chooses a chip.
    pub(crate) fn set_agents_show(&mut self, show: Show, cx: &mut Context<Self>) {
        self.agents_show = show;
        cx.notify();
    }

    /// Go to terminal: the block of `terminal` is revealed, and the popup closes.
    pub(crate) fn go_to_agent(
        &mut self,
        terminal: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((space, id)) = self.watched_caller(terminal) else {
            return;
        };
        self.close_agents(window, cx);
        self.reveal(&space, id, window, cx);
    }

    /// Open request: the inbox opens, in the workspace of `terminal`, at its oldest request.
    pub(crate) fn open_agent_request(
        &mut self,
        terminal: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The request's own workspace: a viewer of home's block isn't in `caller`'s.
        let Some((key, space)) = self
            .asking(terminal)
            .and_then(|(wait, _)| Some((wait.key, self.request_space(wait)?)))
        else {
            return;
        };
        self.show_request(&space, key, window, cx);
    }

    /// ↑ / ↓: the previous or next row is selected (none past the ends).
    fn agents_step(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.listed_agents();
        if let Some(at) = self.selected_in(&rows)
            && let Some(next) = at.checked_add_signed(by).and_then(|i| rows.get(i))
        {
            let terminal = next.terminal.clone();
            self.agents_at = Some(terminal.clone());
            self.show_agent(rows, &terminal, by < 0);
            if let Some(field) = &self.agents_search {
                field.update(cx, |field, cx| field.focus(window, cx));
            }
            cx.notify();
        }
    }

    /// The popup's keys, run before its search field gets them: ↑ ↓ move in the list, Enter is
    /// Go to terminal, Esc clears the search and then closes the popup; the workspace's
    /// shortcuts go on, but those that act on the terminal in front. Other keys are the field's.
    pub(crate) fn on_agents_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let k = &e.keystroke;
        let command = super::effective(
            self.keymap.command(k),
            false,
            self.space.grid.is_some(),
            false,
        );
        // The theme picker takes no focus: while it is open its keys are its own.
        if self.picker.is_some() {
            self.command_key(e, command, window, cx);
            cx.stop_propagation();
            return;
        }
        let handled = !k.modifiers.modified()
            && match k.key.as_str() {
                "up" | "down" => {
                    self.agents_step(if k.key == "up" { -1 } else { 1 }, window, cx);
                    true
                }
                "enter" => {
                    if let Some(row) = self.agents_selected() {
                        self.go_to_agent(&row.terminal, window, cx);
                    }
                    true
                }
                "escape" => {
                    match self
                        .agents_search
                        .clone()
                        .filter(|_| !self.agents_query.is_empty())
                    {
                        Some(field) => {
                            field.update(cx, |field, cx| field.set_value("", window, cx));
                            self.agents_query.clear();
                            cx.notify();
                        }
                        None => self.close_agents(window, cx),
                    }
                    true
                }
                _ => false,
            };
        if handled {
            cx.stop_propagation();
            return;
        }
        let command = command.filter(|c| {
            !matches!(
                c,
                Command::Copy
                    | Command::Paste
                    | Command::CloseTerminal
                    | Command::PreviousPrompt
                    | Command::NextPrompt
                    | Command::Find
            )
        });
        if command.is_some() {
            self.command_key(e, command, window, cx);
            cx.stop_propagation();
        }
    }
}
