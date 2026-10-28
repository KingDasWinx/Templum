//! The welcome: on a first run, five pages over a frosted canvas where a newcomer picks a theme,
//! a shell and the agents, and learns the keys. Every choice is a settings edit, seen at once.
//! See docs/specs/welcome.md.

use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::{Icon, TITLE_BAR_HEIGHT};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, BoxShadow, ClickEvent, Context, ElementId,
    FocusHandle, FontWeight, Hsla, KeyDownEvent, SharedString, TestSupportExt, Window, div, point,
    px,
};

use crate::autostart;
use crate::grid::hsla;
use crate::keys::Command;
use crate::motion;
use crate::settings;
use crate::sidebar;
use crate::theme::Ui;
use crate::workspace::Workspace;

/// The card's largest; smaller windows get a smaller one.
const CARD_W: f32 = 760.0;
const CARD_H: f32 = 600.0;

/// The welcome's pages, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Hello,
    Look,
    Shell,
    Agents,
    Ready,
}

impl Page {
    pub const ALL: [Page; 5] = [
        Page::Hello,
        Page::Look,
        Page::Shell,
        Page::Agents,
        Page::Ready,
    ];

    fn title(self) -> &'static str {
        match self {
            Page::Hello => "Welcome",
            Page::Look => "Look",
            Page::Shell => "Shell",
            Page::Agents => "Agents",
            Page::Ready => "Ready",
        }
    }

    fn next(self) -> Option<Page> {
        Page::ALL.get(self as usize + 1).copied()
    }

    fn previous(self) -> Option<Page> {
        (self as usize).checked_sub(1).map(|i| Page::ALL[i])
    }
}

/// The open welcome, or the closed one still fading out.
pub(crate) struct Welcome {
    pub since: Instant,
    pub closing: bool,
    pub page: Page,
    /// The last move went back: the page comes in from the left.
    pub back: bool,
    /// A first run: the first terminal waits for it to close.
    pub first: bool,
    /// The agents found on this computer, by id; looked for once, when it opened.
    pub installed: Vec<String>,
    /// What the person chose for starting Templum at login; None: nothing yet.
    pub autostart: Option<bool>,
    /// The last page, where the start-at-login option is, was shown: the person saw the option.
    pub seen_autostart: bool,
    pub focus: FocusHandle,
}

impl Welcome {
    /// What the start-at-login toggle shows, given what the system has (`system`): the person's
    /// choice; else the system's state, and in a first run, on when `system` has no entry and
    /// Templum could keep one (`State::offered`).
    pub fn autostart_shown(&self, system: &autostart::State) -> bool {
        self.autostart
            .unwrap_or(system.on() || self.first && system.offered())
    }

    /// What closing applies to the system's entry: the person's choice. In a first run, the
    /// default too (on, as shown), but only once the last page was shown: leaving before it, with
    /// Skip or Esc, never starts Templum at login for someone who didn't see the option.
    fn autostart_to_apply(&self, system: &autostart::State) -> Option<bool> {
        self.autostart
            .or((self.first && self.seen_autostart && system.offered()).then_some(true))
    }

    /// How much of it is in view at `now`: 0 hidden, 1 all.
    pub fn shown(&self, now: Instant) -> f32 {
        motion::slid(self.since, self.closing, now)
    }
}

impl Workspace {
    /// Opens the welcome on its first page, with the keyboard. `first`: a first run, whose
    /// terminal opens when it closes.
    pub(crate) fn open_welcome(
        &mut self,
        first: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.load_lists(window);
        let installed = self
            .agents
            .iter()
            .filter(|a| self.agent_installed(a))
            .map(|a| a.id.clone())
            .collect();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.menu = None;
        self.welcome = Some(Welcome {
            since: Instant::now(),
            closing: false,
            page: Page::Hello,
            back: false,
            first,
            installed,
            autostart: None,
            seen_autostart: false,
            focus,
        });
        self.load_autostart(cx);
        cx.notify();
    }

    /// Fades the welcome out and writes what was chosen; after a first run's, the first terminal
    /// opens, in the shell picked.
    pub(crate) fn close_welcome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(welcome) = self.welcome.as_mut().filter(|w| !w.closing) else {
            return;
        };
        welcome.since = motion::turned(welcome.since, false, Instant::now());
        welcome.closing = true;
        let first = welcome.first;
        let autostart = welcome.autostart_to_apply(&self.autostart);
        self.apply_and_save(window, cx);
        if let Some(on) = autostart {
            self.set_autostart(on, window, cx);
        }
        window.focus(&self.focus_handle, cx);
        if first {
            self.open_first_terminal(window, cx);
        }
        cx.notify();
    }

    /// Shows page `to`.
    pub(crate) fn welcome_page(&mut self, to: Page, cx: &mut Context<Self>) {
        if let Some(welcome) = &mut self.welcome {
            welcome.back = (to as usize) < (welcome.page as usize);
            welcome.page = to;
            welcome.seen_autostart |= to == Page::Ready;
            cx.notify();
        }
    }

    /// The next page; after the last, the welcome closes.
    fn welcome_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.welcome.as_ref().and_then(|w| w.page.next()) {
            Some(next) => self.welcome_page(next, cx),
            None => self.close_welcome(window, cx),
        }
    }

    /// Keys while the welcome has them (and its fields left them). What the title bar opened over
    /// it has them first: the theme picker its keys, the usage popover Esc. Then Esc skips the
    /// welcome, Enter goes on (not from a field), and the settings, the theme picker and quitting
    /// still run.
    fn on_welcome_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let plain = !k.modifiers.modified();
        let command = self.keymap.command(k);
        let on_card = self
            .welcome
            .as_ref()
            .is_some_and(|w| w.focus.is_focused(window));
        if self.picker.is_some() {
            self.command_key(e, command, window, cx);
        } else if self.usage_open && k.key == "escape" {
            self.usage_open = false;
            cx.notify();
        } else {
            match k.key.as_str() {
                "escape" if plain => self.close_welcome(window, cx),
                "enter" if plain && on_card => self.welcome_next(window, cx),
                _ => match command.filter(|c| {
                    matches!(c, Command::Settings | Command::ThemePicker | Command::Quit)
                }) {
                    // Through command_key: holding the quit shortcut isn't a second press.
                    Some(command) => self.command_key(e, Some(command), window, cx),
                    None => return,
                },
            }
        }
        cx.stop_propagation();
    }

    /// The welcome's toggle for starting Templum at login: applied when it closes.
    pub(crate) fn choose_autostart(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(welcome) = &mut self.welcome {
            welcome.autostart = Some(on);
            cx.notify();
        }
    }

    /// A first run's welcome is open: nothing is written yet, so quitting from it leaves a first
    /// run for next time.
    pub(crate) fn first_run_welcome(&self) -> bool {
        self.welcome.as_ref().is_some_and(|w| w.first && !w.closing)
    }
}

pub(crate) fn render(
    ws: &Workspace,
    welcome: &Welcome,
    now: Instant,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let shown = welcome.shown(now);
    let size = window.viewport_size();
    let (w, h) = (
        f32::from(size.width),
        f32::from(size.height - TITLE_BAR_HEIGHT),
    );
    let card_w = CARD_W.min(w - 48.0).max(0.0);
    let card_h = CARD_H.min(h - 48.0).max(0.0);
    let fg = hsla(ws.theme.palette.foreground, 1.0);
    let page = welcome.page;

    let steps = div()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .px(px(20.0))
        .py(px(14.0))
        .border_b_1()
        .border_color(ui.hairline)
        .children(Page::ALL.map(|p| step(ui, p, page, cx)));

    let body = match page {
        Page::Hello => hello(ws),
        Page::Look => heading(
            ui,
            fg,
            "Make it yours",
            "A theme and a font. They apply right away: look behind this window.",
        ),
        Page::Shell => heading(
            ui,
            fg,
            "Your shell",
            "What new terminals run, and where they start.",
        ),
        Page::Agents => heading(
            ui,
            fg,
            "AI agents",
            "Templum is a cockpit for terminal agents like Claude Code and Codex: open one from \
             the canvas, see when it waits for you, and keep an eye on your plan's limits.",
        ),
        Page::Ready => heading(
            ui,
            fg,
            "You're all set",
            "Two last choices, and the keys worth knowing.",
        ),
    };
    let entity = cx.entity();
    let groups = settings::welcome_groups(ws, &entity, page, &welcome.installed, window, cx);
    let dir = if welcome.back { -1.0 } else { 1.0 };
    let content = div()
        .id("welcome-scroll")
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .child(
            div()
                .relative()
                .flex()
                .flex_col()
                .gap(px(20.0))
                .px(px(32.0))
                .py(px(24.0))
                // The first page is short: it sits in the middle.
                .when(page == Page::Hello, |el| el.min_h_full().justify_center())
                .child(body)
                .children(groups)
                .when(page == Page::Ready, |el| el.child(good_to_know(ws)))
                // Each page fades in, sliding from the side it comes from.
                .with_animation(
                    SharedString::from(format!("welcome-page-{}", page as usize)),
                    Animation::new(motion::SLIDE).with_easing(motion::slide),
                    move |el, t| el.opacity(t).left(px(dir * 24.0 * (1.0 - t))),
                ),
        )
        .test_support();

    let last = page.next().is_none();
    let footer = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(20.0))
        .py(px(14.0))
        .border_t_1()
        .border_color(ui.hairline)
        .when_some(page.previous(), |el, previous| {
            el.child(
                pill("welcome-back", ui, false)
                    .gap(px(6.0))
                    .child(Icon::new(IconName::ArrowLeft).size(px(14.0)))
                    .child("Back")
                    .on_click(
                        cx.listener(move |ws, _: &ClickEvent, _, cx| ws.welcome_page(previous, cx)),
                    )
                    .test_support(),
            )
        })
        .child(div().flex_1())
        .when(!last, |el| {
            el.child(
                pill("welcome-skip", ui, false)
                    .text_color(ui.title)
                    .child("Skip")
                    .on_click(
                        cx.listener(|ws, _: &ClickEvent, window, cx| ws.close_welcome(window, cx)),
                    )
                    .test_support(),
            )
        })
        .child(
            pill("welcome-next", ui, true)
                .gap(px(6.0))
                .text_color(hsla(ws.theme.palette.background, 1.0))
                .child(match page {
                    Page::Hello => "Get started",
                    Page::Ready => "Start using Templum",
                    _ => "Next",
                })
                .when(!last, |el| {
                    el.child(Icon::new(IconName::ArrowRight).size(px(14.0)))
                })
                .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.welcome_next(window, cx)))
                .test_support(),
        );

    let card = div()
        .id("welcome")
        .absolute()
        .left(px((w - card_w) / 2.0))
        // It rises into place as it fades in.
        .top(px((h - card_h) / 2.0 + (1.0 - shown) * 24.0))
        .w(px(card_w))
        .h(px(card_h))
        .opacity(shown)
        .flex()
        .flex_col()
        .bg(ui.surface)
        .text_color(fg)
        .rounded(px(14.0))
        .border_1()
        .border_color(ui.hairline)
        .shadow(vec![BoxShadow {
            color: ui.shadow,
            offset: point(px(0.0), px(16.0)),
            blur_radius: px(48.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .track_focus(&welcome.focus)
        .on_key_down(cx.listener(Workspace::on_welcome_key))
        // Fading out, the card still takes its presses: a second click on Skip isn't the canvas's.
        .occlude()
        .child(steps)
        .child(content)
        .child(footer)
        .test_support();

    div()
        .id("welcome-backdrop")
        .absolute()
        .top(TITLE_BAR_HEIGHT)
        .left_0()
        .right_0()
        .bottom_0()
        // A modal: nothing under it gets the pointer, and a press beside the card closes nothing.
        // Fading out, it lets the canvas have it at once.
        .when(!welcome.closing, |el| el.occlude())
        .bg(ui.canvas.opacity(shown))
        .children(frost(ws, shown, w, h))
        .child(card)
        .into_any_element()
}

/// The canvas as if seen through frosted glass. GPUI can't blur what is drawn behind an element,
/// but a shadow is a true Gaussian blur of its shape: each block on screen becomes a soft shape of
/// its card's color, and three glows in the theme's colors keep an empty canvas from looking bare.
fn frost(ws: &Workspace, shown: f32, w: f32, h: f32) -> Vec<AnyElement> {
    let space = &ws.space;
    let p = ws.theme.palette;
    let left = if ws.sidebar { sidebar::WIDTH } else { 0.0 };
    let radius = 10.0 * space.view.zoom();
    // One soft shape per color, on top of each other.
    let blob = |x: f32, y: f32, bw: f32, bh: f32, r: f32, colors: &[Hsla], blur: f32| {
        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(bw))
            .h(px(bh))
            .rounded(px(r))
            .shadow(
                colors
                    .iter()
                    .map(|&color| BoxShadow {
                        color: color.opacity(color.a * shown),
                        offset: point(px(0.0), px(0.0)),
                        blur_radius: px(blur),
                        spread_radius: px(0.0),
                        inset: false,
                    })
                    .collect(),
            )
            .into_any_element()
    };
    let strength = if ws.theme.is_dark { 0.30 } else { 0.34 };
    let size = 0.5 * w.min(h);
    let glows = [(0.2, 0.25, 4), (0.8, 0.3, 5), (0.55, 0.85, 6)].map(|(x, y, ansi)| {
        blob(
            x * w - size / 2.0,
            y * h - size / 2.0,
            size,
            size,
            size / 2.0,
            &[hsla(p.ansi[ansi], strength)],
            size / 2.5,
        )
    });
    // A block is a shade lighter than the canvas: frosted, it needs a little more to show.
    let tint = if ws.theme.is_dark {
        vec![ws.ui.surface, hsla(p.foreground, 0.08)]
    } else {
        vec![gpui_kit::white()]
    };
    let blocks = space
        .canvas
        .blocks()
        .iter()
        .filter(|b| space.shown(b.id))
        .map(|b| space.view.rect_to_screen(b.rect))
        .filter(|r| r.x + left < w && r.x + left + r.w > 0.0 && r.y < h && r.y + r.h > 0.0)
        .map(|r| blob(left + r.x, r.y, r.w, r.h, radius, &tint, 36.0));
    // The glows over the blocks tint them, as light through the glass.
    blocks.chain(glows).collect()
}

/// One page in the row at the top: its number and name, the current one stronger. A click shows it.
fn step(ui: Ui, p: Page, current: Page, cx: &mut Context<Workspace>) -> AnyElement {
    let here = p == current;
    let done = (p as usize) < (current as usize);
    div()
        .id(ElementId::Name(
            format!("welcome-step-{}", p as usize).into(),
        ))
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(10.0))
        .py(px(4.0))
        .rounded_full()
        .text_size(px(12.0))
        .cursor_pointer()
        .map(|el| {
            if here {
                el.bg(ui.hover).font_weight(FontWeight::MEDIUM)
            } else {
                el.text_color(ui.title_dim).hover(move |s| s.bg(ui.hover))
            }
        })
        .active(move |s| s.bg(ui.pressed))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(18.0))
                .rounded_full()
                .text_size(px(10.0))
                .map(|el| {
                    if here || done {
                        el.bg(ui.accent).text_color(ui.surface)
                    } else {
                        el.border_1().border_color(ui.hairline)
                    }
                })
                .map(|el| {
                    if done {
                        el.child(Icon::new(IconName::Check).size(px(11.0)))
                    } else {
                        el.child((p as usize + 1).to_string())
                    }
                }),
        )
        .child(p.title())
        .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.welcome_page(p, cx)))
        .test_support()
        .into_any_element()
}

/// A footer button: a pill, filled with the accent for the main one.
fn pill(id: &'static str, ui: Ui, main: bool) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .px(px(14.0))
        .py(px(6.0))
        .rounded_full()
        .text_size(px(13.0))
        .cursor_pointer()
        .map(|el| {
            if main {
                el.bg(ui.accent)
                    .font_weight(FontWeight::MEDIUM)
                    .hover(move |s| s.bg(ui.accent.opacity(0.85)))
                    .active(move |s| s.bg(ui.accent.opacity(0.7)))
            } else {
                el.hover(move |s| s.bg(ui.hover))
                    .active(move |s| s.bg(ui.pressed))
            }
        })
}

/// A page's title and what it is about.
fn heading(ui: Ui, fg: Hsla, title: &'static str, about: &'static str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .text_size(px(22.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .child(title),
        )
        .child(div().text_size(px(13.0)).text_color(ui.title).child(about))
        .into_any_element()
}

/// The first page: what Templum is, in three cards.
fn hello(ws: &Workspace) -> AnyElement {
    let ui = ws.ui;
    let fg = hsla(ws.theme.palette.foreground, 1.0);
    let feature = |icon: IconName, title: &'static str, about: &'static str| {
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(16.0))
            .rounded(px(10.0))
            .border_1()
            .border_color(ui.hairline)
            .bg(ui.card)
            .child(Icon::new(icon).size(px(20.0)).text_color(ui.accent))
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(ui.title_dim)
                    .child(about),
            )
    };
    let settings_key = ws.keymap.keystrokes(Command::Settings).into_iter().next();
    div()
        .flex()
        .flex_col()
        .gap(px(20.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(28.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg)
                        .child("Welcome to Templum"),
                )
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(ui.title)
                        .child("An infinite canvas for your terminals and your AI agents."),
                ),
        )
        .child(
            div()
                .grid()
                .grid_cols(3)
                .gap(px(12.0))
                .child(feature(
                    IconName::SquareTerminal,
                    "Terminals on a canvas",
                    "Place them anywhere, pan and zoom around them, or tile them full screen.",
                ))
                .child(feature(
                    IconName::Bot,
                    "A cockpit for agents",
                    "Claude Code, Codex and more: see which one waits for you, and your plan's \
                     limits.",
                ))
                .child(feature(
                    IconName::LayoutDashboard,
                    "Notes, kanban, Git",
                    "Notes, boards, editors and your repository's changes beside your terminals.",
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(12.0))
                .text_color(ui.title_dim)
                .child("A minute to set up. Everything here is also in Settings")
                .children(settings_key.map(|k| settings::chip(Kbd::new(k), ui))),
        )
        .into_any_element()
}

/// The last page's keys, from the keymap in effect.
fn good_to_know(ws: &Workspace) -> AnyElement {
    let ui = ws.ui;
    let pill = move |text: &'static str| settings::chip(div(), ui).child(text).into_any_element();
    let key = |command: Command| match ws.keymap.keystrokes(command).into_iter().next() {
        Some(k) => settings::chip(Kbd::new(k), ui).into_any_element(),
        None => pill("No shortcut"),
    };
    let rows: Vec<(AnyElement, &'static str)> = vec![
        (key(Command::NewTerminal), "A new terminal"),
        (
            pill(if cfg!(target_os = "macos") {
                "Hold Option"
            } else {
                "Hold Alt"
            }),
            "Move the canvas over blocks",
        ),
        (
            pill("Right-click"),
            "On the canvas: terminals, AI agents, notes, kanban",
        ),
        (key(Command::GoToWaiting), "The terminal waiting for you"),
        (key(Command::Workspaces), "Home: your workspaces"),
        (
            key(Command::ToggleGrid),
            "Grid mode: terminals tiled full screen",
        ),
        (key(Command::FitAll), "Every block in view"),
        (key(Command::Settings), "Settings"),
    ];
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Good to know"),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(10.0))
                .border_1()
                .border_color(ui.hairline)
                .bg(ui.card)
                .children(rows.into_iter().enumerate().map(|(i, (keys, what))| {
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .px(px(14.0))
                        .py(px(9.0))
                        .when(i > 0, |el| el.border_t_1().border_color(ui.hairline))
                        .child(div().text_size(px(13.0)).child(what))
                        .child(keys)
                })),
        )
        .into_any_element()
}
