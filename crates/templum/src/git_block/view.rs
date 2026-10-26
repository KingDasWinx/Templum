//! The Git popup: its header (the folder, close), and its body: what the folder holds, or the
//! repository's top bar, tabs and shown tab.

use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _, Size as UiSize};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Div, ElementId, Hsla, KeyDownEvent, ScrollWheelEvent,
    SharedString, TestSupportExt, div, px,
};
use templum_git::Operation;

use super::branches::branches;
use super::changes::changes;
use super::history::history;
use super::stashes::stashes;
use super::switcher::Opened;
use super::widgets::{centered, icon_button};
use super::{FONT, Found, Git, Sync, Tab, words};
use crate::editor::button;
use crate::grid::hsla;
use crate::menu::Target;
use crate::popup;
use crate::theme::Ui;
use crate::workspace::Workspace;
use crate::workspace::remote::Deed;

/// How to get `git` on this OS.
pub(crate) fn install_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "Install it with `xcode-select --install` or `brew install git`."
    } else if cfg!(windows) {
        "Install it with `winget install Git.Git`, or from git-scm.com."
    } else {
        "Install it with your package manager: `sudo pacman -S git`, `sudo apt install git`."
    }
}

/// The popup, `w` × `h` pixels: the header (the folder, a click lists the others; the branch
/// and where it stands with the server; Sync and the other actions; full screen; close), and the
/// body.
pub(crate) fn popup(
    ws: &Workspace,
    git: &Git,
    w: f32,
    h: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let (name, path) = (git.name(), SharedString::from(git.place()));
    let repo = git.has_repo() && !git.gone;
    let title = div()
        .id("git-folder")
        .flex()
        .items_center()
        .gap(px(4.0))
        .min_w(px(0.0))
        .cursor_pointer()
        .hover(move |st| st.text_color(ui.accent))
        .child(div().truncate().child(name))
        .child(
            Icon::new(IconName::ChevronDown)
                .size(px(13.0))
                .text_color(ui.title_dim),
        )
        .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
        .on_click(cx.listener(|ws, e: &ClickEvent, window, cx| {
            ws.open_menu(Target::GitFolders, e.position(), window, cx)
        }))
        .test_support();
    let subtitle = repo.then(|| branch_line(ws, git, cx).into_any_element());
    let actions = if repo {
        actions(ws, git, cx)
    } else {
        Vec::new()
    };
    popup::frame("git-popup", ws, w, h)
        .child(popup::header(
            ws,
            "git",
            popup::tile(IconName::FolderGit2, ui.accent),
            title,
            subtitle,
            actions,
            cx,
        ))
        .child(div().flex_1().min_h(px(0.0)).child(body(ws, git, 1.0, cx)))
        .test_support()
        .into_any_element()
}

/// Under the folder's name: the branch (a click opens the branch dialog), where it stands with the
/// server, and the terminals working in the repository.
fn branch_line(ws: &Workspace, git: &Git, cx: &mut Context<Workspace>) -> impl IntoElement {
    let ui = ws.ui;
    let s = &git.status;
    let branch: String = match (&s.branch, &s.oid) {
        (Some(b), _) => b.clone(),
        (None, Some(oid)) => format!("detached at {}", &oid[..oid.len().min(7)]),
        (None, None) => "no commits".to_string(),
    };
    let (state, tip) = words::server_state(s);
    // The terminals working in the repository, as their last snapshot saw them: cheap.
    let terminals = match (&git.found, git.repo()) {
        (Found::Home(root), _) => ws.home_terminals_in(root).len(),
        (_, Some(repo)) => ws
            .space
            .terminals()
            .filter(|(_, pane)| pane.works_in(&repo.root))
            .count(),
        _ => 0,
    };
    let dot = || div().text_color(ui.title_dim).child("·");
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .min_w(px(0.0))
        .text_size(px(FONT))
        .text_color(ui.title_dim)
        .child(
            div()
                .id("branch")
                .flex()
                .items_center()
                .gap(px(4.0))
                .min_w(px(0.0))
                .text_color(ui.title)
                .cursor_pointer()
                .hover(move |st| st.text_color(ui.accent))
                .child(Icon::new(IconName::GitBranch).size(px(12.0)))
                .child(div().truncate().child(branch))
                .tooltip(|window, cx| Tooltip::new("Switch or create a branch").build(window, cx))
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.open_branch_dialog(Opened::Header, window, cx)
                }))
                .test_support(),
        )
        .when(!state.is_empty(), |el| {
            let tip = SharedString::from(tip);
            el.child(dot()).child(
                div()
                    .id("git-state")
                    .flex_shrink_0()
                    .child(state)
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx)),
            )
        })
        .when(terminals > 0, |el| {
            el.child(dot()).child(
                div()
                    .id("git-terminals")
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(3.0))
                    .cursor_pointer()
                    .hover(move |st| st.text_color(ui.accent))
                    .child(Icon::new(IconName::SquareTerminal).size(px(12.0)))
                    .child(terminals.to_string())
                    .tooltip(|window, cx| {
                        Tooltip::new("Terminals in this repository").build(window, cx)
                    })
                    .on_click(cx.listener(move |ws, e: &ClickEvent, window, cx| {
                        ws.open_menu(Target::GitTerminals, e.position(), window, cx)
                    }))
                    .test_support(),
            )
        })
}

/// The header's actions: Compare with main, Sync (or Publish branch), ⋯ and ↻.
fn actions(ws: &Workspace, git: &Git, cx: &mut Context<Workspace>) -> Vec<AnyElement> {
    let ui = ws.ui;
    let s = &git.status;
    // Compare with main: while there is a default branch and this branch isn't it.
    let compare_with = git
        .default_branch
        .as_deref()
        .map(words::local_name)
        .filter(|b| s.branch.as_deref().is_some_and(|c| c != *b))
        .map(str::to_string);
    let on_branch = s.branch.clone().filter(|_| s.oid.is_some());
    let publish = s.upstream.is_none() || s.gone;
    let running = git.sync.is_some();
    // A command running at home runs on: the button says so and doesn't stop it.
    let at_home = ws
        .space
        .remote
        .as_deref()
        .filter(|_| git.home.is_some())
        .map(crate::mirror::computer_of);
    // The running command's button spins, and a click on it cancels it (Fetch and Pull from ⋯
    // spin here too). Tinted while there is something to send or get.
    let sync_button = (on_branch.is_some() || running).then(|| {
        let (label, word, icon) = match (running, publish) {
            (true, _) if at_home.is_some() => ("Running", "Sync", None),
            (true, _) => ("Cancel", "Sync", None),
            (false, true) => ("Publish branch", "Push", Some(IconName::CloudUpload)),
            (false, false) => ("Sync", "Sync", Some(IconName::ArrowDownUp)),
        };
        let stuck = at_home.clone();
        let pending = publish || s.ahead > 0 || s.behind > 0;
        let (bg, color) = if pending || running {
            (ui.accent.opacity(0.16), ui.accent)
        } else {
            (ui.card, ui.title)
        };
        let el = div()
            .id("git-sync")
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(6.0))
            .h(px(28.0))
            .px(px(12.0))
            .rounded(px(8.0))
            .bg(bg)
            .text_size(px(FONT))
            .font_weight(gpui_kit::FontWeight::MEDIUM)
            .text_color(color)
            .cursor_pointer()
            .hover(move |st| {
                st.bg(if pending {
                    ui.accent.opacity(0.24)
                } else {
                    ui.hover
                })
            })
            .children(icon.map(|i| Icon::new(i).size(px(14.0))))
            .when(running, |el| {
                el.child(Spinner::new().with_size(UiSize::Size(px(12.0))))
            })
            .child(label)
            .tooltip(move |window, cx| match &stuck {
                Some(computer) => Tooltip::new(format!(
                    "Running on {computer}: it can't be stopped from here."
                ))
                .build(window, cx),
                None => Tooltip::new(words::tip(word)).build(window, cx),
            });
        if running && at_home.is_some() {
            el.test_support().into_any_element()
        } else if running {
            el.on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.cancel_git(cx)))
                .test_support()
                .into_any_element()
        } else if git.acting > 0 {
            el.opacity(0.5).test_support().into_any_element()
        } else {
            let branch = on_branch.clone().unwrap_or_default();
            el.on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                if publish {
                    ws.publish(branch.clone(), window, cx)
                } else {
                    ws.sync_git(window, cx)
                }
            }))
            .test_support()
            .into_any_element()
        }
    });
    let compare = compare_with.map(|base| {
        let current = s.branch.clone().unwrap_or_default();
        let tip = SharedString::from(format!("Compare with {base}"));
        icon_button("git-compare-main", IconName::GitCompareArrows, ui, 1.0)
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                ws.compare_branch(current.clone(), window, cx)
            }))
            .test_support()
            .into_any_element()
    });
    compare
        .into_iter()
        .chain(sync_button)
        .chain([
            icon_button("refresh", IconName::RefreshCw, ui, 1.0)
                .tooltip(|window, cx| Tooltip::new("Refresh").build(window, cx))
                .on_click(
                    cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        ws.refresh_git_now(window, cx)
                    }),
                )
                .test_support()
                .into_any_element(),
            icon_button("git-more", IconName::Ellipsis, ui, 1.0)
                .tooltip(|window, cx| Tooltip::new("More").build(window, cx))
                .on_click(cx.listener(move |ws, e: &ClickEvent, window, cx| {
                    ws.open_menu(Target::GitMore, e.position(), window, cx)
                }))
                .test_support()
                .into_any_element(),
        ])
        .collect()
}

/// The body below the header, at the zoom `z`.
pub(crate) fn body(ws: &Workspace, git: &Git, z: f32, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.ui;
    let content: AnyElement = if git.gone {
        centered(ui, z)
            .child(format!("{} is gone", git.folder.display()))
            .child(
                button("choose-folder", "Choose another folder…", ui, z)
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        ws.choose_git_folder(window, cx)
                    }))
                    .test_support(),
            )
            .into_any_element()
    } else {
        match &git.found {
            Found::Looking => centered(ui, z)
                .child("Looking for the repository…")
                .into_any_element(),
            Found::NoGit(err) => centered(ui, z)
                .child(format!(
                    "Couldn't run {}: {}",
                    ws.config.git.program(),
                    err.first_line()
                ))
                .child(install_hint())
                .child(
                    button("retry", "Try again", ui, z)
                        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                            ws.look_up_git(window, cx)
                        }))
                        .test_support(),
                )
                .into_any_element(),
            Found::NotARepo => centered(ui, z)
                .child(format!("{} is not a Git repository", git.place()))
                .child(
                    div()
                        .flex()
                        .gap(px(8.0 * z))
                        .child(
                            button("init", "Initialize", ui, z)
                                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                                    ws.init_git(window, cx)
                                }))
                                .test_support(),
                        )
                        .child({
                            // Spins while a clone runs; a click on it then cancels it.
                            let cloning = git.sync == Some(Sync::Cloning);
                            let label = if cloning {
                                "Cancel"
                            } else {
                                "Clone a repository…"
                            };
                            let el = button("git-clone", label, ui, z).when(cloning, |el| {
                                el.child(Spinner::new().with_size(UiSize::Size(px(12.0 * z))))
                            });
                            if cloning {
                                el.on_click(
                                    cx.listener(move |ws, _: &ClickEvent, _, cx| ws.cancel_git(cx)),
                                )
                            } else {
                                el.on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                                    ws.ask_clone(window, cx)
                                }))
                            }
                            .test_support()
                        })
                        .child(
                            button("choose-folder", "Choose another folder…", ui, z)
                                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                                    ws.choose_git_folder(window, cx)
                                }))
                                .test_support(),
                        ),
                )
                .into_any_element(),
            Found::Refused(why) => centered(ui, z)
                .child(why.clone())
                .child(
                    button("retry", "Try again", ui, z)
                        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                            ws.look_up_git(window, cx)
                        }))
                        .test_support(),
                )
                .into_any_element(),
            Found::Repo(_) | Found::Home(_) => repo_body(ws, git, z, cx),
        }
    };
    // The wheel scrolls the lists and stops there; with Ctrl it zooms the canvas.
    div()
        .size_full()
        .flex()
        .flex_col()
        .track_focus(&git.focus)
        .on_key_down(cx.listener(move |ws, e: &KeyDownEvent, window, cx| ws.git_key(e, window, cx)))
        .on_scroll_wheel(|e: &ScrollWheelEvent, _, cx| {
            if !e.modifiers.control {
                cx.stop_propagation();
            }
        })
        .child(content)
        .into_any_element()
}

/// The repository's view: the banner, the tabs, the shown tab.
fn repo_body(ws: &Workspace, git: &Git, z: f32, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.ui;
    let s = &git.status;
    let fg = hsla(ws.theme.palette.foreground, 1.0);
    let (yellow, red) = (
        hsla(ws.theme.palette.ansi[3], 1.0),
        hsla(ws.theme.palette.ansi[1], 1.0),
    );
    let detached = s.branch.is_none() && s.oid.is_some();
    let banner = if let Some(op) = git.operation {
        let conflicts = s.conflicts().len();
        // The Git word behind the sentence, explained on hover.
        let word = match op {
            Operation::Rebase => words::tip("Rebase"),
            Operation::Merge => words::tip("Merge"),
            _ => "",
        };
        Some(
            note(ui, yellow, IconName::TriangleAlert, z)
                .id("git-banner")
                .child(
                    div()
                        .id("git-banner-text")
                        .flex_1()
                        .min_w(px(0.0))
                        .child(words::operation_sentence(op, conflicts))
                        .when(!word.is_empty(), |el| {
                            el.tooltip(move |window, cx| Tooltip::new(word).build(window, cx))
                        }),
                )
                .child(match ws.why_not(Deed::Git("abort")) {
                    // A Control share can't abort at home: dimmed, the rule's words its tooltip.
                    Some(why) => button("git-abort", "Abort", ui, z)
                        .opacity(0.5)
                        .tooltip(move |window, cx| Tooltip::new(why.clone()).build(window, cx))
                        .test_support(),
                    None => button("git-abort", "Abort", ui, z)
                        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                            ws.abort_operation(window, cx)
                        }))
                        .test_support(),
                })
                .when(conflicts == 0, |el| {
                    el.child(
                        button("git-continue", "Continue", ui, z)
                            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                                ws.continue_operation(window, cx)
                            }))
                            .test_support(),
                    )
                })
                .test_support(),
        )
    } else if detached {
        let back = git.default_branch.clone();
        Some(
            note(ui, yellow, IconName::TriangleAlert, z)
                .id("git-banner")
                .child(div().flex_1().min_w(px(0.0)).child(
                    "You're looking at an old version. Commits made here won't belong to any branch.",
                ))
                .child(
                    button("git-branch-here", "Create a branch here", ui, z)
                        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                            ws.open_branch_dialog(Opened::Create, window, cx)
                        }))
                        .test_support(),
                )
                .when_some(back, |el, back| {
                    let label = format!("Back to {}", words::local_name(&back));
                    let remote = back.contains('/');
                    el.child(
                        button("git-back", label, ui, z)
                            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                                ws.switch_branch(back.clone(), remote, window, cx)
                            }))
                            .test_support(),
                    )
                })
                .test_support(),
        )
    } else {
        None
    };
    let changed = s.entries.len();
    let tabs = div()
        .id("git-tabs")
        .flex()
        .items_end()
        .flex_shrink_0()
        .gap(px(16.0 * z))
        .h(px(34.0 * z))
        .px(px(14.0 * z))
        .border_b_1()
        .border_color(ui.hairline)
        .children(Tab::ALL.map(|tab| {
            let active = git.tab == tab;
            let icon = match tab {
                Tab::Changes => IconName::FileDiff,
                Tab::History => IconName::Clock,
                Tab::Branches => IconName::GitBranch,
                Tab::Stashes => IconName::Archive,
            };
            div()
                .id(ElementId::Name(format!("git-tab-{}", tab.name()).into()))
                .flex()
                .items_center()
                .gap(px(6.0 * z))
                .h_full()
                .border_b_2()
                .border_color(if active {
                    ui.accent
                } else {
                    gpui_kit::transparent_black()
                })
                .text_size(px(FONT * z))
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .text_color(if active { fg } else { ui.title_dim })
                .cursor_pointer()
                .hover(move |st| st.text_color(fg))
                .child(Icon::new(icon).size(px(13.0 * z)))
                .child(tab.title())
                .when(tab == Tab::Changes && changed > 0, |el| {
                    el.child(
                        div()
                            .px(px(6.0 * z))
                            .rounded_full()
                            .bg(ui.accent.opacity(0.16))
                            .text_size(px(10.0 * z))
                            .text_color(ui.accent)
                            .child(changed.to_string()),
                    )
                })
                .when(tab == Tab::Stashes, |el| {
                    el.tooltip(|window, cx| Tooltip::new(words::tip("Stash")).build(window, cx))
                })
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.set_git_tab(tab, window, cx)
                }))
                .test_support()
        }));
    let area: AnyElement = match git.tab {
        Tab::Changes => changes(ws, git, z, cx),
        Tab::History => history(ws, git, z, cx),
        Tab::Branches => branches(ws, git, z, cx),
        Tab::Stashes => stashes(ws, git, z, cx),
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(tabs)
        .children(banner)
        .when_some(git.error.clone(), |el, e| {
            el.child(note(ui, red, IconName::CircleAlert, z).child(e))
        })
        .child(area)
        .into_any_element()
}

/// A strip under the tabs, in `color`: what holds for the whole repository (a merge under way, a
/// detached HEAD, a failed read).
fn note(ui: Ui, color: Hsla, icon: IconName, z: f32) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(8.0 * z))
        .mx(px(10.0 * z))
        .mt(px(8.0 * z))
        .px(px(10.0 * z))
        .py(px(6.0 * z))
        .rounded(px(8.0 * z))
        .bg(color.opacity(0.12))
        .text_size(px(FONT * z))
        .text_color(ui.title)
        .child(Icon::new(icon).size(px(14.0 * z)).text_color(color))
}
