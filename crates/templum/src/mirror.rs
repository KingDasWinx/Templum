//! What a remote workspace shows besides its blocks (docs/specs/templum-to-templum.md, The remote
//! workspace and When it isn't live): the banner over the canvas's top edge, the tile's mark and
//! tooltip, and the Forget… dialog. The words are `workspace::remote::banner_words`'.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Disableable as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, ElementId, HighlightStyle, InteractiveText, StyledText, TestSupportExt,
    Window, div, px,
};
use templum_api::client::Status;

use crate::grid::hsla;
use crate::theme::Ui;
use crate::workspace::Workspace;
use crate::workspace::remote::{
    Act, Banner, MOST_BLOCKS, MOST_WIRES, NOT_MIRRORED, RemoteSpace, banner_words,
};

/// A round trip past this is yellow: each key waits for it.
const SLOW: Duration = Duration::from_millis(250);

/// The computer a remote mirrors: its name from home, else the code's host.
pub(crate) fn computer_of(remote: &RemoteSpace) -> String {
    remote
        .me
        .as_ref()
        .map(|m| m.computer.clone())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| crate::workspace::remote::host_of(&remote.entry.url))
}

/// The slow round trip's tooltip: "Each key waits about 300 ms for winx-desktop's answer."
pub(crate) fn slow_tip(latency: Duration, computer: &str) -> String {
    format!(
        "Each key waits about {} ms for {computer}'s answer.",
        latency.as_millis()
    )
}

/// What the banner's ⓘ lists: what a mirror doesn't show or do yet, and how many blocks it takes
/// at most.
pub(crate) fn not_mirrored_tip() -> String {
    format!("{NOT_MIRRORED} Only the front {MOST_BLOCKS} blocks and {MOST_WIRES} wires are shown.")
}

/// What Start it does: "Starts API on winx-desktop and shows it on its screen."
fn start_tip(name: &str, remote: &RemoteSpace) -> String {
    format!(
        "Starts {name} on {} and shows it on its screen.",
        computer_of(remote)
    )
}

/// The tile's tooltip: "API, on winx-desktop · 84 ms".
pub(crate) fn tile_tip(name: &str, remote: &RemoteSpace) -> String {
    let mut tip = format!("{name}, on {}", computer_of(remote));
    if let Some(latency) = remote.latency.filter(|_| remote.status == Status::Live) {
        tip += &format!(" · {} ms", latency.as_millis());
    }
    tip
}

/// The small mark at a remote tile's bottom-right. The caller places it.
pub(crate) fn tile_mark(ui: Ui, space: &str) -> impl IntoElement + Styled {
    div()
        .id(ElementId::Name(format!("workspace-remote-{space}").into()))
        .flex()
        .items_center()
        .justify_center()
        .child(
            Icon::new(IconName::Globe)
                .size(px(11.0))
                .text_color(ui.accent),
        )
        .test_support()
}

/// The banner of the remote workspace shown, if that is one.
pub(crate) fn banner(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<AnyElement> {
    let remote = ws.space.remote.as_deref()?;
    let ui = ws.ui;
    let id = ws.space.id.clone();
    let Banner { text, acts, dim } = banner_words(remote, Instant::now(), SystemTime::now());
    let live = remote.status == Status::Live;
    let trying = matches!(remote.status, Status::Reconnecting { .. });
    let slow = remote
        .latency
        .filter(|l| live && *l > SLOW && remote.home.as_ref().is_none_or(|h| !h.stopped));
    let computer = computer_of(remote);
    let status = remote.status.clone();
    let yellow = hsla(ws.theme.palette.ansi[3], 1.0);
    let entity = cx.entity();
    let start = start_tip(&ws.space.name, remote);
    let button = |name: String, label: &'static str, act: Act| {
        let (entity, id, start) = (entity.clone(), id.clone(), start.clone());
        crate::title_bar::button("remote-act", ui)
            .id(ElementId::Name(name.into()))
            .flex_shrink_0()
            .child(label)
            .when(act == Act::StartIt, |el| {
                el.tooltip(move |window, cx| Tooltip::new(start.clone()).build(window, cx))
            })
            .on_click(move |_, window, cx| {
                entity.update(cx, |ws, cx| ws.remote_act(&id, act, window, cx))
            })
            .test_support()
    };
    // The round trip, in yellow when each key waits long for it: a range of the words' one line,
    // since each child of the words would be a line of its own.
    let slow_ms = slow.and_then(|l| {
        let ms = format!("{} ms", l.as_millis());
        let at = text.find(&ms)?;
        Some((at..at + ms.len(), slow_tip(l, &computer)))
    });
    let line = StyledText::new(text).with_highlights(
        slow_ms
            .iter()
            .map(|(range, _)| (range.clone(), HighlightStyle::from(yellow))),
    );
    let mut line = InteractiveText::new("remote-banner-line", line);
    if let Some((range, tip)) = slow_ms {
        line = line.tooltip(move |ix, window, cx| {
            range
                .contains(&ix)
                .then(|| Tooltip::new(tip.clone()).build(window, cx))
        });
    }
    let info_tip = not_mirrored_tip();
    let chain_tip = matches!(
        status,
        Status::Cut(templum_api::client::Why::AddedAuthority)
    );
    let words = div()
        .id("remote-banner-text")
        .flex_1()
        .min_w(px(0.0))
        .overflow_hidden()
        .text_size(px(12.0))
        .text_color(ui.title)
        .whitespace_nowrap()
        .truncate()
        .child(line)
        .when(chain_tip, |el| {
            el.tooltip(|window, cx| {
                Tooltip::new("or the server doesn't send its whole chain").build(window, cx)
            })
        })
        .test_support();
    let mut row = div()
        .id("remote-banner")
        .flex()
        .items_center()
        .gap(px(8.0))
        .max_w(px(760.0))
        .min_w(px(0.0))
        .h(px(32.0))
        .pl(px(14.0))
        .pr(px(6.0))
        .rounded_full()
        .overflow_hidden()
        .border_1()
        .border_color(ui.hairline)
        .bg(ui.surface)
        .when(dim, |el| el.border_color(yellow.opacity(0.6)))
        .child(words)
        .child(
            div()
                .id("remote-banner-info")
                .flex_shrink_0()
                .child(
                    Icon::new(IconName::Info)
                        .size(px(14.0))
                        .text_color(ui.title),
                )
                .tooltip(move |window, cx| Tooltip::new(info_tip.clone()).build(window, cx))
                .test_support(),
        );
    for (n, act) in acts.iter().enumerate() {
        row = row.child(button(format!("remote-act-{n}"), act.label(&status), *act));
    }
    // Disconnect while it is live or trying (a stopped workspace is live, too).
    if live || trying {
        row = row.child(button(
            "remote-disconnect".into(),
            "Disconnect",
            Act::DontConnect,
        ));
    }
    Some(
        div()
            .id("remote-banner-row")
            .absolute()
            .top(px(8.0))
            .left_0()
            .w_full()
            .px(px(16.0))
            .flex()
            .justify_center()
            .overflow_hidden()
            .child(row.test_support())
            .into_any_element(),
    )
}

/// A veil over the blocks while the remote isn't live: the canvas's color at half.
pub(crate) fn veil(ws: &Workspace) -> Option<AnyElement> {
    ws.space.remote.as_deref().filter(|r| r.dim())?;
    Some(
        div()
            .id("remote-veil")
            .absolute()
            .size_full()
            .bg(ws.ui.canvas.opacity(0.5))
            .test_support()
            .into_any_element(),
    )
}

/// A stopped workspace at home: a card in the canvas's middle, with Start it.
pub(crate) fn stopped_card(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<AnyElement> {
    let remote = ws.space.remote.as_deref()?;
    if remote.status != Status::Live || !remote.home.as_ref().is_some_and(|h| h.stopped) {
        return None;
    }
    let ui = ws.ui;
    let (id, entity) = (ws.space.id.clone(), cx.entity());
    let text = banner_words(remote, Instant::now(), SystemTime::now()).text;
    let tip = start_tip(&ws.space.name, remote);
    Some(
        div()
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("remote-stopped")
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.0))
                    .p(px(20.0))
                    .max_w(px(420.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(ui.hairline)
                    .bg(ui.surface)
                    .child(
                        div()
                            .w_full()
                            .overflow_hidden()
                            .truncate()
                            .text_color(ui.title)
                            .child(text),
                    )
                    .child(
                        crate::title_bar::button("remote-start", ui)
                            .child("Start it")
                            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                            .on_click(move |_, window, cx| {
                                entity.update(cx, |ws, cx| {
                                    ws.remote_act(&id, Act::StartIt, window, cx)
                                })
                            }),
                    )
                    .test_support(),
            )
            .into_any_element(),
    )
}

/// Forget… on remote `id`: "Forget API on winx-desktop?" with the switch that revokes the share
/// there too (on while it is live, off and disabled otherwise).
pub(crate) fn forget_dialog(
    ws: &mut Workspace,
    id: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some((name, computer, live)) = ws
        .spaces()
        .find(|s| s.id == id)
        .and_then(|s| Some((s, s.remote.as_deref()?)))
        .map(|(s, r)| (s.name.clone(), computer_of(r), r.status == Status::Live))
    else {
        return;
    };
    let (workspace, id) = (cx.entity(), id.to_string());
    let revoke = Rc::new(Cell::new(live));
    let title = format!("Forget {name} on {computer}?");
    let words = format!("Also revoke the share on {computer}");
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (workspace, id, revoke, state) = (
            workspace.clone(),
            id.clone(),
            revoke.clone(),
            revoke.clone(),
        );
        let toggle = revoke.clone();
        alert
            .confirm()
            .title(title.clone())
            .button_props(DialogButtonProps::default().ok_text("Forget"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        Switch::new("forget-revoke")
                            .checked(state.get() && live)
                            .disabled(!live)
                            .on_click(move |on: &bool, window, _| {
                                toggle.set(*on);
                                window.refresh();
                            }),
                    )
                    .child(div().text_size(px(13.0)).child(words.clone())),
            )
            .on_ok(move |_, window, cx| {
                let revoke = revoke.get() && live;
                workspace.update(cx, |ws, cx| ws.forget_remote(&id, revoke, window, cx));
                true
            })
    });
}
