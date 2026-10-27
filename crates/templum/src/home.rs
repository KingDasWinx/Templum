//! Home, Templum's starting point (docs/specs/home.md): what waits for the person across every
//! workspace, each workspace as a card found by name or folder and opened with a click or Enter,
//! and recent folders to start one from. A full-window screen (docs/specs/design-system.md,
//! Screens): window chrome beside the sidebar, over the canvas.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, TITLE_BAR_HEIGHT};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, BoxShadow, ClickEvent, Context, ElementId, Entity, FocusHandle, FontWeight,
    Hsla, KeyDownEvent, MouseButton, MouseDownEvent, ScrollHandle, SharedString, TestSupportExt,
    Window, div, point, px,
};

use crate::grid::hsla;
use crate::keys::Command;
use crate::menu::Target;
use crate::motion;
use crate::popup;
use crate::sidebar;
use crate::theme::Ui;
use crate::workspace::agents_view::{Doing, Show};
use crate::workspace::{Body, Space, Workspace, workspace_digit};

/// The column's widest; a narrower room gets a narrower one.
pub(crate) const CONTENT_MAX: f32 = 1040.0;
/// A card's narrowest: the grid has as many columns of it as fit, at most four.
const CARD_MIN: f32 = 220.0;
const GAP: f32 = 16.0;
/// The most recent folders shown.
const RECENT: usize = 6;

/// The open Home, or the closed one still fading out.
pub(crate) struct Screen {
    pub since: Instant,
    pub closing: bool,
    pub search: Entity<InputState>,
    /// The card the arrows moved to, among those shown; None: the first, until an arrow.
    pub highlight: Option<usize>,
    /// What had the keyboard before: it gets it back.
    pub before: Option<FocusHandle>,
    pub scroll: ScrollHandle,
}

impl Screen {
    /// How much of the Home is up at `now`: 0 hidden, 1 all.
    pub fn shown(&self, now: Instant) -> f32 {
        motion::slid(self.since, self.closing, now)
    }

    /// The Home fading the other way, from where it is.
    pub fn reversed(self, now: Instant) -> Screen {
        Screen {
            since: motion::turned(self.since, self.closing, now),
            closing: !self.closing,
            ..self
        }
    }
}

/// What a workspace holds, as its card tells it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Counts {
    pub terminals: usize,
    pub notes: usize,
    pub boards: usize,
    pub checklists: usize,
    pub editors: usize,
    /// The agents running, by name, with how many.
    pub agents: Vec<(String, usize)>,
    /// The agents waiting for the person.
    pub needs: usize,
    /// Its programs were ended: the counts are its saved canvas's.
    pub stopped: bool,
    /// The computer it mirrors and the connection's state (`live`, `reconnecting`…), when it is a
    /// remote workspace: its agents are counted at work.
    pub mirror: Option<(String, &'static str)>,
}

impl Counts {
    /// "3 terminals · 1 note · 2 boards"; "Stopped · 2 terminals"; "Mirror of winx-desktop ·
    /// live · 3 terminals"; "Empty" for nothing.
    pub fn line(&self) -> String {
        let count = |n: usize, one: &str, many: &str| match n {
            0 => None,
            1 => Some(format!("1 {one}")),
            n => Some(format!("{n} {many}")),
        };
        let parts: Vec<String> = [
            count(self.terminals, "terminal", "terminals"),
            count(self.notes, "note", "notes"),
            count(self.boards, "board", "boards"),
            count(self.checklists, "checklist", "checklists"),
            count(self.editors, "editor", "editors"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let line = if parts.is_empty() {
            "Empty".to_string()
        } else {
            parts.join(" · ")
        };
        match (&self.mirror, self.stopped) {
            (Some((computer, state)), _) => format!("Mirror of {computer} · {state} · {line}"),
            (None, true) => format!("Stopped · {line}"),
            (None, false) => line,
        }
    }

    /// "2 Claude Code running · 1 Codex running" ("working" on a mirror); None without agents.
    pub fn running(&self) -> Option<String> {
        let verb = if self.mirror.is_some() {
            "working"
        } else {
            "running"
        };
        let parts: Vec<String> = (self.agents.iter())
            .map(|(agent, n)| format!("{n} {agent} {verb}"))
            .collect();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    /// "1 needs you", "2 need you"; None when none does.
    pub fn needs_words(&self) -> Option<String> {
        match self.needs {
            0 => None,
            1 => Some("1 needs you".into()),
            n => Some(format!("{n} need you")),
        }
    }

    fn agent(&mut self, name: &str) {
        match self.agents.iter_mut().find(|(a, _)| a == name) {
            Some((_, n)) => *n += 1,
            None => self.agents.push((name.to_string(), 1)),
        }
    }
}

/// What waits for the person, a tile of the Home each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wait {
    NeedsYou,
    Done,
    Requests,
    Automations,
}

impl Wait {
    pub(crate) const ALL: [Wait; 4] = [
        Wait::NeedsYou,
        Wait::Done,
        Wait::Requests,
        Wait::Automations,
    ];

    /// The words under its count `n`.
    pub(crate) fn words(self, n: usize) -> &'static str {
        match (self, n == 1) {
            (Wait::NeedsYou, true) => "agent needs you",
            (Wait::NeedsYou, false) => "agents need you",
            (Wait::Done, true) => "agent done",
            (Wait::Done, false) => "agents done",
            (Wait::Requests, true) => "request waits",
            (Wait::Requests, false) => "requests wait",
            (Wait::Automations, true) => "automation went wrong",
            (Wait::Automations, false) => "automations went wrong",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Wait::NeedsYou => "home-needs-you",
            Wait::Done => "home-done",
            Wait::Requests => "home-requests",
            Wait::Automations => "home-automations",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Wait::NeedsYou => IconName::Bot,
            Wait::Done => IconName::CircleCheck,
            Wait::Requests => IconName::Inbox,
            Wait::Automations => IconName::AlarmClock,
        }
    }

    /// Yellow for what waits, red for what went wrong, the accent for what is done.
    fn color(self, ws: &Workspace) -> Hsla {
        let ansi = &ws.theme.palette.ansi;
        match self {
            Wait::NeedsYou | Wait::Requests => hsla(ansi[3], 1.0),
            Wait::Done => ws.ui.accent,
            Wait::Automations => hsla(ansi[1], 1.0),
        }
    }
}

/// "Good morning" from 5:00, "Good afternoon" from 12:00, "Good evening" from 18:00.
pub(crate) fn greeting(hour: u32) -> &'static str {
    match hour {
        5..12 => "Good morning",
        12..18 => "Good afternoon",
        _ => "Good evening",
    }
}

/// What the header says under the greeting: "4 workspaces · 3 agents at work".
pub(crate) fn holds_words(workspaces: usize, at_work: usize) -> String {
    let spaces = match workspaces {
        1 => "1 workspace".to_string(),
        n => format!("{n} workspaces"),
    };
    match at_work {
        0 => spaces,
        1 => format!("{spaces} · 1 agent at work"),
        n => format!("{spaces} · {n} agents at work"),
    }
}

/// A card's foot: "Open now" for the workspace behind the Home, else "Opened 3 h ago"; None for
/// one never opened since this was kept. Times are `notes::now`'s.
pub(crate) fn opened_words(opened: Option<u64>, current: bool, now: u64) -> Option<String> {
    if current {
        return Some("Open now".into());
    }
    opened.map(|at| format!("Opened {}", crate::notes::ago(at, now).to_lowercase()))
}

/// A workspace being dragged by its card or its sidebar tile: dropped on another, it takes
/// that one's place. It draws itself under the pointer.
#[derive(Clone)]
pub(crate) struct SpaceDrag {
    pub id: String,
    pub name: SharedString,
    pub ui: Ui,
}

impl Render for SpaceDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let ui = self.ui;
        div()
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(8.0))
            .bg(ui.surface)
            .border_1()
            .border_color(ui.accent)
            .shadow(vec![BoxShadow {
                color: ui.shadow,
                offset: point(px(0.0), px(8.0)),
                blur_radius: px(20.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .text_size(px(13.0))
            .text_color(ui.title)
            .opacity(0.92)
            .child(self.name.clone())
    }
}

/// Lets `el`, workspace `space`'s card or tile at position `at`, be dragged and dropped on.
pub(crate) fn draggable(
    el: gpui_kit::Stateful<gpui_kit::Div>,
    space: &Space,
    at: usize,
    ui: Ui,
    cx: &mut Context<Workspace>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let drag = SpaceDrag {
        id: space.id.clone(),
        name: space.name.clone().into(),
        ui,
    };
    el.on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
        .drag_over::<SpaceDrag>(move |s, _, _, _| s.border_color(ui.accent))
        .on_drop(cx.listener(move |ws, drag: &SpaceDrag, window, cx| {
            ws.move_space(&drag.id, at, window, cx);
            cx.stop_propagation();
        }))
}

/// The room's padding at each side of the column, in a room `room` wide.
fn side(room: f32) -> f32 {
    if room < 640.0 { 16.0 } else { 32.0 }
}

/// The column's width in a room `room` wide.
pub(crate) fn content_width(room: f32) -> f32 {
    CONTENT_MAX.min(room - 2.0 * side(room)).max(0.0)
}

/// Columns of cards in a column `width` wide: as many `CARD_MIN` ones as fit, one to four.
pub(crate) fn columns(width: f32) -> usize {
    (((width + GAP) / (CARD_MIN + GAP)) as usize).clamp(1, 4)
}

/// The blocks' unfocused shadow, under a card.
fn card_shadow(ui: Ui) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: ui.shadow,
        offset: point(px(0.0), px(4.0)),
        blur_radius: px(12.0),
        spread_radius: px(0.0),
        inset: false,
    }]
}

impl Workspace {
    /// The Home is up (not fading out).
    pub(crate) fn home_up(&self) -> bool {
        self.screen.as_ref().is_some_and(|s| !s.closing)
    }

    /// The width the Home has: the window's, less the sidebar's.
    fn home_room(&self, window: &Window) -> f32 {
        let left = if self.sidebar { sidebar::WIDTH } else { 0.0 };
        f32::from(window.viewport_size().width) - left
    }

    /// Opens the Home already up: Templum starts on it.
    pub(crate) fn open_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_screen(window, cx);
        if let Some(screen) = &mut self.screen {
            screen.since = screen
                .since
                .checked_sub(motion::SLIDE)
                .unwrap_or(screen.since);
        }
    }

    /// Opens the Home, its search field taking the keyboard, or closes it. It never shows over
    /// the welcome: the welcome makes way.
    pub(crate) fn toggle_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        match self.screen.take() {
            Some(open) if !open.closing => {
                self.screen = Some(open);
                self.close_screen(window, cx);
                return;
            }
            // Still fading out: it turns back from where it is.
            Some(closing) => self.screen = Some(closing.reversed(now)),
            None => {
                let search = cx.new(|cx| {
                    InputState::new(window, cx).placeholder("Find a workspace by name or folder")
                });
                cx.subscribe(&search, |ws, _, event: &InputEvent, cx| {
                    if let InputEvent::Change = event {
                        if let Some(screen) = &mut ws.screen {
                            screen.highlight = None;
                        }
                        cx.notify();
                    }
                })
                .detach();
                self.screen = Some(Screen {
                    since: now,
                    closing: false,
                    search,
                    highlight: None,
                    before: window.focused(cx),
                    scroll: ScrollHandle::new(),
                });
            }
        }
        self.menu = None;
        self.close_welcome(window, cx);
        if let Some(screen) = &self.screen {
            screen.search.update(cx, |s, cx| s.focus(window, cx));
        }
        cx.notify();
    }

    /// Fades the Home out, and gives the keyboard back to what had it.
    pub(crate) fn close_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.home_up() {
            return;
        }
        let Some(open) = self.screen.take() else {
            return;
        };
        let before = open.before.clone();
        self.screen = Some(open.reversed(Instant::now()));
        window.focus(before.as_ref().unwrap_or(&self.focus_handle), cx);
        cx.notify();
    }

    /// The cards shown: the workspaces whose name or folder has the search's text, by their
    /// position.
    fn screen_cards(&self, cx: &App) -> Vec<usize> {
        let query = self
            .screen
            .as_ref()
            .map(|s| s.search.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        self.order()
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                let folder = s.folder.as_deref().map(|f| {
                    format!("{}\n{}", f.display(), crate::agents::tilde(f)).to_lowercase()
                });
                s.name.to_lowercase().contains(&query) || folder.is_some_and(|f| f.contains(&query))
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Shows the workspace at `at` (its position) and closes the Home.
    fn open_card(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.close_screen(window, cx);
        self.switch_to(at, window, cx);
    }

    /// The Home's keys, whoever has the keyboard: what the title bar opened over it first (the
    /// theme picker its keys, the usage popover Esc); then Esc closes it, Up and Down move the
    /// highlight a row, Enter opens it, Ctrl+digit jumps; Templum's own shortcuts run. False for
    /// anything else, which is typing.
    pub(crate) fn home_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let k = &e.keystroke;
        let plain = !k.modifiers.modified();
        let command = self.keymap.command(k);
        if self.picker.is_some() {
            self.command_key(e, command, window, cx);
            return true;
        }
        if self.usage_open && k.key == "escape" {
            self.usage_open = false;
            cx.notify();
            return true;
        }
        let cards = self.screen_cards(cx);
        let step = columns(content_width(self.home_room(window)));
        match k.key.as_str() {
            "escape" if plain => self.close_screen(window, cx),
            "up" | "down" if plain => {
                if let Some(screen) = &mut self.screen
                    && !cards.is_empty()
                {
                    let at = screen.highlight.unwrap_or(0);
                    screen.highlight = Some(if k.key == "up" {
                        at.saturating_sub(step)
                    } else {
                        (at + step).min(cards.len() - 1)
                    });
                    cx.notify();
                }
            }
            "enter" if plain => {
                let at = self.screen.as_ref().and_then(|s| s.highlight).unwrap_or(0);
                if let Some(&card) = cards.get(at) {
                    self.open_card(card, window, cx);
                }
            }
            _ => {
                if let Some(to) = workspace_digit(k, cfg!(target_os = "macos"))
                    && to <= self.others.len()
                {
                    self.open_card(to, window, cx);
                } else if let Some(command) = command.filter(|c| {
                    matches!(
                        c,
                        Command::Workspaces
                            | Command::NewWorkspace
                            | Command::ConnectTemplum
                            | Command::NextWorkspace
                            | Command::PreviousWorkspace
                            | Command::ToggleSidebar
                            | Command::Settings
                            | Command::ThemePicker
                            | Command::GoToWaiting
                            | Command::AllAgents
                            | Command::Automations
                            | Command::Quit
                    )
                }) {
                    // Through command_key: holding the quit shortcut isn't a second press.
                    self.command_key(e, Some(command), window, cx);
                } else {
                    return false;
                }
            }
        }
        true
    }

    /// The Home's keys while its search field has them.
    fn on_screen_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.home_key(e, window, cx) {
            cx.stop_propagation();
        }
    }

    /// A key that reached the canvas while the Home is up: the Home's keys run, and anything
    /// else gives its search the keyboard (a terminal behind never gets it).
    pub(crate) fn home_key_from_canvas(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.home_key(e, window, cx)
            && let Some(screen) = &self.screen
        {
            screen.search.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    /// What `space` holds: its blocks by kind, the agents running in its terminals and those
    /// waiting for the person.
    pub(crate) fn counts(&self, space: &Space) -> Counts {
        let mut counts = Counts {
            stopped: space.stopped,
            mirror: space.remote.as_deref().map(|r| {
                let computer = r.me.as_ref().map_or_else(
                    || crate::workspace::remote::host_of(&r.entry.url),
                    |m| m.computer.clone(),
                );
                (computer, r.status.word())
            }),
            ..Counts::default()
        };
        if space.stopped {
            use crate::session::Item;
            for item in space.saved.iter().flat_map(|s| &s.items) {
                match item {
                    Item::Terminal(_) => counts.terminals += 1,
                    Item::Note(_) => counts.notes += 1,
                    Item::Kanban(_) => counts.boards += 1,
                    Item::Checklist(_) => counts.checklists += 1,
                    Item::Editor(_) => counts.editors += 1,
                    // A Git block of an older Templum: the workspace's Git once it starts.
                    Item::Git(_) => {}
                }
            }
            return counts;
        }
        for body in space.blocks.values() {
            match body {
                Body::Terminal(pane) => {
                    counts.terminals += 1;
                    // A mirror counts the agents at work at home.
                    let working = pane.remote.is_none()
                        || matches!(
                            self.doing(pane),
                            Doing::Working(_) | Doing::AwaitsSubagents(_)
                        );
                    if working && let Some(agent) = self.agent_name(pane) {
                        counts.agent(agent);
                    }
                    // As the Agents view lists it.
                    if pane.follows.is_none()
                        && matches!(pane.watch.shown(), Some((Doing::NeedsYou(_), _)))
                    {
                        counts.needs += 1;
                    }
                }
                Body::Note(_) => counts.notes += 1,
                Body::Kanban(_) => counts.boards += 1,
                Body::Checklist(_) => counts.checklists += 1,
                Body::Editor(_) => counts.editors += 1,
                Body::Mirrored(card) => match card.kind.as_str() {
                    "note" => counts.notes += 1,
                    "kanban" => counts.boards += 1,
                    "checklist" => counts.checklists += 1,
                    "editor" => counts.editors += 1,
                    _ => {}
                },
            }
        }
        counts
    }

    /// How many of each `Wait` there are, across every workspace that runs.
    pub(crate) fn waiting(&self, rows: &[crate::workspace::agents_view::AgentRow]) -> [usize; 4] {
        Wait::ALL.map(|wait| match wait {
            Wait::NeedsYou => rows
                .iter()
                .filter(|r| matches!(r.doing, Doing::NeedsYou(_)))
                .count(),
            Wait::Done => rows.iter().filter(|r| r.doing == Doing::Done).count(),
            Wait::Requests => self.requests_waiting().len(),
            Wait::Automations => self.automations_unseen() + self.home_unseen(),
        })
    }

    /// A press on a tile: the Home closes and the popup of what waits opens on its first item,
    /// in its workspace.
    pub(crate) fn home_press(&mut self, wait: Wait, window: &mut Window, cx: &mut Context<Self>) {
        match wait {
            Wait::NeedsYou => self.agents_on(Show::NeedsYou, window, cx),
            Wait::Done => self.agents_on(Show::Done, window, cx),
            Wait::Requests => {
                let oldest = self
                    .requests_waiting()
                    .first()
                    .map(|(w, s)| (s.clone(), w.key));
                if let Some((space, key)) = oldest {
                    self.close_screen(window, cx);
                    self.show_request(&space, key, window, cx);
                }
            }
            Wait::Automations => {
                let first = self
                    .automation_ids()
                    .into_iter()
                    .find(|id| self.automation_runs.get(id).is_some_and(|run| run.unseen));
                let space = first
                    .and_then(|id| self.automations.get(id))
                    .and_then(|a| a.workspace.clone());
                if let Some(to) = space.and_then(|s| self.order().iter().position(|o| o.id == s)) {
                    self.switch_to(to, window, cx);
                }
                match first {
                    Some(id) => self.show_automation(id, window, cx),
                    None => self.open_automations(window, cx),
                }
            }
        }
    }

    /// The Agents popup on the chip `show`, its first agent selected (the workspace shown's
    /// first, else the sidebar's order), that agent's workspace shown.
    fn agents_on(&mut self, show: Show, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.agent_rows().into_iter().find(|r| show.shows(&r.doing)) else {
            return;
        };
        if let Some(to) = self.order().iter().position(|s| s.id == row.space) {
            self.switch_to(to, window, cx);
        }
        self.agents_show = show;
        // An old search could hide it.
        self.agents_query.clear();
        if let Some(field) = self.agents_search.clone() {
            field.update(cx, |field, cx| field.set_value("", window, cx));
        }
        self.open_agents(window, cx);
        self.select_agent(&row.terminal, window, cx);
    }

    /// The folders the workspaces opened lately (the one shown first, then the sidebar's order),
    /// `RECENT` at most, but any workspace's own folder and the mirrors'.
    pub(crate) fn recent_folders(&self) -> Vec<PathBuf> {
        let own: Vec<&Path> = self.spaces().filter_map(|s| s.folder.as_deref()).collect();
        let mut folders: Vec<PathBuf> = Vec::new();
        let spaces = std::iter::once(&self.space)
            .chain(self.order().into_iter().filter(|s| s.id != self.space.id))
            .filter(|s| !s.is_remote());
        for folder in spaces.flat_map(|s| &s.recent) {
            if !own.contains(&folder.as_path()) && !folders.contains(folder) {
                folders.push(folder.clone());
            }
        }
        folders.truncate(RECENT);
        folders
    }

    /// Open folder…: the system's folder picker, then `open_folder_space`.
    fn pick_folder_space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ws = cx.entity().downgrade();
        crate::launcher::pick_folder(
            Rc::new(move |folder: &Path, window: &mut Window, cx: &mut App| {
                ws.update(cx, |ws, cx| ws.open_folder_space(folder, window, cx))
                    .ok();
                None
            }),
            window,
            cx,
        );
    }
}

pub(crate) fn render(
    ws: &Workspace,
    screen: &Screen,
    now: Instant,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let shown = screen.shown(now);
    let left = if ws.sidebar { sidebar::WIDTH } else { 0.0 };
    let room = f32::from(window.viewport_size().width) - left;
    let width = content_width(room);
    let fg = hsla(ws.theme.palette.foreground, 1.0);
    let rows = ws.agent_rows();
    let at_work = rows.iter().filter(|r| r.doing.works()).count();
    let hour = chrono::Timelike::hour(&chrono::Local::now());

    let primary = div()
        .id("home-new")
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.0))
        .h(px(28.0))
        .px(px(12.0))
        .rounded(px(8.0))
        .bg(ui.accent.opacity(0.16))
        .text_color(ui.accent)
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |s| s.bg(ui.accent.opacity(0.24)))
        .child(Icon::new(IconName::Plus).size(px(14.0)))
        .child("New workspace")
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.add_space(window, cx)))
        .test_support();
    let header = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .min_w(px(0.0))
                .flex_1()
                .child(
                    div()
                        .text_size(px(22.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg)
                        .child(greeting(hour)),
                )
                .child(
                    div()
                        .id("home-holds")
                        .text_size(px(13.0))
                        .text_color(ui.title_dim)
                        .truncate()
                        .child(holds_words(ws.order().len(), at_work))
                        .test_support(),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_size(px(13.0))
                .child(primary)
                .child(
                    header_button("home-open-folder", ui)
                        .gap(px(6.0))
                        .child(Icon::new(IconName::FolderOpen).size(px(14.0)))
                        .child("Open folder…")
                        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                            ws.pick_folder_space(window, cx)
                        }))
                        .test_support(),
                )
                .child(
                    header_button("home-connect", ui)
                        .child(Icon::new(IconName::Plug).size(px(14.0)))
                        .tooltip(|window, cx| {
                            Tooltip::new("Connect to a Templum…").build(window, cx)
                        })
                        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                            ws.connect_templum(window, cx)
                        }))
                        .test_support(),
                )
                .child(
                    header_button("home-settings", ui)
                        .child(Icon::new(IconName::Settings).size(px(14.0)))
                        .tooltip(|window, cx| Tooltip::new("Settings").build(window, cx))
                        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                            ws.toggle_settings(window, cx)
                        }))
                        .test_support(),
                ),
        );
    let search = Input::new(&screen.search).cleanable(true).prefix(
        Icon::new(IconName::Search)
            .size(px(14.0))
            .text_color(ui.title_dim),
    );

    let counts = ws.waiting(&rows);
    let tiles = div()
        .grid()
        // Four when each has room for its longest words.
        .grid_cols(if width < 960.0 { 2 } else { 4 })
        .gap(px(GAP))
        .children(
            Wait::ALL
                .into_iter()
                .zip(counts)
                .map(|(wait, n)| waiting_tile(ws, wait, n, cx)),
        );

    let cards = ws.screen_cards(cx);
    let query = screen.search.read(cx).value().trim().to_string();
    // Enter opens the first card until an arrow moves on: show which, once there's a choice.
    let highlight = screen.highlight.or((!query.is_empty()).then_some(0));
    let order = ws.order();
    let wall = crate::notes::now();
    let grid = div()
        .id("screen-cards")
        .grid()
        .grid_cols(columns(width) as u16)
        .gap(px(GAP))
        .children(
            cards
                .iter()
                .enumerate()
                .map(|(n, &at)| card(ws, order[at], at, highlight == Some(n), wall, cx)),
        );
    let workspaces = popup::section(ws, "Workspaces")
        .gap(px(12.0))
        .child(grid)
        .when(cards.is_empty(), |el| {
            el.child(
                div()
                    .text_size(px(13.0))
                    .text_color(ui.title_dim)
                    .child(format!("No workspace matches “{query}”")),
            )
        });

    let recent = ws.recent_folders();
    let recent = (!recent.is_empty()).then(|| {
        popup::section(ws, "Recent folders").gap(px(12.0)).child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.0))
                .children(recent.into_iter().enumerate().map(|(i, folder)| {
                    let name = folder.file_name().map_or_else(
                        || folder.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    let tip = SharedString::from(crate::agents::tilde(&folder));
                    div()
                        .id(("home-recent", i))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .h(px(30.0))
                        .px(px(10.0))
                        .rounded(px(8.0))
                        .bg(ui.surface)
                        .border_1()
                        .border_color(ui.hairline)
                        .text_size(px(13.0))
                        .text_color(ui.title)
                        .cursor_pointer()
                        .hover(move |s| s.border_color(ui.hover_border).text_color(fg))
                        .child(
                            Icon::new(IconName::Folder)
                                .size(px(14.0))
                                .text_color(ui.title_dim),
                        )
                        .child(name)
                        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                            ws.open_folder_space(&folder, window, cx)
                        }))
                        .test_support()
                })),
        )
    });

    let column = div()
        .w(px(width))
        .mx_auto()
        // It rises into place as it fades in.
        .pt(px(40.0 + (1.0 - shown) * 24.0))
        .pb(px(48.0))
        .flex()
        .flex_col()
        .gap(px(28.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(16.0))
                .child(header)
                .child(search),
        )
        .child(
            popup::section(ws, "Waiting for you")
                .gap(px(12.0))
                .child(tiles),
        )
        .child(workspaces)
        .children(recent);
    div()
        .id("screen")
        .absolute()
        .top(TITLE_BAR_HEIGHT)
        .left(px(left))
        .right_0()
        .bottom_0()
        // Nothing under it gets the pointer; fading out, it lets the canvas have it at once.
        .when(!screen.closing, |el| el.occlude())
        .bg(ui.canvas.opacity(shown))
        .text_color(fg)
        .on_key_down(cx.listener(Workspace::on_screen_key))
        .child(
            div()
                .relative()
                .size_full()
                .opacity(shown)
                .child(
                    div()
                        .id("home-scroll")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&screen.scroll)
                        .child(column),
                )
                .child(crate::kanban::scrollbar_over(
                    &screen.scroll,
                    "home-scrollbar".into(),
                    1.0,
                )),
        )
        .test_support()
        .into_any_element()
}

/// A ghost button of the header: words, an icon, or both.
fn header_button(id: &'static str, ui: Ui) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .h(px(28.0))
        .min_w(px(28.0))
        .px(px(8.0))
        .rounded(px(8.0))
        .text_size(px(13.0))
        .text_color(ui.title)
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .active(move |s| s.bg(ui.pressed))
}

/// What waits, `n` of it: its tile, its count and its words; a press away when there is any.
fn waiting_tile(ws: &Workspace, wait: Wait, n: usize, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.ui;
    let calm = n == 0;
    let color = if calm { ui.title_dim } else { wait.color(ws) };
    div()
        .id(wait.id())
        .flex()
        .items_center()
        .gap(px(12.0))
        .min_w(px(0.0))
        .p(px(14.0))
        .rounded(px(12.0))
        .bg(ui.surface)
        .border_1()
        .border_color(ui.hairline)
        .shadow(card_shadow(ui))
        .child(popup::tile(wait.icon(), color))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.0))
                .child(
                    div()
                        .text_size(px(20.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .when(calm, |el| el.text_color(ui.title_dim))
                        .child(n.to_string()),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(ui.title_dim)
                        .truncate()
                        .child(wait.words(n)),
                ),
        )
        .when(!calm, |el| {
            el.cursor_pointer()
                .hover(move |s| s.border_color(ui.hover_border))
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.home_press(wait, window, cx)
                }))
        })
        .test_support()
        .into_any_element()
}

/// One workspace's card: its tile, name and folder, what it holds, its agents and when it was
/// last opened; its menu at the top right; the one behind the Home ringed.
fn card(
    ws: &Workspace,
    space: &Space,
    at: usize,
    highlighted: bool,
    wall: u64,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let active = at == ws.active;
    let id = space.id.clone();
    let shortcut = (at < 9).then(|| {
        let key = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };
        SharedString::from(format!("{} · {key}+{}", space.name, at + 1))
    });
    let dot = |color: Hsla| div().size(px(7.0)).flex_shrink_0().rounded_full().bg(color);
    let counts = ws.counts(space);
    let yellow = hsla(ws.theme.palette.ansi[3], 1.0);
    let agents = (counts.running().is_some() || counts.needs > 0).then(|| {
        div()
            .id(ElementId::Name(
                format!("space-card-agents-{}", space.id).into(),
            ))
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(10.0))
            .gap_y(px(2.0))
            .text_size(px(12.0))
            .when_some(counts.running(), |el, running| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .min_w(px(0.0))
                        .text_color(ui.title)
                        .child(dot(hsla(ws.theme.palette.ansi[2], 1.0)))
                        .child(div().truncate().child(running)),
                )
            })
            .when_some(counts.needs_words(), |el, needs| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .text_color(yellow)
                        .child(dot(yellow))
                        .child(needs),
                )
            })
            .test_support()
    });
    let folder = space.folder.as_deref().map(crate::agents::tilde);
    let menu_id = space.id.clone();
    let el = div().id(ElementId::Name(format!("space-card-{}", space.id).into()));
    draggable(el, space, at, ui, cx)
        .min_w(px(0.0))
        .min_h(px(156.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .p(px(16.0))
        .rounded(px(12.0))
        // Opaque: the active card's ring is a shadow, which a see-through fill would show
        // through, blue behind the text.
        .bg(ui.surface)
        .border_1()
        .cursor_pointer()
        // Stopped, or a mirror that isn't live: dimmed as its sidebar tile, but while the keyboard
        // is on it.
        .when(
            !highlighted && (space.stopped || space.remote.as_deref().is_some_and(|r| r.dim())),
            |el| el.opacity(0.45),
        )
        .map(|el| {
            if active {
                el.border_color(ui.accent).shadow(vec![BoxShadow {
                    color: ui.ring,
                    offset: point(px(0.0), px(0.0)),
                    blur_radius: px(0.0),
                    spread_radius: px(2.0),
                    inset: false,
                }])
            } else if highlighted {
                el.border_color(ui.hover_border)
                    .bg(ui.surface.blend(ui.hover))
                    .shadow(card_shadow(ui))
            } else {
                el.border_color(ui.hairline)
                    .shadow(card_shadow(ui))
                    .hover(move |s| s.border_color(ui.hover_border))
            }
        })
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(12.0))
                .child(sidebar::tile_face(&space.look, &space.name, &ws.theme, ui))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(space.name.clone()),
                        )
                        .when_some(folder, |el, folder| {
                            // Cut from its start: the folder's own name is what tells it.
                            el.child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(ui.title_dim)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis_start()
                                    .child(folder),
                            )
                        }),
                )
                .children(crate::share_dialog::mirror_mark(
                    ws,
                    &space.id,
                    "space-card-mirrored",
                    12.0,
                ))
                .child(
                    crate::git_block::widgets::icon_button(
                        ElementId::Name(format!("space-card-menu-{}", space.id).into()),
                        IconName::Ellipsis,
                        ui,
                        1.0,
                    )
                    .flex_shrink_0()
                    .tooltip(|window, cx| Tooltip::new("More").build(window, cx))
                    // The card's click and drag aren't this button's.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |ws, e: &ClickEvent, window, cx| {
                        ws.open_menu(Target::Space(menu_id.clone()), e.position(), window, cx);
                        cx.stop_propagation();
                    }))
                    .test_support(),
                ),
        )
        .child(
            div()
                .id(ElementId::Name(
                    format!("space-card-line-{}", space.id).into(),
                ))
                .text_size(px(12.0))
                .text_color(ui.title_dim)
                .line_clamp(2)
                .child(counts.line())
                .test_support(),
        )
        .children(agents)
        .children(opened_words(space.opened, active, wall).map(|words| {
            div()
                .id(ElementId::Name(
                    format!("space-card-opened-{}", space.id).into(),
                ))
                .mt_auto()
                .text_size(px(11.5))
                .text_color(ui.title_dim)
                .child(words)
                .test_support()
        }))
        .when_some(shortcut, |el, tip| {
            el.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        })
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| ws.open_card(at, window, cx)))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                ws.open_menu(Target::Space(id.clone()), e.position, window, cx);
                cx.stop_propagation();
            }),
        )
        .test_support()
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_says_its_blocks_then_its_agents() {
        let mut counts = Counts {
            terminals: 3,
            notes: 1,
            ..Counts::default()
        };
        assert_eq!(counts.line(), "3 terminals · 1 note");
        assert_eq!(counts.running(), None);
        counts.agent("Claude Code");
        assert_eq!(counts.running().as_deref(), Some("1 Claude Code running"));
        counts.agent("Claude Code");
        counts.agent("Codex");
        counts.boards = 2;
        assert_eq!(counts.line(), "3 terminals · 1 note · 2 boards");
        assert_eq!(
            counts.running().as_deref(),
            Some("2 Claude Code running · 1 Codex running")
        );
        counts.checklists = 2;
        assert!(counts.line().contains("2 boards · 2 checklists"));
        counts.checklists = 1;
        assert!(counts.line().ends_with("2 boards · 1 checklist"));
        assert_eq!(Counts::default().line(), "Empty");
        let stopped = Counts {
            terminals: 2,
            stopped: true,
            ..Counts::default()
        };
        assert_eq!(stopped.line(), "Stopped · 2 terminals");
        assert_eq!(stopped.needs_words(), None);
        let mirror = Counts {
            terminals: 3,
            agents: vec![("Claude Code".into(), 1)],
            needs: 2,
            mirror: Some(("winx-desktop".into(), "live")),
            ..Counts::default()
        };
        assert_eq!(mirror.line(), "Mirror of winx-desktop · live · 3 terminals");
        assert_eq!(mirror.running().as_deref(), Some("1 Claude Code working"));
        assert_eq!(mirror.needs_words().as_deref(), Some("2 need you"));
    }

    #[test]
    fn narrow_rooms_get_fewer_columns() {
        assert_eq!(content_width(1400.0), CONTENT_MAX);
        assert_eq!(columns(content_width(1400.0)), 4);
        assert_eq!(columns(content_width(900.0)), 3);
        assert_eq!(columns(content_width(700.0)), 2);
        assert_eq!(columns(content_width(400.0)), 1);
        assert_eq!(columns(0.0), 1);
    }

    #[test]
    fn the_greeting_follows_the_hour() {
        assert_eq!(greeting(4), "Good evening");
        assert_eq!(greeting(5), "Good morning");
        assert_eq!(greeting(11), "Good morning");
        assert_eq!(greeting(12), "Good afternoon");
        assert_eq!(greeting(17), "Good afternoon");
        assert_eq!(greeting(18), "Good evening");
        assert_eq!(greeting(23), "Good evening");
    }

    #[test]
    fn the_words_follow_the_counts() {
        assert_eq!(holds_words(1, 0), "1 workspace");
        assert_eq!(holds_words(4, 1), "4 workspaces · 1 agent at work");
        assert_eq!(holds_words(4, 3), "4 workspaces · 3 agents at work");
        assert_eq!(Wait::NeedsYou.words(1), "agent needs you");
        assert_eq!(Wait::NeedsYou.words(0), "agents need you");
        assert_eq!(Wait::Done.words(2), "agents done");
        assert_eq!(Wait::Requests.words(1), "request waits");
        assert_eq!(Wait::Automations.words(3), "automations went wrong");
    }

    #[test]
    fn a_card_says_when_it_was_opened() {
        let now = 1_000_000;
        assert_eq!(
            opened_words(Some(now - 10), true, now).as_deref(),
            Some("Open now")
        );
        assert_eq!(opened_words(None, false, now), None);
        assert_eq!(
            opened_words(Some(now - 3 * 3600), false, now).as_deref(),
            Some("Opened 3 h ago")
        );
        assert_eq!(
            opened_words(Some(now - 100_000), false, now).as_deref(),
            Some("Opened yesterday")
        );
        assert_eq!(
            opened_words(Some(now - 10), false, now).as_deref(),
            Some("Opened just now")
        );
    }
}
