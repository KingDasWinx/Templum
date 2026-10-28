//! Connecting to another Templum: the Connect to a Templum… dialog
//! (docs/specs/templum-to-templum.md, On the mirror: connecting), a dialog of the popups' kind
//! (docs/specs/design-system.md, Popups). The code leaves the field the moment Connect is
//! pressed, whatever comes of it.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Icon, Sizable as _, Size as UiSize, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, SharedString, TestSupportExt, Window, div,
    px,
};
use templum_api::client::{proxy_from_env, proxy_of};
use templum_api::code::{self, Code, Key};

use crate::grid::hsla;
use crate::theme::{Theme, Ui};
use crate::workspace::Workspace;
use crate::workspace::remote::{Act, Connecting};
use crate::{balloon, popup};

/// What a dialog's words are set in; what is said about them.
const TEXT: f32 = 13.0;
const SMALL: f32 = 11.5;
/// The body's and the foot's sides.
const SIDE: f32 = 16.0;

/// Where the key goes when a keyring takes it, each platform with its own name for it.
fn keyring_words() -> &'static str {
    if cfg!(target_os = "macos") {
        "Kept in this computer's keyring (the Keychain)."
    } else if cfg!(windows) {
        "Kept in this computer's keyring (the Credential Manager)."
    } else {
        "Kept in this computer's keyring (the Secret Service)."
    }
}

/// What Templum to Templum is, under the title.
const WHAT: &str = "Work in a workspace of another computer as if it were here";
const CODE_HELP: &str = "Make one on the other computer: a workspace's Edit… ▸ Share…";
const NO_KEYRING: &str =
    "This computer has no keyring Templum can use: the connection lasts until Templum quits.";
const FILE_SWITCH: &str = "Keep the key in a file only you can read";
const FILE_WARNING: &str = "A copy of the data folder would carry it.";
const ACCESS_HELP: &str = "A Cloudflare Access service token, for a tunnel with Access in front.";
const PROXY_HELP: &str = "For a network that reaches out only through a proxy.";

/// The code's host without its port, for the environment's proxy and `NO_PROXY`.
fn bare_host(code: &Code) -> String {
    let host = code.host();
    match host.rsplit_once(':') {
        Some((name, port)) if !name.ends_with(']') || port.chars().all(|c| c.is_ascii_digit()) => {
            name.trim_matches(['[', ']']).to_string()
        }
        _ => host,
    }
}

/// What the folded Network section holds, said beside its name: "Access token · proxy
/// proxy.corp:8080", or what it is for when it holds nothing. A proxy's login is never shown.
fn network_words(client_id: &str, proxy: &str) -> String {
    let mut held = Vec::new();
    if !client_id.trim().is_empty() {
        held.push("Access token".to_string());
    }
    let proxy = proxy.trim();
    if !proxy.is_empty() {
        held.push(match proxy_of(proxy) {
            Ok((address, _)) => format!("proxy {}", address.trim_start_matches("http://")),
            Err(_) => "proxy".to_string(),
        });
    }
    if held.is_empty() {
        "Cloudflare Access, proxy".to_string()
    } else {
        held.join(" · ")
    }
}

/// What the dialog shows and holds.
pub(crate) struct ConnectFlow {
    workspace: Entity<Workspace>,
    code: Entity<InputState>,
    client_id: Entity<InputState>,
    client_secret: Entity<InputState>,
    proxy: Entity<InputState>,
    /// The Network section is unfolded.
    more: bool,
    /// The proxy was prefilled from the environment (once, for the first code read).
    prefilled: bool,
    /// Keep the key in a file.
    file: bool,
    /// The address Connect was pressed for, until the remote says how it went.
    connecting: Option<String>,
    /// The remote a new code is for (Paste a new code…), else the dialog makes a new one.
    replace: Option<String>,
    title: SharedString,
    subtitle: SharedString,
    error: Option<String>,
    theme: std::rc::Rc<Theme>,
    ui: Ui,
    /// The terminal's font, the code's.
    mono: SharedString,
}

impl ConnectFlow {
    /// The code in the field, if it reads.
    fn decoded(&self, cx: &App) -> Option<Code> {
        code::decode(&self.code.read(cx).value()).ok()
    }

    /// Connect, or Enter: the code leaves the field, and the workspace makes the remote. Nothing
    /// while the field holds no code or a connection is under way.
    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.connecting.is_some() {
            return;
        }
        let Some(code) = self.decoded(cx) else {
            return;
        };
        let host = code.host();
        let text = |input: &Entity<InputState>, cx: &App| {
            let value = input.read(cx).value().trim().to_string();
            (!value.is_empty()).then_some(value)
        };
        // (A new code for a remote that left the secret empty keeps the one it has.)
        let secret = text(&self.client_secret, cx).map(Key::new);
        let access = text(&self.client_id, cx).map(|id| (id, secret));
        let proxy = text(&self.proxy, cx);
        let draft = Connecting {
            code,
            access,
            proxy: proxy.clone().map(Key::new),
            file: self.file,
        };
        self.code
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.client_secret
            .update(cx, |input, cx| input.set_value("", window, cx));
        // The proxy stays for a second try, without the login it may carry.
        if let Some((address, _)) = proxy.and_then(|p| proxy_of(&p).ok()) {
            self.proxy
                .update(cx, |input, cx| input.set_value(address, window, cx));
        }
        // A new code for a remote that is already here: the banner tells how it goes.
        if let Some(id) = self.replace.clone() {
            let done = self
                .workspace
                .update(cx, |ws, cx| ws.replace_code(&id, draft, window, cx));
            match done {
                Ok(()) => window.close_dialog(cx),
                Err(why) => self.error = Some(why),
            }
            cx.notify();
            return;
        }
        let made = self
            .workspace
            .update(cx, |ws, cx| ws.connect_code(draft, window, cx));
        match made {
            Ok(_) => {
                self.connecting = Some(host);
                self.error = None;
            }
            Err(why) => self.error = Some(why),
        }
        cx.notify();
    }

    /// The workspace said how the remote went.
    fn heard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.connecting.is_none() {
            return;
        }
        match self.workspace.read(cx).connect_result() {
            Some(Ok(())) => window.close_dialog(cx),
            Some(Err(why)) => {
                self.connecting = None;
                self.error = Some(why);
            }
            None => return,
        }
        cx.notify();
    }

    /// The dialog closes before the remote is live (Cancel, Close, Esc, a press outside):
    /// nothing of it is left.
    fn give_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.connecting = None;
        self.workspace
            .update(cx, |ws, cx| ws.cancel_connect(window, cx));
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

    /// The line under the code's field: where to make one while it is empty, then what the code
    /// is, or what is wrong with it.
    fn code_says(&self, cx: &App) -> AnyElement {
        let typed = self.code.read(cx).value();
        let line = match code::decode(&typed) {
            Ok(code) => Ok(code.said()),
            Err(wrong) if !typed.trim().is_empty() => Err(wrong.to_string()),
            Err(_) => return self.help("connect-code-help", CODE_HELP).into_any_element(),
        };
        let red = hsla(self.theme.palette.ansi[1], 1.0);
        div()
            .id("connect-says")
            .w_full()
            .flex()
            .items_start()
            .gap(px(4.0))
            .text_size(px(SMALL))
            .map(|el| match line {
                Ok(said) => el
                    .text_color(self.ui.title_dim)
                    .child(
                        Icon::new(IconName::Check)
                            .size(px(12.0))
                            .text_color(self.ui.accent)
                            .flex_shrink_0()
                            .mt(px(1.0)),
                    )
                    .child(div().min_w(px(0.0)).child(said)),
                Err(wrong) => el.text_color(red).child(wrong),
            })
            .test_support()
            .into_any_element()
    }

    /// The Key row: where the key goes; with no keyring, the switch that keeps it in a file.
    fn key_field(&self, keyring: Option<bool>, cx: &Context<Self>) -> Div {
        let field = div().flex().flex_col().gap(px(4.0));
        if keyring != Some(false) {
            return field.child(
                div()
                    .id("connect-keyring")
                    .min_h(px(popup::FIELD_H))
                    .flex()
                    .items_center()
                    .text_size(px(12.0))
                    .child(keyring_words())
                    .test_support(),
            );
        }
        let file = self.file;
        let this = cx.entity();
        field
            .child(
                popup::line()
                    .min_h(px(popup::FIELD_H))
                    .child(Switch::new("connect-file").small().checked(file).on_click(
                        move |on: &bool, _, cx| {
                            let on = *on;
                            this.update(cx, |flow, cx| {
                                flow.file = on;
                                cx.notify();
                            })
                        },
                    ))
                    .child(div().text_size(px(12.0)).child(FILE_SWITCH)),
            )
            .child(if file {
                self.help("connect-file-says", FILE_WARNING)
            } else {
                self.help("connect-keyring", NO_KEYRING)
            })
    }

    /// The Network section, folded until its name is pressed: the Access service token and the
    /// proxy.
    fn network(&self, ws: &Workspace, cx: &Context<Self>) -> Div {
        let ui = self.ui;
        let more = self.more;
        let this = cx.entity();
        let toggle = div()
            .id("connect-more")
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_pointer()
            .child(
                Icon::new(if more {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(px(12.0))
                .text_color(ui.title_dim),
            )
            .child(crate::inbox::section(ws, "Network"))
            .when(!more, |el| {
                el.child(
                    div()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(SMALL))
                        .text_color(ui.title_dim)
                        .child(network_words(
                            &self.client_id.read(cx).value(),
                            &self.proxy.read(cx).value(),
                        )),
                )
            })
            .on_click(move |_, _, cx| {
                this.update(cx, |flow, cx| {
                    flow.more = !flow.more;
                    cx.notify();
                })
            })
            .test_support();
        let field = |input: &Entity<InputState>| Input::new(input).small();
        let with_help = |input: Input, id: &'static str, help: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(input)
                .child(self.help(id, help))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(toggle)
            .when(more, |el| {
                el.child(popup::row(ws, "Client ID", field(&self.client_id)))
                    .child(popup::row(
                        ws,
                        "Client Secret",
                        with_help(
                            field(&self.client_secret),
                            "connect-access-help",
                            ACCESS_HELP,
                        ),
                    ))
                    .child(popup::row(
                        ws,
                        "Proxy",
                        with_help(field(&self.proxy), "connect-proxy-help", PROXY_HELP),
                    ))
            })
    }

    /// The foot: how it goes (connecting, a failure, a question) and the buttons.
    fn foot(&self, ws: &Workspace, ready: bool, cx: &Context<Self>) -> Div {
        let ui = self.ui;
        let red = hsla(self.theme.palette.ansi[1], 1.0);
        let yellow = hsla(self.theme.palette.ansi[3], 1.0);
        let choice = ws.connect_choice();
        let this = cx.entity();
        let said = |id: &'static str, color, words: String| {
            div()
                .id(id)
                .w_full()
                .text_size(px(SMALL))
                .text_color(color)
                .child(words)
                .test_support()
        };
        // A certificate from an authority this computer added: the person decides, and Enter
        // doesn't decide for them.
        let buttons: Vec<AnyElement> = if choice.is_some() {
            let choose = |id: &'static str, label: &'static str, act: Act| {
                let workspace = self.workspace.clone();
                balloon::ghost(id, label, ui, 1.0)
                    .on_click(move |_, window, cx| {
                        workspace.update(cx, |ws, cx| {
                            if let Some(id) = ws.pending_connect.clone() {
                                ws.remote_act(&id, act, window, cx);
                            }
                        })
                    })
                    .test_support()
                    .into_any_element()
            };
            vec![
                choose("connect-dont", "Don't connect", Act::DontConnect),
                choose("connect-anyway", "Connect anyway", Act::ConnectAnyway),
            ]
        } else {
            let go = ready && self.connecting.is_none();
            let (cancel, connect) = (this.clone(), this);
            vec![
                balloon::ghost("connect-cancel", "Cancel", ui, 1.0)
                    .on_click(move |_, window, cx| dismiss(&cancel, window, cx))
                    .test_support()
                    .into_any_element(),
                balloon::solid("connect-go", "Connect", None, ui, 1.0)
                    .when(!go, |el| el.opacity(0.4).cursor_default())
                    .on_click(move |_, window, cx| {
                        connect.update(cx, |flow, cx| flow.connect(window, cx))
                    })
                    .test_support()
                    .into_any_element(),
            ]
        };
        let waiting = self
            .connecting
            .as_ref()
            .filter(|_| choice.is_none())
            .map(|host| {
                div()
                    .id("connect-connecting")
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .min_w(px(0.0))
                    .text_size(px(SMALL))
                    .text_color(ui.title_dim)
                    .child(Spinner::new().with_size(UiSize::Size(px(12.0))))
                    .child(
                        div()
                            .min_w(px(0.0))
                            .truncate()
                            .child(format!("Connecting to {host}…")),
                    )
                    .test_support()
            });
        popup::foot(ws).child(
            popup::content()
                .px(px(SIDE))
                .gap(px(6.0))
                .children(choice.map(|words| said("connect-added", yellow, words)))
                .children(
                    self.error
                        .clone()
                        .map(|why| said("connect-error", red, why)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .children(waiting)
                        .child(div().flex_1())
                        .children(buttons),
                ),
        )
    }
}

impl Render for ConnectFlow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.clone();
        let ws = workspace.read(cx);
        let decoded = self.decoded(cx);
        let code_row = popup::row(
            ws,
            "Code",
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    Input::new(&self.code)
                        .small()
                        .font_family(self.mono.clone()),
                )
                .child(self.code_says(cx)),
        );
        let key_row = popup::row(ws, "Key", self.key_field(ws.keyring, cx));
        let body = popup::content()
            .px(px(SIDE))
            .py(px(16.0))
            .gap(px(18.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(code_row)
                    .child(key_row),
            )
            .child(self.network(ws, cx));
        let this = cx.entity();
        popup::dialog_frame("connect-dialog", ws)
            .text_size(px(TEXT))
            .child(popup::dialog_header(
                ws,
                "connect",
                popup::tile(IconName::Plug, self.ui.accent),
                self.title.clone(),
                Some(popup::subtitle(ws, self.subtitle.clone())),
                move |_: &ClickEvent, window, cx| dismiss(&this, window, cx),
            ))
            .child(body)
            .child(self.foot(ws, decoded.is_some(), cx))
            .test_support()
    }
}

/// Cancel and Close: what Esc does.
fn dismiss(flow: &Entity<ConnectFlow>, window: &mut Window, cx: &mut App) {
    flow.update(cx, |flow, cx| flow.give_up(window, cx));
    window.close_dialog(cx);
}

/// Connect to a Templum…: the code's field, then Connect.
pub(crate) fn open(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    open_as(ws, None, window, cx);
}

/// Paste a new code… for the remote workspace `id`: the same dialog, which replaces that
/// connection's code instead of making a workspace.
pub(crate) fn open_for(
    ws: &mut Workspace,
    id: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    open_as(ws, Some(id.to_string()), window, cx);
}

fn open_as(
    ws: &mut Workspace,
    replace: Option<String>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    ws.probe_keyring(window, cx);
    let (title, subtitle) = match replace
        .as_deref()
        .and_then(|id| ws.spaces().find(|s| s.id == id))
    {
        Some(space) => (
            "Paste a new code".to_string(),
            match space.remote.as_deref() {
                Some(r) => format!("For {} on {}", space.name, crate::mirror::computer_of(r)),
                None => format!("For {}", space.name),
            },
        ),
        None => ("Connect to a Templum".to_string(), WHAT.to_string()),
    };
    let workspace = cx.entity();
    let code = cx.new(|cx| InputState::new(window, cx).placeholder("tplc1_…"));
    let client_id = cx.new(|cx| InputState::new(window, cx).placeholder("xxxxxxxx.access"));
    let client_secret = cx.new(|cx| {
        let state = InputState::new(window, cx).masked(true);
        match replace {
            Some(_) => state.placeholder("Empty keeps it"),
            None => state,
        }
    });
    let proxy = cx.new(|cx| InputState::new(window, cx).placeholder("http://proxy.corp:8080"));
    // A new code for a remote starts from what that remote reaches home with; the secrets are
    // not shown, and left empty they stay (`replace_code`).
    let (old_id, old_proxy) = replace
        .as_deref()
        .and_then(|id| ws.spaces().find(|s| s.id == id))
        .and_then(|space| space.remote.as_deref())
        .map(|r| (r.entry.access_id.clone(), r.entry.proxy.clone()))
        .unwrap_or_default();
    let more = old_id.is_some() || old_proxy.is_some();
    for (input, value) in [(&client_id, old_id), (&proxy, old_proxy.clone())] {
        if let Some(value) = value {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
    }
    let view = cx.new(|cx| {
        // The remote's progress and the keyring's answer change what the dialog says.
        cx.observe_in(
            &workspace,
            window,
            |flow: &mut ConnectFlow, _, window, cx| {
                flow.heard(window, cx);
                cx.notify();
            },
        )
        .detach();
        ConnectFlow {
            workspace: workspace.clone(),
            code: code.clone(),
            client_id: client_id.clone(),
            client_secret: client_secret.clone(),
            proxy: proxy.clone(),
            more,
            prefilled: old_proxy.is_some(),
            file: false,
            connecting: None,
            replace,
            title: title.into(),
            subtitle: subtitle.into(),
            error: None,
            theme: ws.theme.clone(),
            ui: ws.ui,
            mono: ws.cell.font.family.clone(),
        }
    });
    // What is typed changes what the dialog says; a new code makes the last failure old news.
    let typed = view.downgrade();
    App::subscribe(cx, &code, move |field, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
            let fresh = !field.read(cx).value().is_empty();
            typed
                .update(cx, |flow, cx| {
                    if fresh {
                        flow.error = None;
                    }
                    cx.notify()
                })
                .ok();
        }
    })
    .detach();
    for field in [&client_id, &proxy] {
        let typed = view.downgrade();
        App::subscribe(cx, field, move |_, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                typed.update(cx, |_, cx| cx.notify()).ok();
            }
        })
        .detach();
    }
    // The proxy the environment names for the code's host fills its field, once.
    cx.observe_in(&view, window, |_, view, window, cx| {
        let (prefilled, decoded) = {
            let flow = view.read(cx);
            (flow.prefilled, flow.decoded(cx))
        };
        if let (false, Some(code)) = (prefilled, decoded) {
            let found = proxy_from_env(&bare_host(&code), |name| std::env::var(name).ok());
            view.update(cx, |flow, cx| {
                flow.prefilled = true;
                if let Some(found) = found {
                    flow.proxy
                        .update(cx, |input, cx| input.set_value(found, window, cx));
                }
            });
        }
    })
    .detach();
    let shown = view.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let (enter, closed) = (shown.clone(), shown.clone());
        popup::dialog(dialog, shown.clone())
            // Enter connects; the dialog closes once the remote is live.
            .on_ok(move |_, window, cx| {
                enter.update(cx, |flow, cx| flow.connect(window, cx));
                false
            })
            .on_close(move |_, window, cx| {
                closed.update(cx, |flow, cx| flow.give_up(window, cx));
            })
    });
    code.update(cx, |input, cx| input.focus(window, cx));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folded_network_says_what_it_holds_without_a_proxy_s_login() {
        assert_eq!(network_words("", " "), "Cloudflare Access, proxy");
        assert_eq!(
            network_words("a1.access", "http://user:hunter2@proxy.corp:8080"),
            "Access token · proxy proxy.corp:8080"
        );
        // What doesn't read as a proxy may still hold a login: it isn't repeated.
        assert_eq!(network_words("", "user:hunter2@proxy.corp"), "proxy");
    }
}
