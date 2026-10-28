//! The Git block's commit form: the message, and Commit (its ▾: Commit & Push, Amend).

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _, Size as UiSize};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, SharedString, TestSupportExt, div, px};

use super::widgets::{field, icon_button};
use super::words;
use super::{FONT, Git};
use crate::editor::button;
use crate::grid::hsla;
use crate::menu::Target;
use crate::workspace::Workspace;
use crate::workspace::remote::NOT_YET;

/// The commit form's Description, about three rows high, at zoom 1.
const DESCRIPTION_H: f32 = 64.0;

/// The summary, the description once there is a summary, and the Commit button (its ▾: Commit &
/// Push, Amend); or why committing isn't possible.
pub(super) fn commit_form(
    ws: &Workspace,
    git: &Git,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let can = git.can_commit(cx);
    let len = git.summary_len(cx);
    let amend = git.amend;
    let described = len > 0 || amend || !git.description.read(cx).value().is_empty();
    // Lit with the theme's accent once there is something to commit.
    let (bg, color) = if can {
        (ui.accent, ui.surface)
    } else {
        (ui.card, ui.title_dim)
    };
    let hover = hsla(ws.theme.palette.foreground, 0.10);
    let commit = div()
        .id("git-commit")
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0 * z))
        .child(Icon::new(IconName::Check).size(px(14.0 * z)))
        .child(div().truncate().child(git.commit_label()))
        .tooltip(|window, cx| Tooltip::new(words::tip("Commit")).build(window, cx))
        .when(can, |el| {
            el.cursor_pointer().hover(move |st| st.bg(hover)).on_click(
                cx.listener(move |ws, _: &ClickEvent, window, cx| ws.git_commit(false, window, cx)),
            )
        })
        .test_support();
    let menu = div()
        .id("git-commit-menu")
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(px(32.0 * z))
        .h_full()
        .border_l_1()
        .border_color(color.opacity(0.3))
        .cursor_pointer()
        .hover(move |st| st.bg(hover))
        .child(Icon::new(IconName::ChevronDown).size(px(14.0 * z)))
        .tooltip(|window, cx| Tooltip::new("Commit & Push, Amend").build(window, cx))
        .on_click(cx.listener(move |ws, e: &ClickEvent, window, cx| {
            ws.open_menu(Target::GitCommitMenu, e.position(), window, cx)
        }))
        .test_support();
    div()
        .flex()
        .flex_shrink_0()
        .flex_col()
        .gap(px(8.0 * z))
        .p(px(10.0 * z))
        .border_t_1()
        .border_color(ui.hairline)
        .text_size(px(FONT * z))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0 * z))
                .child(field(&git.summary, z).flex_1())
                .when(len > 0, |el| {
                    el.child(
                        div()
                            .id("git-summary-count")
                            .flex_shrink_0()
                            .text_size(px(11.0 * z))
                            .text_color(if git.summary_too_long(cx) {
                                hsla(ws.theme.palette.ansi[3], 1.0)
                            } else {
                                ui.title_dim
                            })
                            .child(len.to_string())
                            .test_support(),
                    )
                })
                .children(generate_button(ws, git, z, cx)),
        )
        .when(described, |el| {
            el.child(
                // About three rows, at the zoom: gpui-kit sizes it from the theme's fixed font
                // otherwise.
                Textarea::new(&git.description)
                    .text_size(px(FONT * z))
                    .h(px(DESCRIPTION_H * z))
                    .w_full(),
            )
        })
        .when(amend, |el| {
            let yellow = hsla(ws.theme.palette.ansi[3], 1.0);
            el.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0 * z))
                    .px(px(8.0 * z))
                    .py(px(4.0 * z))
                    .rounded(px(6.0 * z))
                    .bg(yellow.opacity(0.12))
                    .text_size(px(11.0 * z))
                    .text_color(ui.title)
                    .child(
                        Icon::new(IconName::Pencil)
                            .size(px(12.0 * z))
                            .text_color(yellow),
                    )
                    .child(div().flex_1().child("Amending the last commit"))
                    .child(
                        div()
                            .id("git-amend-off")
                            .cursor_pointer()
                            .text_color(ui.title_dim)
                            .hover(|st| st.text_color(yellow))
                            .child(Icon::new(IconName::Close).size(px(12.0 * z)))
                            .tooltip(|window, cx| {
                                Tooltip::new("Commit a new one instead").build(window, cx)
                            })
                            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                                ws.set_amend(false, window, cx)
                            }))
                            .test_support(),
                    ),
            )
        })
        .child(
            div()
                .flex()
                .items_center()
                .h(px(34.0 * z))
                .rounded(px(8.0 * z))
                .overflow_hidden()
                .bg(bg)
                .text_color(color)
                .text_size(px(12.5 * z))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(commit)
                .child(menu),
        )
        .when(git.author.is_none() && git.has_repo(), |el| {
            el.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0 * z))
                    .text_color(ui.title_dim)
                    .child("Set user.name and user.email to commit")
                    .child(
                        button("git-set-author", "Set…", ui, z)
                            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                                ws.set_git_author(window, cx)
                            }))
                            .test_support(),
                    ),
            )
        })
        .into_any_element()
}

/// Generate ✨ and its ⚙, beside the summary: shown while the agent chosen in Settings ▸ Git is ready and
/// there is something to describe (what is staged, or every change for Commit all changes), or
/// while the agent writes, when it spins and a click stops it.
fn generate_button(
    ws: &Workspace,
    git: &Git,
    z: f32,
    cx: &mut Context<Workspace>,
) -> Option<AnyElement> {
    // On a mirror home's agent writes the message, whatever agent this computer has.
    let home = ws
        .space
        .remote
        .as_deref()
        .filter(|_| git.home.is_some())
        .map(crate::mirror::computer_of);
    let agent = ws.commit_agent.as_ref().filter(|a| a.ready);
    if home.is_none() && agent.is_none() {
        return None;
    }
    let running = git.generating.is_some();
    if !git.has_repo() || (git.status.staged().is_empty() && !git.commit_all() && !running) {
        return None;
    }
    let ui = ws.ui;
    let tip: SharedString = match (&home, running, agent) {
        (Some(computer), true, _) => {
            format!("Running on {computer}: it can't be stopped from here.").into()
        }
        (Some(computer), false, _) => {
            format!("Generate: {computer}'s agent writes the message").into()
        }
        (None, true, _) => "Stop the agent".into(),
        (None, false, Some(agent)) => {
            format!("Generate: {} writes the message", agent.title).into()
        }
        (None, false, None) => return None,
    };
    let generate = div()
        .id("git-generate")
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(22.0 * z))
        .rounded(px(5.0 * z))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(if running {
            Spinner::new()
                .with_size(UiSize::Size(px(12.0 * z)))
                .into_any_element()
        } else {
            Icon::new(IconName::Sparkles)
                .size(px(13.0 * z))
                .text_color(ui.title)
                .into_any_element()
        })
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
    // Running at home, a click does nothing: it can't be stopped from here.
    let generate = if home.is_some() && running {
        generate.test_support()
    } else {
        generate
            .on_click(
                cx.listener(move |ws, _: &ClickEvent, window, cx| ws.generate_message(window, cx)),
            )
            .test_support()
    };
    let menu = icon_button("git-generate-menu", IconName::Settings2, ui, z).flex_shrink_0();
    // The ⚙ (the model, this repository's instructions) stays on home for now.
    let menu = if home.is_some() {
        menu.opacity(0.5)
            .tooltip(|window, cx| Tooltip::new(NOT_YET).build(window, cx))
            .test_support()
    } else {
        menu.tooltip(|window, cx| {
            Tooltip::new("The model, and this repository's instructions").build(window, cx)
        })
        .on_click(cx.listener(move |ws, e: &ClickEvent, window, cx| {
            ws.open_menu(Target::GitGenerateMenu, e.position(), window, cx)
        }))
        .test_support()
    };
    Some(
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .child(generate)
            .child(menu)
            .into_any_element(),
    )
}
