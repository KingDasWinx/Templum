//! Opening an AI agent from the canvas's menu: the dialog that asks where it runs, with the
//! workspace's recent folders, the system's folder picker, and a path to type. See
//! docs/specs/agents.md.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Icon, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClickEvent, Context, ElementId, FontWeight, PathPromptOptions, TestSupportExt, Window,
    div, px,
};
use templum_canvas::Point;
use templum_terminal::Agent;

use crate::grid::hsla;
use crate::workspace::{Workspace, show_notification};

/// A typed path: `~` and `~/…` are the home folder.
pub(crate) fn expand(text: &str) -> PathBuf {
    let text = text.trim();
    match (text.strip_prefix('~'), std::env::home_dir()) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            home.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(text),
    }
}

/// A folder as a row shows it: its name, and where it is with the home folder as `~`.
fn label(folder: &Path) -> (String, String) {
    let name = folder.file_name().map_or_else(
        || folder.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let place = match std::env::home_dir().and_then(|h| folder.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => folder.display().to_string(),
    };
    (name, place)
}

/// Asks where `agent` runs, then opens it there with its block's top-left corner at `at`.
/// `installed`: its command was found; if not, the dialog says how to install it.
pub(crate) fn ask_folder(
    ws: &Workspace,
    agent: Agent,
    at: Point,
    installed: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let entity = cx.entity();
    let ui = ws.ui;
    let warn = hsla(ws.theme.palette.ansi[3], 1.0);
    // The terminal's font: the install command is to copy into one.
    let mono = ws.cell.font.family.clone();
    let recent: Vec<PathBuf> = ws
        .space
        .recent
        .iter()
        .filter(|f| f.is_dir())
        .cloned()
        .collect();
    let input =
        cx.new(|cx| InputState::new(window, cx).placeholder("Or type a path, like ~/projects/app"));
    let open = {
        let entity = entity.clone();
        Rc::new(move |folder: &Path, window: &mut Window, cx: &mut App| {
            entity.update(cx, |ws, cx| ws.launch_agent(agent, folder, at, window, cx))
        })
    };
    let typed = {
        let (open, input) = (open.clone(), input.clone());
        move |window: &mut Window, cx: &mut App| -> bool {
            let text = input.read(cx).value().to_string();
            !text.trim().is_empty() && open(&expand(&text), window, cx).is_some()
        }
    };
    let field = input.clone();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let rows = recent.iter().enumerate().map(|(i, folder)| {
            let (name, place) = label(folder);
            let (open, folder) = (open.clone(), folder.clone());
            div()
                .id(ElementId::Name(format!("recent-{i}").into()))
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(8.0))
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .active(move |s| s.bg(ui.pressed))
                .child(
                    Icon::new(IconName::Folder)
                        .size(px(16.0))
                        .text_color(ui.title_dim),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w(px(0.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child(name),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.0))
                                .text_color(ui.title_dim)
                                .child(place),
                        ),
                )
                .on_click(move |_: &ClickEvent, window, cx| {
                    if open(&folder, window, cx).is_some() {
                        window.close_dialog(cx);
                    }
                })
                .test_support()
        });
        let choose = {
            let open = open.clone();
            div()
                .id("choose-folder")
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(ui.hairline)
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .active(move |s| s.bg(ui.pressed))
                .child(Icon::new(IconName::FolderOpen).size(px(16.0)))
                .child(div().text_size(px(13.0)).child("Choose a folder…"))
                .on_click(move |_: &ClickEvent, window, cx| {
                    window.close_dialog(cx);
                    pick_folder(open.clone(), window, cx);
                })
                .test_support()
        };
        let typed = typed.clone();
        alert
            .confirm()
            .title(format!("Open {}", agent.title()))
            .w(px(480.0))
            .button_props(DialogButtonProps::default().ok_text("Open"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .when(!installed, |el| {
                        el.child(
                            div()
                                .px(px(10.0))
                                .py(px(8.0))
                                .rounded(px(8.0))
                                .bg(ui.card)
                                .text_size(px(12.0))
                                .child(div().text_color(warn).child(format!(
                                    "{} isn't installed on this computer. To install it:",
                                    agent.program()
                                )))
                                .child(div().font_family(mono.clone()).child(agent.install())),
                        )
                    })
                    .when(!recent.is_empty(), |el| {
                        el.child(
                            div()
                                .text_size(px(12.0))
                                .text_color(ui.title_dim)
                                .child("Recent in this workspace"),
                        )
                        .children(rows)
                    })
                    .child(choose)
                    .child(Input::new(&field)),
            )
            .on_ok(move |_, window, cx| typed(window, cx))
    });
    input.update(cx, |input, cx| input.focus(window, cx));
}

/// Opens the agent in a folder.
type Open = dyn Fn(&Path, &mut Window, &mut App) -> Option<templum_canvas::BlockId>;

/// The system's folder picker; the chosen folder opens the agent.
fn pick_folder(open: Rc<Open>, window: &mut Window, cx: &mut App) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Open".into()),
    });
    window
        .spawn(cx, async move |cx| {
            match picked.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(folder) = paths.into_iter().next() {
                        cx.update(|window, cx| open(&folder, window, cx)).ok();
                    }
                }
                Ok(Ok(None)) | Err(_) => {}
                // No portal on Linux, for one: typing a path still works.
                Ok(Err(err)) => {
                    let message = format!("Couldn't open the folder picker ({err}); type a path");
                    eprintln!("templum: {message}");
                    cx.update(|window, cx| {
                        show_notification(window, cx, Notification::warning(message))
                    })
                    .ok();
                }
            }
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_paths_expand_the_home_folder() {
        let home = std::env::home_dir().unwrap();
        assert_eq!(expand("~"), home);
        assert_eq!(expand(" ~/code/app "), home.join("code/app"));
        assert_eq!(expand("/srv/site"), PathBuf::from("/srv/site"));
        assert_eq!(
            expand("~other/x"),
            PathBuf::from("~other/x"),
            "another user's: as typed"
        );
    }
}
