//! Sharing a workspace with another Templum: the Share… dialog, and the words of a share's row
//! in the Edit… dialog (docs/specs/templum-to-templum.md, On home: sharing a workspace).

use std::net::IpAddr;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Disableable as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClipboardItem, Context, ElementId, Entity, FontWeight, SharedString, TestSupportExt,
    Window, div, px,
};
use templum_api::model::ShareView;
use templum_api::store::{Scope, Settings, Use, Way, share_problem};

use crate::grid::hsla;
use crate::settings::api::{expiry_words, used_words};
use crate::settings::row::menu_button;
use crate::theme::{Theme, Ui};
use crate::workspace::api::ShareDraft;
use crate::workspace::{Workspace, show_notification};

/// What the Edit… dialog says under Sharing when nothing is shared.
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

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A share's row in the Edit… dialog, under its name: "Full · expires in 29 days", and
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

/// The Share… dialog's state: its fields, and the code once it is made.
pub(crate) struct ShareFlow {
    workspace: Entity<Workspace>,
    space: String,
    name: Entity<InputState>,
    /// The address of the way "through a tunnel", and of "directly".
    tunnel: Entity<InputState>,
    direct: Entity<InputState>,
    scope: Scope,
    days: Days,
    way: Way,
    /// Why Make code was refused.
    error: Option<String>,
    /// The connection code, shown once. Nothing else holds it; Done drops it.
    code: Option<String>,
    theme: Rc<Theme>,
    ui: Ui,
    mono: SharedString,
}

impl ShareFlow {
    /// The connection code, while it is shown; None before it is made and after Done.
    #[cfg(test)]
    pub(crate) fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    fn address(&self, cx: &App) -> String {
        match self.way {
            Way::Public => self.tunnel.read(cx).value().to_string(),
            Way::Pinned => self.direct.read(cx).value().to_string(),
        }
    }

    /// What stands in the way of Make code now: the API's fit, or an empty name.
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

    fn make(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().to_string();
        let draft = ShareDraft {
            workspace: self.space.clone(),
            name,
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
                self.code = Some(code);
                self.error = None;
            }
            Err(why) => self.error = Some(why),
        }
        cx.notify();
    }

    fn label(&self, text: &'static str) -> impl IntoElement {
        div()
            .mt(px(4.0))
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(self.ui.title)
            .child(text)
    }

    fn dim(&self, id: &'static str, text: &'static str) -> impl IntoElement {
        div()
            .id(id)
            .text_size(px(12.0))
            .text_color(self.ui.title_dim)
            .child(text)
            .test_support()
    }

    fn fields(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
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
        let blocked = self.blocked(cx);
        let red = hsla(self.theme.palette.ansi[1], 1.0);
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(self.label("Name"))
            .child(Input::new(&self.name))
            .child(self.label("Can"))
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
            .child(if scope == Scope::Full {
                self.dim("share-scope-says", FULL_SAYS)
            } else {
                self.dim("share-scope-says", CONTROL_SAYS)
            })
            .child(self.label("Expires"))
            .child(menu_button(
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
            ))
            .child(self.label("The other computer reaches this one"))
            .child(menu_button(
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
            ))
            .child(self.label("Address"))
            .child(Input::new(match way {
                Way::Public => &self.tunnel,
                Way::Pinned => &self.direct,
            }))
            .children(blocked.map(|blocked| {
                match blocked {
                    Problem::Text(text) => div()
                        .id("share-problem")
                        .text_size(px(12.0))
                        .text_color(red)
                        .child(text)
                        .test_support(),
                    Problem::Fix {
                        before,
                        button,
                        after,
                    } => {
                        let workspace = self.workspace.clone();
                        div()
                            .id("share-problem")
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.0))
                            .text_size(px(12.0))
                            .child(before)
                            .child(Button::new("share-fix").label(button).small().on_click(
                                move |_, _, cx| {
                                    workspace.update(cx, |ws, cx| ws.fit_api(way, cx));
                                },
                            ))
                            .when(!after.is_empty(), |el| el.child(after))
                            .test_support()
                    }
                }
            }))
            .children(self.error.clone().map(|why| {
                div()
                    .id("share-error")
                    .text_size(px(12.0))
                    .text_color(red)
                    .child(why)
                    .test_support()
            }))
    }

    /// The code, whole, in the terminal's font, wrapped by characters; and what to do with it.
    fn shown(&self, code: String) -> gpui_kit::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .id("share-code")
                    .w_full()
                    .overflow_hidden()
                    .p(px(8.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(self.ui.hairline)
                    .font_family(self.mono.clone())
                    .text_size(px(12.0))
                    .child(code)
                    .test_support(),
            )
            .child(
                div()
                    .id("share-paste-it")
                    .text_size(px(12.0))
                    .text_color(self.ui.title_dim)
                    .child(PASTE_IT)
                    .test_support(),
            )
    }
}

impl Render for ShareFlow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.code.clone() {
            Some(code) => self.shown(code),
            None => self.fields(cx),
        }
    }
}

/// Share… of the workspace `space`: its fields, Make code, then the code once.
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
        // The fit of the API and the fields' text change what the dialog says.
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        ShareFlow {
            workspace: workspace.clone(),
            space: space.to_string(),
            name: name.clone(),
            tunnel: tunnel.clone(),
            direct: direct.clone(),
            scope: Scope::Full,
            days: Some(DEFAULT_DAYS),
            way,
            error: None,
            code: None,
            theme: ws.theme.clone(),
            ui: ws.ui,
            mono: ws.cell.font.family.clone(),
        }
    });
    for field in [&name, &tunnel, &direct] {
        let view = view.downgrade();
        App::subscribe(
            cx,
            field,
            move |_, event: &gpui_kit::component::input::InputEvent, cx| {
                if let gpui_kit::component::input::InputEvent::Change = event {
                    view.update(cx, |_, cx| cx.notify()).ok();
                }
            },
        )
        .detach();
    }
    let (shown, opened) = (view.clone(), view);
    window.open_alert_dialog(cx, move |alert, _, cx| {
        let flow = shown.clone();
        let made = flow.read(cx).code.is_some();
        let blocked = flow.read(cx).blocked(cx).is_some()
            || flow.read(cx).name.read(cx).value().trim().is_empty();
        let footer = if made {
            let (copy, done) = (flow.clone(), flow.clone());
            div()
                .flex()
                .justify_end()
                .gap(px(8.0))
                .child(
                    Button::new("share-copy")
                        .label("Copy")
                        .on_click(move |_, _, cx| {
                            if let Some(code) = copy.read(cx).code.clone() {
                                cx.write_to_clipboard(ClipboardItem::new_string(code));
                            }
                        }),
                )
                .child(Button::new("share-done").label("Done").primary().on_click(
                    move |_, window, cx| {
                        // The text leaves memory before the dialog goes.
                        done.update(cx, |flow, _| flow.code = None);
                        window.close_dialog(cx);
                    },
                ))
        } else {
            let make = flow.clone();
            div()
                .flex()
                .justify_end()
                .gap(px(8.0))
                .child(
                    Button::new("share-cancel")
                        .label("Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    Button::new("share-make")
                        .label("Make code")
                        .primary()
                        .disabled(blocked)
                        .on_click(move |_, _, cx| make.update(cx, |flow, cx| flow.make(cx))),
                )
        };
        let flow_close = flow.clone();
        alert
            .confirm()
            .title(if made {
                "Your connection code"
            } else {
                "Share workspace"
            })
            .w(px(520.0))
            .footer(footer)
            .child(flow.clone())
            .on_close(move |_, _, cx| flow_close.update(cx, |flow, _| flow.code = None))
    });
    name.update(cx, |input, cx| {
        input.focus(window, cx);
        input.select_all(window, cx);
    });
    Some(opened)
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
