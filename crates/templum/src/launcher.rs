//! Opening an AI agent from the canvas's menu: the dialog that asks where it runs, with the
//! workspace's own, home and recent folders, the system's folder picker, and a path to type. See
//! docs/specs/agents.md.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::agents::AgentEntry;
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

use crate::theme::Ui;
use crate::workspace::remote::HomeAgent;
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
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => folder.display().to_string(),
    };
    (name, place)
}

/// Opens something in a folder: Some when it opened, so the dialog closes.
pub(crate) type Open = dyn Fn(&Path, &mut Window, &mut App) -> Option<templum_canvas::BlockId>;

/// Asks where `agent` runs, then opens it there with its block's top-left corner at `at`.
pub(crate) fn ask_folder(
    ws: &Workspace,
    agent: AgentEntry,
    at: Point,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let entity = cx.entity();
    let id = agent.id.clone();
    let open: Rc<Open> = Rc::new(move |folder, window, cx| {
        entity.update(cx, |ws, cx| ws.launch_agent(&id, folder, at, window, cx))
    });
    ask_folder_for(ws, format!("Open {}", agent.title), open, window, cx);
}

/// Asks for a folder: this workspace's own, the home one, the recent ones, the system's picker,
/// or a typed path; `open` gets the choice.
pub(crate) fn ask_folder_for(
    ws: &Workspace,
    title: String,
    open: Rc<Open>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let ui = ws.ui;
    // The workspace's own folder comes first, then the home folder, each once.
    let pinned = ws.space.folder.clone().filter(|f| f.is_dir());
    let home = std::env::home_dir().filter(|h| h.is_dir() && Some(h) != pinned.as_ref());
    let recent: Vec<PathBuf> = ws
        .space
        .recent
        .iter()
        .filter(|f| f.is_dir() && Some(*f) != pinned.as_ref() && Some(*f) != home.as_ref())
        .cloned()
        .collect();
    let input =
        cx.new(|cx| InputState::new(window, cx).placeholder("Or type a path, like ~/projects/app"));
    let typed = {
        let (open, input) = (open.clone(), input.clone());
        move |window: &mut Window, cx: &mut App| -> bool {
            let text = input.read(cx).value().to_string();
            !text.trim().is_empty() && open(&expand(&text), window, cx).is_some()
        }
    };
    let field = input.clone();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let pinned = pinned.as_ref().map(|f| {
            row(
                "workspace-folder",
                f,
                Some("Workspace folder"),
                open.clone(),
                ui,
            )
        });
        let home = home.as_ref().map(|h| {
            row(
                "personal-space",
                h,
                Some("Personal space"),
                open.clone(),
                ui,
            )
        });
        let rows = recent
            .iter()
            .enumerate()
            .map(|(i, folder)| row(format!("recent-{i}"), folder, None, open.clone(), ui));
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
            .title(title.clone())
            .w(px(480.0))
            .button_props(DialogButtonProps::default().ok_text("Open"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .children(pinned)
                    .children(home)
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

/// A folder to click: its name and where it is, and `tag` at the right when it says what the
/// folder is to the workspace. `open` gets it; the dialog closes once it opened.
fn row(
    id: impl Into<ElementId>,
    folder: &Path,
    tag: Option<&'static str>,
    open: Rc<Open>,
    ui: Ui,
) -> impl IntoElement {
    let (name, place) = label(folder);
    let folder = folder.to_path_buf();
    pick_row(id, name, place, tag, ui, move |window, cx| {
        if open(&folder, window, cx).is_some() {
            window.close_dialog(cx);
        }
    })
}

/// A row to click: `name` over `place`, and `tag` at the right; `pick` runs on the click.
fn pick_row(
    id: impl Into<ElementId>,
    name: String,
    place: String,
    tag: Option<&'static str>,
    ui: Ui,
    pick: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
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
                .flex_1()
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
        .when_some(tag, |el, tag| {
            el.child(
                div()
                    .flex_none()
                    .text_size(px(12.0))
                    .text_color(ui.title_dim)
                    .child(tag),
            )
        })
        .on_click(move |_: &ClickEvent, window, cx| pick(window, cx))
        .test_support()
}

/// What a folder chosen in the Open dialog of a mirror does.
type Go = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// What the Open dialog of a mirror shows of its one request: it is asking home, or home refused
/// it and said why.
#[derive(Default)]
struct Asking {
    asking: bool,
    refused: Option<String>,
}

/// Asks where home's agent `agent` runs, on the other computer: its workspace's folder and its
/// terminals' (paths of home's), or a path to type, inside the workspace's folder or whole. Home
/// opens it with its block's top-left corner at `at`; the dialog stays, saying it is asking, until
/// home answers, and closes when it opened (a refusal is said in it).
pub(crate) fn ask_home_folder(
    ws: &Workspace,
    agent: HomeAgent,
    at: Point,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let ui = ws.ui;
    let agent_title = agent.title.clone();
    let computer = ws
        .space
        .remote
        .as_deref()
        .map_or_else(String::new, crate::mirror::computer_of);
    let folders = ws.home_folders();
    let state = Rc::new(RefCell::new(Asking::default()));
    let (entity, space) = (cx.entity(), ws.space.id.clone());
    let go: Go = {
        let state = state.clone();
        Rc::new(move |folder, window, cx| {
            if std::mem::replace(&mut state.borrow_mut().asking, true) {
                return;
            }
            state.borrow_mut().refused = None;
            let answered = state.clone();
            let sent = entity.update(cx, |ws, cx| {
                if ws.space.id != space {
                    return Err("Another workspace is shown now: nothing was done.".into());
                }
                ws.remote_open_agent(
                    &agent.id,
                    folder,
                    at,
                    move |ws, result, window, cx| {
                        answered.borrow_mut().asking = false;
                        match result {
                            Ok(()) => {
                                // The dialog gives the keyboard back to the menu that opened it,
                                // gone by now: the canvas takes it, where the new block waits.
                                window.close_dialog(cx);
                                window.focus(&ws.focus_handle, cx);
                            }
                            Err(sentence) => {
                                answered.borrow_mut().refused = Some(sentence);
                                window.refresh();
                            }
                        }
                    },
                    cx,
                )
            });
            if let Err(sentence) = sent {
                *state.borrow_mut() = Asking {
                    asking: false,
                    refused: Some(sentence),
                };
            }
            window.refresh();
        })
    };
    let input = cx.new(|cx| {
        InputState::new(window, cx).placeholder(format!(
            "A folder on {computer}: inside the workspace's folder, or whole"
        ))
    });
    let typed = {
        let (go, input) = (go.clone(), input.clone());
        move |window: &mut Window, cx: &mut App| {
            let text = input.read(cx).value().to_string();
            if !text.trim().is_empty() {
                go(&text, window, cx);
            }
            // The dialog stays: home's answer closes it.
            false
        }
    };
    let field = input.clone();
    let title = format!("Open {} on {computer}", agent_title);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let rows: Vec<_> = folders
            .iter()
            .enumerate()
            .map(|(i, (path, tag))| {
                let (name, place) = remote_label(path);
                let go = go.clone();
                let path = path.clone();
                pick_row(
                    format!("home-folder-{i}"),
                    name,
                    place,
                    *tag,
                    ui,
                    move |window, cx| go(&path, window, cx),
                )
            })
            .collect();
        let status = {
            let state = state.borrow();
            if state.asking {
                Some(format!("Asking {computer}…"))
            } else {
                state.refused.clone()
            }
        };
        let typed = typed.clone();
        alert
            .confirm()
            .title(title.clone())
            .w(px(480.0))
            .button_props(DialogButtonProps::default().ok_text("Open"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .children(rows)
                    .child(Input::new(&field))
                    .children(
                        status
                            .map(|text| div().text_size(px(12.0)).text_color(ui.title).child(text)),
                    ),
            )
            .on_ok(move |_, window, cx| typed(window, cx))
    });
    input.update(cx, |input, cx| input.focus(window, cx));
}

/// A path of home's computer as a row shows it: its last name over the whole path.
fn remote_label(path: &str) -> (String, String) {
    let name = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(path);
    (name.to_string(), path.to_string())
}

/// The system's folder picker; `open` gets the chosen folder.
pub(crate) fn pick_folder(open: Rc<Open>, window: &mut Window, cx: &mut App) {
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
    fn rows_show_the_home_folder_as_a_tilde() {
        let home = std::env::home_dir().unwrap();
        assert_eq!(label(&home).1, "~");
        assert_eq!(label(&home.join("code/app")).1, "~/code/app");
    }

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
