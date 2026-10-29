//! Sharing a workspace with another Templum: the Share dialog, opened from the workspace's menu
//! (docs/specs/templum-to-templum.md, On home: sharing a workspace), a dialog of the popups' kind
//! (docs/specs/design-system.md, Popups). It shows the workspace's shares, each with Revoke; New
//! share asks for a share's fields, and Make code shows its code once.

use std::net::IpAddr;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Div, ElementId, Entity, FocusHandle,
    FontWeight, SharedString, Stateful, TestSupportExt, Window, div, px,
};
use templum_api::model::ShareView;
use templum_api::store::{Scope, Settings, Use, Way, share_problem};

use crate::grid::hsla;
use crate::settings::api::{expiry_words, used_words};
use crate::settings::row::menu_button;
use crate::theme::{Theme, Ui};
use crate::workspace::api::ShareDraft;
use crate::workspace::{Workspace, show_notification};
use crate::{balloon, popup};

/// What the Share dialog says when nothing is shared.
pub(crate) const INVITE: &str = "Let another Templum mirror this workspace: watch it live and work \
    with its blocks, cards, checklists, Git, automations and agents from there.";
const FULL_SAYS: &str = "Also type and click into terminals, discard Git changes, delete branches, \
    stashes and automations. Anyone with the code can run commands on this computer.";
const CONTROL_SAYS: &str = "Watch and work with blocks, cards, checklists, Git and automations, \
    open agents and hand them tasks, answer requests. No typing into terminals; agents it opens \
    can run commands.";
const PASTE_IT: &str = "Paste it in the other Templum: sidebar ▸ Connect to a Templum… Templum \
    keeps only a fingerprint of its key: this code can't be shown again.";
/// What a share is called until the person says.
pub(crate) const DEFAULT_NAME: &str = "Another computer";
/// The days a share lasts, by default.
const DEFAULT_DAYS: u64 = 30;
/// What a dialog's words are set in; what is said about them.
const TEXT: f32 = 13.0;
const SMALL: f32 = 11.5;
/// The body's and the foot's sides.
const SIDE: f32 = 16.0;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A share's row in the Share dialog, under its name: "Full · expires in 29 days", and
/// "mirrored now from 203.0.113.7" (`from`: the address of the stream, not of the last call),
/// "last used 3 h ago from 203.0.113.7" or "never used".
pub(crate) fn sharing_words(share: &ShareView, from: &str, now: u64) -> (String, String) {
    let first = format!(
        "{} · {}",
        share.scope.word(),
        expiry_words(share.expires, now)
    );
    let last = share.last.as_ref().map(|last| Use {
        at: last.at,
        addr: last.addr.clone(),
    });
    let second = match (from, share.mirrored) {
        ("", true) => "mirrored now".to_string(),
        (from, true) => format!("mirrored now from {from}"),
        (_, false) => used_words(last.as_ref(), now),
    };
    (first, second)
}

/// The small cast mark of a workspace that is mirrored now, with its tooltip: "Mirrored now on
/// Another computer (203.0.113.7)", a line per mirror. None when nobody mirrors it. Its element id
/// is `id` and the workspace's; the caller places it.
pub(crate) fn mirror_mark(
    ws: &Workspace,
    space: &str,
    id: &'static str,
    size: f32,
) -> Option<impl IntoElement + Styled> {
    let mirrors = ws.mirrored_now(space);
    if mirrors.is_empty() {
        return None;
    }
    let tip: String = mirrors
        .iter()
        .map(|(name, addr)| match addr.is_empty() {
            true => format!("Mirrored now on {name}"),
            false => format!("Mirrored now on {name} ({addr})"),
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(
        div()
            .id(ElementId::Name(format!("{id}-{space}").into()))
            .flex()
            .items_center()
            .justify_center()
            .child(
                Icon::new(IconName::Cast)
                    .size(px(size))
                    .text_color(ws.ui.accent),
            )
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .test_support(),
    )
}

/// This computer's address on its main network: the one a UDP socket would send from toward the
/// internet (connecting one sends nothing). None without a network or on a loopback answer.
pub(crate) fn this_address() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// `address` and `port` as a host and port: `192.168.1.5:7411`, `[fe80::1]:7411`.
fn host_port(address: IpAddr, port: u16) -> String {
    match address {
        IpAddr::V4(v4) => format!("{v4}:{port}"),
        IpAddr::V6(v6) => format!("[{v6}]:{port}"),
    }
}

/// What keeps a share from working, and the one button that may fix it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Problem {
    /// Said before and after a button: "Sharing needs the API. [Turn it on] (this computer only…)".
    Fix {
        before: String,
        button: &'static str,
        after: String,
    },
    /// Said, with nothing to press.
    Text(String),
}

/// What is wrong with sharing as `way` to `address` given the API's `settings`; None when it fits.
pub(crate) fn problem(settings: &Settings, way: Way, address: &str) -> Option<Problem> {
    if !settings.enabled {
        return Some(Problem::Fix {
            before: "Sharing needs the API.".into(),
            button: "Turn it on",
            after: match way {
                Way::Public => format!("(this computer only, port {}).", settings.port),
                Way::Pinned => "for every network, with Templum's certificate".into(),
            },
        });
    }
    match (way, share_problem(settings, way, address)) {
        (Way::Pinned, Some(_)) => Some(Problem::Fix {
            before: "Directly needs the API on every network with Templum's certificate.".into(),
            button: "Apply",
            after: String::new(),
        }),
        (Way::Pinned, None) => {
            let host = address.trim();
            (host.is_empty() || host.contains("://"))
                .then(|| Problem::Text("Give Directly's address as host:port.".into()))
        }
        (Way::Public, Some(sentence)) => Some(Problem::Text(sentence.into())),
        (Way::Public, None) => None,
    }
}

/// The options of a menu button, by value.
fn menu_options(pairs: &[(&'static str, &'static str)]) -> Vec<(SharedString, SharedString)> {
    pairs
        .iter()
        .map(|(value, label)| ((*value).into(), (*label).into()))
        .collect()
}

/// What choosing an option of a menu does to the dialog's state.
type Apply = Rc<dyn Fn(&mut ShareFlow, &str)>;

/// How long a share lasts, in days; None: never.
type Days = Option<u64>;

/// What the header says of the workspace's shares: "Not shared yet", "Shared with 2 computers ·
/// 1 mirroring now".
fn shares_words(shares: usize, mirrored: usize) -> String {
    let with = match shares {
        0 => return "Not shared yet".into(),
        1 => "Shared with 1 computer".to_string(),
        n => format!("Shared with {n} computers"),
    };
    match mirrored {
        0 => with,
        n => format!("{with} · {n} mirroring now"),
    }
}

/// Where the dialog is.
enum Step {
    /// The workspace's shares, each with Revoke: where it opens.
    Shares,
    /// A new share's fields, and Make code.
    New,
    /// The code just made, shown once. Nothing else holds it; leaving this step drops it.
    Code(String),
}

/// What a foot's button does.
type Act = fn(&mut ShareFlow, &mut Window, &mut Context<ShareFlow>);
/// A foot's button: its id, its words and what it does.
type Press = (&'static str, &'static str, Act);

/// The Share dialog's state: its step, a new share's fields, and the code once it is made.
pub(crate) struct ShareFlow {
    workspace: Entity<Workspace>,
    space: String,
    step: Step,
    name: Entity<InputState>,
    /// The address of the way "through a tunnel", and of "directly".
    tunnel: Entity<InputState>,
    direct: Entity<InputState>,
    scope: Scope,
    days: Days,
    way: Way,
    /// Why Make code was refused.
    error: Option<String>,
    /// The keyboard's place in the steps without a field, so Enter and Esc still reach the dialog.
    focus: FocusHandle,
    theme: Rc<Theme>,
    ui: Ui,
    mono: SharedString,
}

impl ShareFlow {
    /// The connection code, while it is shown; None before it is made and after it is left.
    #[cfg(test)]
    pub(crate) fn code(&self) -> Option<&str> {
        match &self.step {
            Step::Code(code) => Some(code),
            _ => None,
        }
    }

    fn address(&self, cx: &App) -> String {
        match self.way {
            Way::Public => self.tunnel.read(cx).value().to_string(),
            Way::Pinned => self.direct.read(cx).value().to_string(),
        }
    }

    /// What stands in the way of a share now: the API's fit, or a server that isn't listening.
    fn blocked(&self, cx: &App) -> Option<Problem> {
        let ws = self.workspace.read(cx);
        let Some(store) = ws.api.store.as_ref() else {
            return Some(Problem::Text("There is no data folder.".into()));
        };
        problem(&store.settings, self.way, &self.address(cx)).or_else(|| {
            // The settings fit, but the server may not have started (the port is taken).
            (ws.api.server.is_none() || ws.api.error.is_some()).then(|| {
                Problem::Text(
                    ws.api
                        .error
                        .clone()
                        .unwrap_or_else(|| "The API is not listening.".into()),
                )
            })
        })
    }

    /// Make code can act: nothing blocks it and the share has a name.
    fn ready(&self, cx: &App) -> bool {
        self.blocked(cx).is_none() && !self.name.read(cx).value().trim().is_empty()
    }

    /// New share: the fields, the name's text selected.
    fn new_share(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.step = Step::New;
        self.error = None;
        self.name.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    /// Back to the shares, from the fields, or from the code (Done), which leaves memory.
    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.step = Step::Shares;
        self.error = None;
        // The field that had the keyboard is gone.
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn make(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        let draft = ShareDraft {
            workspace: self.space.clone(),
            name: self.name.read(cx).value().to_string(),
            scope: self.scope,
            expires: self.days.map(|days| now() + days * 86_400),
            way: self.way,
            address: self.address(cx),
        };
        let made = self
            .workspace
            .update(cx, |ws, cx| ws.share_workspace(draft, cx));
        match made {
            Ok((_, code)) => {
                self.step = Step::Code(code);
                self.error = None;
                // The next share starts from the default name.
                self.name
                    .update(cx, |input, cx| input.set_value(DEFAULT_NAME, window, cx));
                self.focus.focus(window, cx);
            }
            Err(why) => self.error = Some(why),
        }
        cx.notify();
    }

    fn copy(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Step::Code(code) = &self.step {
            cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
        }
    }

    /// Close, the header's and the foot's: the code leaves memory with the dialog.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.step = Step::Shares;
        window.close_dialog(cx);
    }

    /// Enter: the step's solid button.
    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.step {
            Step::Shares => self.new_share(window, cx),
            Step::New => self.make(window, cx),
            Step::Code(_) => self.back(window, cx),
        }
    }

    /// A dim line of help under a field.
    fn help(&self, id: &'static str, text: impl Into<SharedString>) -> impl IntoElement {
        div()
            .id(id)
            .w_full()
            .text_size(px(SMALL))
            .text_color(self.ui.title_dim)
            .child(text.into())
            .test_support()
    }

    /// The shares, a row each with Revoke, which applies at once; with none, what sharing does.
    fn shares(&self, ws: &Workspace, rows: Vec<ShareView>) -> Div {
        let ui = self.ui;
        let body = popup::content().px(px(SIDE)).py(px(16.0));
        if rows.is_empty() {
            return body.child(
                div()
                    .id("share-invite")
                    .p(px(12.0))
                    .rounded(px(8.0))
                    .bg(ui.card)
                    .text_size(px(12.0))
                    .text_color(ui.title_dim)
                    .child(INVITE)
                    .test_support(),
            );
        }
        let at = now();
        let yellow = hsla(self.theme.palette.ansi[3], 1.0);
        let list = rows.into_iter().enumerate().map(|(n, row)| {
            let from = ws.mirrored_from(&row.id).unwrap_or_default();
            let (first, second) = sharing_words(&row, &from, at);
            let expired = row.expires.is_some_and(|end| end <= at);
            let token = row.id[1..].parse::<u64>().unwrap_or(0);
            let id = |part: &str| ElementId::Name(format!("share-{part}-{}", row.id).into());
            let workspace = self.workspace.clone();
            div()
                .id(id("row"))
                .flex()
                .items_center()
                .gap(px(12.0))
                .py(px(10.0))
                .pl(px(12.0))
                .pr(px(8.0))
                .when(n > 0, |el| el.border_t_1().border_color(ui.hairline))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(row.name.clone()),
                        )
                        .child(
                            div()
                                .id(id("first"))
                                .truncate()
                                .text_size(px(SMALL))
                                .text_color(if expired { yellow } else { ui.title_dim })
                                .child(first)
                                .test_support(),
                        )
                        .child(
                            div()
                                .id(id("last"))
                                .flex()
                                .items_center()
                                .gap(px(5.0))
                                .text_size(px(SMALL))
                                .text_color(ui.title_dim)
                                .when(row.mirrored, |el| {
                                    el.child(
                                        div()
                                            .flex_shrink_0()
                                            .size(px(6.0))
                                            .rounded_full()
                                            .bg(ui.accent),
                                    )
                                })
                                .child(div().min_w(px(0.0)).truncate().child(second))
                                .test_support(),
                        ),
                )
                .child(
                    balloon::ghost(id("revoke"), "Revoke", ui, 1.0)
                        .flex_shrink_0()
                        .h(px(popup::FIELD_H))
                        .px(px(10.0))
                        .on_click(move |_: &ClickEvent, _, cx| {
                            workspace.update(cx, |ws, cx| ws.revoke_token(token, cx))
                        })
                        .test_support(),
                )
                .test_support()
        });
        body.child(
            div()
                .flex()
                .flex_col()
                .rounded(px(8.0))
                .bg(ui.card)
                .children(list),
        )
    }

    /// A new share's fields: who it is for, what it can do and for how long; then how the other
    /// computer reaches this one, and what the API needs for it.
    fn fields(&self, ws: &Workspace, cx: &Context<Self>) -> Div {
        let (days, scope, way) = (self.days, self.scope, self.way);
        let this = cx.entity();
        let pick = |apply: Apply| {
            let this = this.clone();
            move |value: SharedString, cx: &mut App| {
                this.update(cx, |flow, cx| {
                    apply(flow, value.as_ref());
                    cx.notify();
                })
            }
        };
        let days_key: SharedString = days.map_or("never".into(), |d| d.to_string().into());
        let red = hsla(self.theme.palette.ansi[1], 1.0);
        let problem = self.blocked(cx).map(|blocked| match blocked {
            Problem::Text(text) => div()
                .id("share-problem")
                .w_full()
                .text_size(px(SMALL))
                .text_color(red)
                .child(text)
                .test_support()
                .into_any_element(),
            Problem::Fix {
                before,
                button,
                after,
            } => {
                let workspace = self.workspace.clone();
                popup::line()
                    .id("share-problem")
                    .gap(px(6.0))
                    .text_size(px(SMALL))
                    .text_color(self.ui.title)
                    .child(before)
                    .child(Button::new("share-fix").label(button).xsmall().on_click(
                        move |_, _, cx| workspace.update(cx, |ws, cx| ws.fit_api(way, cx)),
                    ))
                    .when(!after.is_empty(), |el| el.child(after))
                    .test_support()
                    .into_any_element()
            }
        });
        let under = |field: AnyElement| div().flex().flex_col().gap(px(4.0)).child(field);
        let address = match way {
            Way::Public => &self.tunnel,
            Way::Pinned => &self.direct,
        };
        popup::content()
            .px(px(SIDE))
            .py(px(16.0))
            .gap(px(18.0))
            .child(
                popup::section(ws, "New share")
                    .child(popup::row(ws, "Name", Input::new(&self.name).small()))
                    .child(popup::row(
                        ws,
                        "Can",
                        under(
                            popup::line()
                                .child(menu_button(
                                    "share-scope",
                                    menu_options(&[("full", "Full"), ("control", "Control")]),
                                    if scope == Scope::Full {
                                        "full"
                                    } else {
                                        "control"
                                    }
                                    .into(),
                                    false,
                                    pick(Rc::new(|flow, value| {
                                        flow.scope = if value == "full" {
                                            Scope::Full
                                        } else {
                                            Scope::Control
                                        }
                                    })),
                                ))
                                .into_any_element(),
                        )
                        .child(self.help(
                            "share-scope-says",
                            if scope == Scope::Full {
                                FULL_SAYS
                            } else {
                                CONTROL_SAYS
                            },
                        )),
                    ))
                    .child(popup::row(
                        ws,
                        "Expires",
                        popup::line().child(menu_button(
                            "share-expires",
                            menu_options(&[
                                ("1", "In 1 day"),
                                ("7", "In 7 days"),
                                ("30", "In 30 days"),
                                ("90", "In 90 days"),
                                ("365", "In 1 year"),
                                ("never", "Never"),
                            ]),
                            days_key,
                            false,
                            pick(Rc::new(|flow, value| {
                                flow.days = value.parse().ok();
                            })),
                        )),
                    )),
            )
            .child(
                popup::section(ws, "Connection")
                    .child(popup::row(
                        ws,
                        "Way in",
                        popup::line().child(menu_button(
                            "share-way",
                            menu_options(&[
                                ("public", "Through a tunnel or proxy"),
                                ("pinned", "Directly"),
                            ]),
                            if way == Way::Public {
                                "public"
                            } else {
                                "pinned"
                            }
                            .into(),
                            false,
                            pick(Rc::new(|flow, value| {
                                flow.way = if value == "pinned" {
                                    Way::Pinned
                                } else {
                                    Way::Public
                                }
                            })),
                        )),
                    ))
                    .child(popup::row(
                        ws,
                        "Address",
                        under(Input::new(address).small().into_any_element()).children(problem),
                    )),
            )
    }

    /// The code, whole, in the terminal's font on a card, and what to do with it.
    fn shown(&self, ws: &Workspace, code: &str) -> Div {
        popup::content().px(px(SIDE)).py(px(16.0)).child(
            popup::section(ws, "Connection code").child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .id("share-code")
                            .w_full()
                            .overflow_hidden()
                            .p(px(12.0))
                            .rounded(px(8.0))
                            .bg(self.ui.card)
                            .font_family(self.mono.clone())
                            .text_size(px(12.0))
                            .child(code.to_string())
                            .test_support(),
                    )
                    .child(self.help("share-paste-it", PASTE_IT)),
            ),
        )
    }

    /// The foot: why Make code was refused, then the step's ghost and solid buttons.
    fn foot(&self, ws: &Workspace, cx: &Context<Self>) -> Div {
        let ui = self.ui;
        let red = hsla(self.theme.palette.ansi[1], 1.0);
        let (ghost, solid, ready): (Press, Press, bool) = match self.step {
            Step::Shares => (
                ("share-dismiss", "Close", ShareFlow::close),
                ("share-new", "New share", ShareFlow::new_share),
                true,
            ),
            Step::New => (
                ("share-back", "Back", ShareFlow::back),
                ("share-make", "Make code", ShareFlow::make),
                self.ready(cx),
            ),
            Step::Code(_) => (
                ("share-copy", "Copy", ShareFlow::copy),
                ("share-done", "Done", ShareFlow::back),
                true,
            ),
        };
        let does = |button: Stateful<Div>, act: Act| {
            let this = cx.entity();
            button
                .on_click(move |_, window, cx| this.update(cx, |flow, cx| act(flow, window, cx)))
                .test_support()
        };
        let error = self
            .error
            .clone()
            .filter(|_| matches!(self.step, Step::New));
        popup::foot(ws).child(
            popup::content()
                .px(px(SIDE))
                .gap(px(6.0))
                .children(error.map(|why| {
                    div()
                        .id("share-error")
                        .w_full()
                        .text_size(px(SMALL))
                        .text_color(red)
                        .child(why)
                        .test_support()
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(div().flex_1())
                        .child(does(balloon::ghost(ghost.0, ghost.1, ui, 1.0), ghost.2))
                        .child(does(
                            balloon::solid(solid.0, solid.1, None, ui, 1.0)
                                .when(!ready, |el| el.opacity(0.4).cursor_default()),
                            solid.2,
                        )),
                ),
        )
    }
}

impl Render for ShareFlow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.clone();
        let ws = workspace.read(cx);
        let rows = ws.share_rows(&self.space);
        let said = shares_words(rows.len(), rows.iter().filter(|row| row.mirrored).count());
        let name = ws
            .spaces()
            .find(|s| s.id == self.space)
            .map_or_else(String::new, |s| s.name.clone());
        let body = match &self.step {
            Step::Shares => self.shares(ws, rows),
            Step::New => self.fields(ws, cx),
            Step::Code(code) => self.shown(ws, code),
        };
        let this = cx.entity();
        popup::dialog_frame("share-dialog", ws)
            .track_focus(&self.focus)
            .text_size(px(TEXT))
            .child(popup::dialog_header(
                ws,
                "share",
                popup::tile(IconName::Cast, self.ui.accent),
                format!("Share {name}"),
                Some(popup::subtitle(ws, said)),
                move |_: &ClickEvent, window, cx| {
                    this.update(cx, |flow, cx| flow.close(window, cx))
                },
            ))
            .child(body)
            .child(self.foot(ws, cx))
            .test_support()
    }
}

/// The Share dialog of the workspace `space`, at its shares. None, with a notification, when
/// there is no data folder to keep a share in.
pub(crate) fn open(
    ws: &Workspace,
    space: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Option<Entity<ShareFlow>> {
    let Some(store) = ws.api.store.as_ref() else {
        show_notification(
            window,
            cx,
            gpui_kit::component::notification::Notification::warning("There is no data folder."),
        );
        return None;
    };
    let settings = &store.settings;
    // Templum's certificate on a network other than this computer picks Directly.
    let way = if settings.tls == templum_api::store::Tls::Own
        && settings.listen != templum_api::store::Listen::Loopback
    {
        Way::Pinned
    } else {
        Way::Public
    };
    let tunnel_default = store.tunnel.clone().unwrap_or_default();
    let direct_default = this_address()
        .map(|ip| host_port(ip, settings.port))
        .unwrap_or_default();
    let workspace = cx.entity();
    let name = cx.new(|cx| InputState::new(window, cx).default_value(DEFAULT_NAME));
    let tunnel = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("https://templum.example.com")
            .default_value(tunnel_default)
    });
    let direct = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("192.168.1.5:7411")
            .default_value(direct_default)
    });
    let view = cx.new(|cx| {
        // The shares, who mirrors, the fit of the API and the fields' text change what it says.
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        ShareFlow {
            workspace: workspace.clone(),
            space: space.to_string(),
            step: Step::Shares,
            name: name.clone(),
            tunnel: tunnel.clone(),
            direct: direct.clone(),
            scope: Scope::Full,
            days: Some(DEFAULT_DAYS),
            way,
            error: None,
            focus: cx.focus_handle(),
            theme: ws.theme.clone(),
            ui: ws.ui,
            mono: ws.cell.font.family.clone(),
        }
    });
    for field in [&name, &tunnel, &direct] {
        let view = view.downgrade();
        App::subscribe(cx, field, move |_, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                view.update(cx, |_, cx| cx.notify()).ok();
            }
        })
        .detach();
    }
    let shown = view.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let (enter, closed) = (shown.clone(), shown.clone());
        popup::dialog(dialog, shown.clone())
            // Enter does the step's solid button; only Close, Esc and a press outside close it.
            .on_ok(move |_, window, cx| {
                enter.update(cx, |flow, cx| flow.enter(window, cx));
                false
            })
            .on_close(move |_, _, cx| closed.update(cx, |flow, _| flow.step = Step::Shares))
    });
    Some(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use templum_api::store::{Listen, Tls};

    fn settings(enabled: bool, listen: Listen, tls: Tls) -> Settings {
        Settings {
            enabled,
            listen,
            tls,
            ..Settings::default()
        }
    }

    #[test]
    fn the_dialog_says_what_the_api_needs_and_how_to_fix_it() {
        let off = settings(false, Listen::Loopback, Tls::Off);
        assert_eq!(
            problem(&off, Way::Public, "https://t.example.com"),
            Some(Problem::Fix {
                before: "Sharing needs the API.".into(),
                button: "Turn it on",
                after: "(this computer only, port 7411).".into()
            })
        );
        assert_eq!(
            problem(&off, Way::Pinned, "192.168.1.5:7411"),
            Some(Problem::Fix {
                before: "Sharing needs the API.".into(),
                button: "Turn it on",
                after: "for every network, with Templum's certificate".into()
            })
        );
        let local = settings(true, Listen::Loopback, Tls::Off);
        let apply = Some(Problem::Fix {
            before: "Directly needs the API on every network with Templum's certificate.".into(),
            button: "Apply",
            after: String::new(),
        });
        assert_eq!(problem(&local, Way::Pinned, "192.168.1.5:7411"), apply);
        assert_eq!(
            problem(&settings(true, Listen::All, Tls::Off), Way::Pinned, "x:1"),
            apply
        );
        assert_eq!(problem(&local, Way::Public, "https://t.example.com"), None);
        assert_eq!(
            problem(&local, Way::Public, "templum.example.com"),
            Some(Problem::Text(
                "The address must start with https://.".into()
            ))
        );
        let direct = settings(true, Listen::All, Tls::Own);
        assert_eq!(problem(&direct, Way::Pinned, "192.168.1.5:7411"), None);
        for bad in ["", "  ", "https://192.168.1.5:7411"] {
            assert_eq!(
                problem(&direct, Way::Pinned, bad),
                Some(Problem::Text(
                    "Give Directly's address as host:port.".into()
                )),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn the_header_counts_the_computers_shared_with_and_those_mirroring() {
        assert_eq!(shares_words(0, 0), "Not shared yet");
        assert_eq!(shares_words(1, 0), "Shared with 1 computer");
        assert_eq!(
            shares_words(2, 1),
            "Shared with 2 computers · 1 mirroring now"
        );
    }

    #[test]
    fn an_address_is_a_host_and_a_port() {
        assert_eq!(
            host_port("192.168.1.5".parse().unwrap(), 7411),
            "192.168.1.5:7411"
        );
        assert_eq!(
            host_port("fe80::1".parse().unwrap(), 7411),
            "[fe80::1]:7411"
        );
    }
}
