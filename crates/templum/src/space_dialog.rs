//! A workspace's look (its icon, color and image) and the Edit… dialog that chooses it, with
//! its name and folder; the New workspace dialog is the same one, making the workspace. See
//! docs/specs/cockpit.md#6-the-workspaces-screen.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::LazyLock;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClickEvent, Context, ElementId, Entity, FontWeight, Hsla, PathPromptOptions,
    TestSupportExt, Window, div, px,
};

use crate::board::Label;
use crate::grid::hsla;
use crate::session;
use crate::sidebar;
use crate::theme::{Theme, Ui};
use crate::workspace::{Space, Workspace, show_notification};

/// The colors a workspace can take, by name: the cards' labels, and the accent.
pub(crate) const COLORS: [&str; 7] = [
    "red", "yellow", "green", "cyan", "blue", "magenta", "accent",
];

/// How a workspace shows: an image, else an icon, else its initials; its color tints them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Look {
    pub icon: Option<IconName>,
    /// One of COLORS.
    pub color: Option<String>,
    /// The image's file.
    pub image: Option<PathBuf>,
}

/// Every icon Templum embeds, by name ("git-branch"), in the catalog's order.
static ICONS: LazyLock<Vec<(String, IconName)>> = LazyLock::new(|| {
    IconName::ALL
        .iter()
        .map(|i| {
            let path = i.path();
            let name = path.strip_prefix("icons/").unwrap_or(&path);
            (name.strip_suffix(".svg").unwrap_or(name).to_string(), *i)
        })
        .collect()
});

/// The icon named `name`, among those Templum embeds.
pub(crate) fn icon(name: &str) -> Option<IconName> {
    ICONS.iter().find(|(n, _)| n == name).map(|(_, i)| *i)
}

/// An icon's name, as `icon` takes it.
pub(crate) fn icon_name(icon: IconName) -> &'static str {
    ICONS
        .iter()
        .find(|(_, i)| *i == icon)
        .map_or("", |(n, _)| n.as_str())
}

/// A color by its name, in `theme`.
pub(crate) fn tint(color: &str, theme: &Theme) -> Option<Hsla> {
    if color == "accent" {
        return Some(theme.ui.accent);
    }
    let label = Label::ALL.into_iter().find(|l| l.name() == color)?;
    Some(hsla(theme.palette.ansi[label.ansi()], 1.0))
}

/// The icons shown before anything is typed in the dialog's search.
pub(crate) const SUGGESTED: [&str; 24] = [
    "rocket",
    "folder",
    "code",
    "terminal",
    "server",
    "database",
    "globe",
    "bug",
    "flask-conical",
    "book",
    "briefcase",
    "house",
    "heart",
    "star",
    "zap",
    "cloud",
    "cpu",
    "package",
    "wrench",
    "test-tube",
    "gamepad-2",
    "music",
    "camera",
    "coffee",
];

/// The most icons a search shows: the grid is drawn every frame.
const FOUND_MAX: usize = 120;

/// The icons for a search: the suggested ones for none, else those whose name has the words.
fn find_icons(query: &str) -> Vec<IconName> {
    let query = query.trim().to_lowercase().replace(' ', "-");
    if query.is_empty() {
        return SUGGESTED.iter().filter_map(|n| icon(n)).collect();
    }
    ICONS
        .iter()
        .filter(|(n, _)| n.contains(&query))
        .map(|(_, i)| *i)
        .take(FOUND_MAX)
        .collect()
}

/// The Edit… dialog's state: what Save (Create, for a new workspace) applies. Cancel drops it.
pub(crate) struct SpaceDialog {
    /// The workspace edited; empty for the one Create makes.
    id: String,
    /// The New workspace dialog: Create makes the workspace.
    creating: bool,
    /// Where a remote's Connection is read.
    workspace: Entity<Workspace>,
    name: Entity<InputState>,
    search: Entity<InputState>,
    look: Look,
    folder: Option<PathBuf>,
    /// Why the image chosen wasn't taken, or why Save was refused.
    note: Option<String>,
    /// The icons found, and the search they are for.
    found: (String, Vec<IconName>),
    /// A mirror of another computer's workspace: its name is home's, and it has a connection
    /// instead of a folder.
    remote: bool,
    theme: Rc<Theme>,
    ui: Ui,
}

impl SpaceDialog {
    fn pick_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let file = match picked.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(err)) => {
                    let message = format!("Couldn't open the file picker: {err}");
                    eprintln!("templum: {message}");
                    cx.update(|window, cx| {
                        show_notification(
                            window,
                            cx,
                            gpui_kit::component::notification::Notification::warning(message),
                        )
                    })
                    .ok();
                    None
                }
            };
            if let Some(file) = file {
                this.update(cx, |d, cx| d.take_image(file, cx)).ok();
            }
        })
        .detach();
    }

    /// The image chosen: shown in the preview, or refused with why.
    fn take_image(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        self.note = session::image_refused(&file);
        if self.note.is_none() {
            self.look.image = Some(file);
        }
        cx.notify();
    }

    fn pick_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        crate::launcher::pick_folder(
            Rc::new(
                move |folder: &std::path::Path, _: &mut Window, cx: &mut App| {
                    this.update(cx, |d, cx| {
                        d.folder = Some(folder.to_path_buf());
                        cx.notify();
                    });
                    // No block is opened.
                    None
                },
            ),
            window,
            cx,
        );
    }

    fn label(&self, text: &'static str) -> impl IntoElement {
        div()
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(self.ui.title)
            .child(text)
    }

    /// A remote's connection: what it reaches and how, what the share can do, when it ends and
    /// how far it is; and Paste a new code….
    fn connection(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use templum_api::store::Scope;
        let ui = self.ui;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let ws = self.workspace.read(cx);
        let remote = ws
            .spaces()
            .find(|s| s.id == self.id)
            .and_then(|s| s.remote.as_deref());
        let mut rows: Vec<(&'static str, String)> = Vec::new();
        if let Some(remote) = remote {
            let entry = &remote.entry;
            let code = templum_api::code::Code {
                url: entry.url.clone(),
                key: templum_api::code::Key::new(String::new()),
                trust: entry.trust(),
            };
            rows.push(("Address", entry.url.clone()));
            rows.push(("Way in", code.said()));
            if entry.trust_added_authority {
                rows.push(("Trust", "Trusts an authority this computer added".into()));
            }
            if let Some(id) = &entry.access_id {
                rows.push(("Access Client ID", id.clone()));
            }
            if let Some(proxy) = &entry.proxy {
                rows.push(("Proxy", proxy.clone()));
            }
            if let Some(me) = &remote.me {
                rows.push((
                    "The share can",
                    match me.scope {
                        Scope::Full => {
                            "Full: also type and click into terminals, discard Git changes, \
                             delete branches, stashes and automations"
                        }
                        _ => {
                            "Control: watch and work with blocks, cards, checklists, Git and \
                             automations, open agents and hand them tasks, answer requests"
                        }
                    }
                    .into(),
                ));
                rows.push((
                    "Expires",
                    crate::settings::api::expiry_words(me.expires, now),
                ));
            }
            if let Some(latency) = remote.latency {
                rows.push(("Round trip", format!("{} ms", latency.as_millis())));
            }
        }
        let (workspace, id) = (self.workspace.clone(), self.id.clone());
        div()
            .id("space-connection")
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(rows.into_iter().map(|(name, value)| {
                div()
                    .id(ElementId::Name(format!("space-connection-{name}").into()))
                    .flex()
                    .gap(px(8.0))
                    .text_size(px(12.0))
                    .child(
                        div()
                            .w(px(110.0))
                            .flex_shrink_0()
                            .text_color(ui.title_dim)
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .text_color(ui.title)
                            .child(value),
                    )
                    .test_support()
            }))
            .child(
                div().flex().mt(px(4.0)).child(
                    Button::new("space-paste-code")
                        .label("Paste a new code…")
                        .small()
                        .on_click(move |_: &ClickEvent, window, cx| {
                            workspace.update(cx, |ws, cx| {
                                crate::connect_dialog::open_for(ws, &id, window, cx)
                            })
                        }),
                ),
            )
            .test_support()
    }

    fn swatches(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = self.ui;
        let fg = hsla(self.theme.palette.foreground, 1.0);
        let chosen = self.look.color.clone();
        let colors = std::iter::once(None).chain(COLORS.iter().map(|c| Some(c.to_string())));
        div().flex().gap(px(8.0)).children(colors.map(|color| {
            let fill = color.as_deref().and_then(|c| tint(c, &self.theme));
            let on = chosen == color;
            let id = format!("space-color-{}", color.as_deref().unwrap_or("none"));
            div()
                .id(ElementId::Name(id.into()))
                .size(px(22.0))
                .rounded_full()
                .cursor_pointer()
                .border_2()
                .border_color(if on { fg } else { ui.hairline })
                .when_some(fill, |el, fill| el.bg(fill))
                .on_click(cx.listener(move |d, _: &ClickEvent, _, cx| {
                    d.look.color = color.clone();
                    cx.notify();
                }))
                .test_support()
        }))
    }

    fn icons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = self.ui;
        let color = self
            .look
            .color
            .as_deref()
            .and_then(|c| tint(c, &self.theme))
            .unwrap_or(ui.title);
        let chosen = self.look.icon;
        let name = self.name.read(cx).value().to_string();
        let cell = |id: String, on: bool| {
            div()
                .id(ElementId::Name(id.into()))
                .flex()
                .items_center()
                .justify_center()
                .size(px(36.0))
                .rounded(px(8.0))
                .border_2()
                .cursor_pointer()
                .map(move |el| {
                    if on {
                        el.border_color(ui.accent).bg(ui.hover)
                    } else {
                        el.border_color(gpui_kit::transparent_black())
                            .hover(move |s| s.bg(ui.hover))
                    }
                })
        };
        // The first cell is no icon: the initials.
        let none = cell("space-icon-none".into(), chosen.is_none())
            .text_size(px(12.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(ui.title)
            .child(sidebar::initials(&name))
            .on_click(cx.listener(|d, _: &ClickEvent, _, cx| {
                d.look.icon = None;
                cx.notify();
            }))
            .test_support();
        let cells = self.found.1.iter().map(|&i| {
            cell(format!("space-icon-{}", icon_name(i)), chosen == Some(i))
                .child(Icon::new(i).size(px(18.0)).text_color(color))
                .on_click(cx.listener(move |d, _: &ClickEvent, _, cx| {
                    d.look.icon = Some(i);
                    cx.notify();
                }))
                .test_support()
        });
        div()
            .id("space-icons")
            .flex()
            .flex_wrap()
            .gap(px(4.0))
            .max_h(px(168.0))
            .overflow_y_scroll()
            .child(none)
            .children(cells)
    }
}

/// A button with no fill, the pointer's fills the rest of the interface has. (`ghost()` shows the
/// kit's accent under the pointer, which the accent's own text color is not meant to sit on.)
fn quiet(ui: Ui, cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(ui.title)
        .hover(ui.hover)
        .active(ui.pressed)
}

impl Render for SpaceDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = self.ui;
        let name = self.name.read(cx).value().to_string();
        let query = self.search.read(cx).value().to_string();
        if self.found.0 != query {
            self.found = (query.clone(), find_icons(&query));
        }
        let red = hsla(self.theme.palette.ansi[1], 1.0);
        let preview = div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(sidebar::tile_face(&self.look, &name, &self.theme, ui))
            .child(
                div()
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if name.trim().is_empty() {
                        "Workspace".to_string()
                    } else {
                        name.trim().to_string()
                    }),
            );
        let image = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                Button::new("space-image-choose")
                    .label("Choose an image…")
                    .small()
                    .on_click(
                        cx.listener(|d, _: &ClickEvent, window, cx| d.pick_image(window, cx)),
                    ),
            )
            .when(self.look.image.is_some(), |el| {
                el.child(
                    Button::new("space-image-remove")
                        .label("Remove image")
                        .small()
                        .custom(quiet(ui, cx))
                        .on_click(cx.listener(|d, _: &ClickEvent, _, cx| {
                            d.look.image = None;
                            cx.notify();
                        })),
                )
            });
        let folder = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .id("space-folder")
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(12.0))
                    .text_color(ui.title)
                    .child(
                        self.folder
                            .as_ref()
                            .map_or("None: new terminals follow the settings".into(), |f| {
                                f.display().to_string()
                            }),
                    )
                    .test_support(),
            )
            .child(
                Button::new("space-folder-choose")
                    .label("Choose…")
                    .small()
                    .on_click(
                        cx.listener(|d, _: &ClickEvent, window, cx| d.pick_folder(window, cx)),
                    ),
            )
            .when(self.folder.is_some(), |el| {
                el.child(
                    Button::new("space-folder-clear")
                        .label("Clear")
                        .small()
                        .custom(quiet(ui, cx))
                        .on_click(cx.listener(|d, _: &ClickEvent, _, cx| {
                            d.folder = None;
                            cx.notify();
                        })),
                )
            });
        let remote = self.remote;
        let computer = self
            .workspace
            .read(cx)
            .spaces()
            .find(|s| s.id == self.id)
            .and_then(|s| s.remote.as_deref())
            .map(crate::mirror::computer_of);
        div()
            .id("space-dialog")
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(preview)
            .child(self.label("Name"))
            .child(if remote {
                div()
                    .id("space-named-at-home")
                    .text_size(px(12.0))
                    .text_color(ui.title_dim)
                    .child(format!(
                        "named on {}",
                        computer.clone().unwrap_or_else(|| "home".into())
                    ))
                    .test_support()
                    .into_any_element()
            } else {
                Input::new(&self.name).into_any_element()
            })
            .child(self.label("Color"))
            .child(self.swatches(cx))
            .child(self.label("Icon"))
            .child(Input::new(&self.search))
            .child(self.icons(cx))
            .when(!remote, |el| {
                el.child(self.label("Image"))
                    .child(image)
                    .child(self.label("Folder"))
                    .child(folder)
            })
            .when(remote, |el| {
                el.child(self.label("Connection"))
                    .child(self.connection(cx))
            })
            .children(self.note.clone().map(|note| {
                div()
                    .id("space-note")
                    .text_size(px(12.0))
                    .text_color(red)
                    .child(note)
                    .test_support()
            }))
            .test_support()
    }
}

/// Opens the Edit… dialog of workspace `id`: its name selected, every change shown in the
/// preview, applied by Save (or Enter), dropped by Cancel.
pub(crate) fn open(ws: &Workspace, id: &str, window: &mut Window, cx: &mut Context<Workspace>) {
    if let Some(space) = ws.spaces().find(|s| s.id == id) {
        show(ws, Some(space), window, cx);
    }
}

/// Opens the New workspace dialog: a new workspace's name ("Workspace 3") selected, no look and
/// no folder. Create (or Enter) makes the workspace and shows it; Cancel and Esc make nothing.
pub(crate) fn open_new(ws: &Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    show(ws, None, window, cx);
}

/// The dialog, for `space` as it is (none: the one Create makes).
fn show(ws: &Workspace, space: Option<&Space>, window: &mut Window, cx: &mut Context<Workspace>) {
    let creating = space.is_none();
    let (id, initial, look, folder, remote) = match space {
        Some(s) => (
            s.id.clone(),
            s.name.clone(),
            s.look.clone(),
            s.folder.clone(),
            s.is_remote(),
        ),
        None => (
            String::new(),
            ws.next_space_name(),
            Look::default(),
            None,
            false,
        ),
    };
    let name = cx.new(|cx| InputState::new(window, cx).default_value(initial));
    let search = cx.new(|cx| InputState::new(window, cx).placeholder("Find an icon"));
    let workspace = cx.entity();
    let view = cx.new(|cx| {
        // A remote's connection changes under the dialog: it draws what is now.
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        SpaceDialog {
            id,
            creating,
            workspace: workspace.clone(),
            name: name.clone(),
            search: search.clone(),
            look,
            folder,
            note: None,
            found: (String::new(), find_icons("")),
            remote,
            theme: ws.theme.clone(),
            ui: ws.ui,
        }
    });
    for field in [&name, &search] {
        // Weak: the view owns the fields, and the dialog's end drops them all.
        let view = view.downgrade();
        App::subscribe(cx, field, move |_, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                view.update(cx, |_, cx| cx.notify()).ok();
            }
        })
        .detach();
    }
    let shown = view.clone();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (view, workspace) = (view.clone(), workspace.clone());
        let (title, ok) = if creating {
            ("New workspace", "Create")
        } else {
            ("Edit workspace", "Save")
        };
        alert
            .confirm()
            .title(title)
            .w(px(480.0))
            .button_props(DialogButtonProps::default().ok_text(ok))
            .child(shown.clone())
            .on_ok(move |_, window, cx| save(&view, &workspace, window, cx))
    });
    // After opening: the dialog takes the focus when it opens.
    name.update(cx, |input, cx| {
        input.focus(window, cx);
        input.select_all(window, cx);
    });
}

/// Applies the dialog; false (it stays open, saying why) when it can't.
fn save(
    view: &Entity<SpaceDialog>,
    workspace: &Entity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let d = view.read(cx);
    let (id, name, look, folder) = (
        d.id.clone(),
        d.name.read(cx).value().to_string(),
        d.look.clone(),
        d.folder.clone(),
    );
    // A mirror keeps only the look: its name is home's, and it has no folder.
    if d.remote {
        workspace.update(cx, |ws, cx| ws.edit_remote_look(&id, look, window, cx));
        return true;
    }
    let creating = d.creating;
    match workspace.update(cx, |ws, cx| {
        if creating {
            ws.create_space(&name, look, folder, window, cx)
        } else {
            ws.edit_space(&id, &name, look, folder, window, cx)
        }
    }) {
        Ok(()) => true,
        Err(why) => {
            view.update(cx, |d, cx| {
                d.note = Some(why);
                cx.notify();
            });
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_and_colors_are_found_by_name() {
        assert_eq!(icon("git-branch"), Some(IconName::GitBranch));
        assert_eq!(icon_name(IconName::GitBranch), "git-branch");
        assert_eq!(icon("no-such-icon"), None);
        let theme = Theme::new("t", templum_terminal::Palette::default(), None, None);
        assert_eq!(tint("accent", &theme), Some(theme.ui.accent));
        assert!(COLORS.iter().all(|c| tint(c, &theme).is_some()));
        assert_eq!(tint("pink", &theme), None);
    }

    #[test]
    fn the_suggested_icons_all_exist() {
        for name in SUGGESTED {
            assert!(icon(name).is_some(), "{name}");
        }
        assert_eq!(find_icons("").len(), SUGGESTED.len());
        let git = find_icons("git");
        assert!(git.contains(&IconName::GitBranch) && git.contains(&IconName::GitCommitHorizontal));
        assert_eq!(find_icons("a").len(), FOUND_MAX);
        assert!(find_icons("no such icon").is_empty());
    }
}
