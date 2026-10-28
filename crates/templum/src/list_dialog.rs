//! The dialog that finds, opens and deletes what is kept app-wide and shown in blocks' tabs: the
//! Notes dialog and the Checklists dialog. A row per entry, the most recently changed first; a
//! field that finds them; a click opens one; its trash deletes it on a second click.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, ElementId, FontWeight, Hsla, MouseButton, TestSupportExt,
    Window, div, px,
};

use crate::grid::hsla;
use crate::note;
use crate::workspace::Workspace;

/// An entry's row: its title, what the line under it says, and its color's dot.
pub(crate) struct Row {
    pub id: u64,
    pub title: String,
    pub detail: String,
    pub dot: Option<Hsla>,
}

#[derive(Clone, Copy)]
/// What a dialog is of: its title, its rows' id prefix (`{prefix}-row-{id}`,
/// `{prefix}-delete-{id}`, `{prefix}s-find`, `{prefix}s-list`, `{prefix}s-new`) and its words.
pub(crate) struct Kind {
    pub title: &'static str,
    pub prefix: &'static str,
    pub find: &'static str,
    pub none: &'static str,
    pub no_match: &'static str,
    pub new: &'static str,
}

/// Opens the dialog of `kind`: `rows` gives the rows a search finds (of what the dialog was opened on,
/// not of whatever is shown by the time it is drawn), `open` opens an entry, `delete`
/// deletes one, `new` adds one. Each closes the dialog first, but `delete`.
pub(crate) fn open(
    kind: Kind,
    rows: impl Fn(&Workspace, &str) -> Vec<Row> + 'static,
    open: impl Fn(&mut Workspace, u64, &mut Window, &mut Context<Workspace>) + Clone + 'static,
    delete: impl Fn(&mut Workspace, u64, &mut Window, &mut Context<Workspace>) + Clone + 'static,
    new: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + Clone + 'static,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let ws = cx.entity();
    let search = cx.new(|cx| InputState::new(window, cx).placeholder(kind.find));
    let field = search.clone();
    {
        let ws = ws.clone();
        App::subscribe(cx, &search, move |_, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                ws.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
    }
    let prefix = kind.prefix;
    let name = move |what: &str, id: u64| ElementId::Name(format!("{prefix}-{what}-{id}").into());
    let ids = move |what: &str| ElementId::Name(format!("{prefix}s-{what}").into());
    // The entry whose trash was clicked once: the next click deletes it.
    let armed: Rc<Cell<Option<u64>>> = Rc::default();
    window.open_alert_dialog(cx, move |alert, _, cx| {
        let query = search.read(cx).value().to_string();
        let ws_now = ws.read(cx);
        let ui = ws_now.ui;
        let red = hsla(ws_now.theme.palette.ansi[1], 1.0);
        let rows: Vec<AnyElement> = rows(ws_now, &query)
            .into_iter()
            .map(|row| {
                let id = row.id;
                let is_armed = armed.get() == Some(id);
                let trash = {
                    let (ws, armed, delete) = (ws.clone(), armed.clone(), delete.clone());
                    div()
                        .id(name("delete", id))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(6.0))
                        .text_size(px(12.0))
                        .text_color(if is_armed { red } else { ui.title_dim })
                        .cursor_pointer()
                        .hover(move |s| s.bg(ui.hover).text_color(red))
                        .child(Icon::new(IconName::Trash).size(px(14.0)))
                        .when(is_armed, |el| el.child("Click again to delete"))
                        // The row behind opens the entry: not on this press.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_: &ClickEvent, window, cx| {
                            if armed.get() == Some(id) {
                                armed.set(None);
                                ws.update(cx, |ws, cx| delete(ws, id, window, cx));
                            } else {
                                armed.set(Some(id));
                                ws.update(cx, |_, cx| cx.notify());
                            }
                        })
                        .test_support()
                };
                let (ws, open) = (ws.clone(), open.clone());
                div()
                    .id(name("row", id))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(ui.hairline)
                    .cursor_pointer()
                    .hover(move |s| s.bg(ui.hover).border_color(ui.hover_border))
                    .when_some(row.dot, |el, color| {
                        el.child(
                            div()
                                .flex_shrink_0()
                                .size(px(10.0))
                                .rounded_full()
                                .bg(color),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(row.title),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(ui.title_dim)
                                    .child(row.detail),
                            ),
                    )
                    .child(trash)
                    .on_click(move |_: &ClickEvent, window, cx| {
                        // First: closing gives the keyboard back to what had it before.
                        window.close_dialog(cx);
                        ws.update(cx, |ws, cx| open(ws, id, window, cx));
                    })
                    .test_support()
                    .into_any_element()
            })
            .collect();
        let none = rows.is_empty().then(|| {
            div()
                .py(px(16.0))
                .text_size(px(13.0))
                .text_color(ui.title_dim)
                .child(if query.trim().is_empty() {
                    kind.none
                } else {
                    kind.no_match
                })
        });
        let add = {
            let (ws, new) = (ws.clone(), new.clone());
            note::button(ids("new"), IconName::Plus, kind.new, ws_now, 1.0).on_click(
                move |_: &ClickEvent, window, cx| {
                    window.close_dialog(cx);
                    ws.update(cx, |ws, cx| new(ws, window, cx));
                },
            )
        };
        alert
            .title(kind.title)
            .w(px(560.0))
            .button_props(DialogButtonProps::default().ok_text("Done"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .id(ids("find"))
                            .child(Input::new(&search))
                            .test_support(),
                    )
                    .child(
                        div()
                            .id(ids("list"))
                            .max_h(px(380.0))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .children(rows)
                            .children(none)
                            .test_support(),
                    )
                    .child(div().flex().child(add.test_support())),
            )
    });
    // After opening: the dialog takes the focus when it opens.
    field.update(cx, |input, cx| input.focus(window, cx));
}

/// Where an entry is open, as a row says it: "Open", "Open in Workspace 2"; None when it isn't.
pub(crate) fn open_at(ws: &Workspace, at: Option<&crate::workspace::Space>) -> Option<String> {
    at.map(|space| {
        if space.id == ws.space.id {
            "Open".to_string()
        } else {
            format!("Open in {}", space.name)
        }
    })
}
