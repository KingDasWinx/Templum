//! A checklist block: one checklist (`checklists.rs` keeps them, app-wide), drawn from
//! `Workspace::checklists` at the block's zoom: its name, its items with their check boxes, and
//! "+ Add an item". With none, a way to open one. And the Checklists dialog, where every
//! checklist is found, opened and deleted. See docs/specs/checklist.md.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::input::{Backspace, Escape, Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, ElementId, Entity, Focusable as _, FontWeight,
    MouseButton, MouseDownEvent, ScrollWheelEvent, SharedString, TestSupportExt, Window, div, px,
    relative,
};
use templum_canvas::{BlockId, Size, chrome};

use crate::checklists::{self, List};
use crate::kanban::{DragPreview, scrollbar_over};
use crate::list_dialog;
use crate::markdown::PreviewScroll;
use crate::note;
use crate::notes;
use crate::workspace::remote::Deed;
use crate::workspace::{ChecklistAct, Workspace};

/// A new checklist block's size, and the smallest one (world units).
pub(crate) const SIZE: Size = Size { w: 320.0, h: 360.0 };
pub(crate) const MIN: Size = Size { w: 220.0, h: 160.0 };
/// An item's text size and its name's, world units.
const FONT: f32 = 13.0;
const NAME_FONT: f32 = 15.0;
/// A check box's side, world units.
const BOX: f32 = 14.0;

/// A checklist block: the checklist it shows, by id (None: none, as when it was deleted), the
/// one field being edited, and its list's scroll, kept through zooms.
pub(crate) struct Checklist {
    pub list: Option<u64>,
    pub editing: Option<Editing>,
    pub scroll: PreviewScroll,
    /// Of a mirrored workspace: its ids are home's, in `RemoteSpace::checklists`, and what is done
    /// to it is home's tool (`Workspace::act_on_checklist`). No local lookup by id meets it.
    pub remote: bool,
}

/// A name or an item of the block's checklist edited in place, every key written to the
/// checklist as it is typed.
pub(crate) struct Editing {
    pub field: Field,
    pub input: Entity<InputState>,
    /// Its typing is an undo step already: the first key took it (or the add it came with).
    pub kept: bool,
}

/// An item being dragged, by its block, its checklist and its id.
pub(crate) struct ItemDrag {
    pub block: BlockId,
    pub list: u64,
    pub item: u64,
}

/// What a field edits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Field {
    Name,
    Item(u64),
    /// A mirrored workspace's new item, at the end: home adds it once it is typed.
    New,
}

impl Checklist {
    /// Its checklist's title ("Deploy 2/5"); "Checklists" with none.
    pub fn title(&self, ws: &Workspace) -> SharedString {
        match self.list.and_then(|id| ws.checklists_of(self)?.get(id)) {
            Some(list) => list.title().into(),
            None => "Checklists".into(),
        }
    }

    /// The keyboard is in its field.
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.editing.as_ref().is_some_and(|e| {
            e.input
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
        })
    }

    /// The field editing `field`, if it is.
    fn field(&self, field: Field) -> Option<&Entity<InputState>> {
        self.editing
            .as_ref()
            .filter(|e| e.field == field)
            .map(|e| &e.input)
    }
}

fn name(what: &str, n: usize, id: u64) -> ElementId {
    ElementId::Name(format!("{what}-{n}-{id}").into())
}

/// The body below the header at the block's zoom `z`: its checklist, or a way to open one.
pub(crate) fn body(
    ws: &Workspace,
    block: BlockId,
    checklist: &Checklist,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let inner = match checklist
        .list
        .and_then(|id| ws.checklists_of(checklist)?.get(id))
    {
        Some(list) => list_view(ws, block, checklist, list, z, cx),
        None => empty(ws, block, checklist.remote, z, cx),
    };
    div()
        .size_full()
        // The wheel scrolls the checklist and stops there; with Ctrl it zooms the canvas.
        .on_scroll_wheel(|e: &ScrollWheelEvent, _, cx| {
            if !e.modifiers.control {
                cx.stop_propagation();
            }
        })
        .child(inner)
        .into_any_element()
}

/// A checklist: its name, a row per item, then + Add an item; scrolling when it doesn't fit.
fn list_view(
    ws: &Workspace,
    block: BlockId,
    checklist: &Checklist,
    list: &List,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let (ui, n, id) = (ws.ui, block.0 as usize, list.id);
    let why = Why::of(ws, checklist.remote);
    let rows: Vec<AnyElement> = list
        .items
        .iter()
        .map(|item| {
            let input = checklist.field(Field::Item(item.id));
            row(ws, block, id, item, input, &why, z, cx)
        })
        .collect();
    // A mirrored workspace's new item, typed in a row of its own after the others.
    let new = checklist.field(Field::New).map(|input| {
        div()
            .id(("checklist-new-row", n))
            .flex()
            .items_start()
            .gap(px(8.0 * z))
            .py(px(4.0 * z))
            .child(
                div()
                    .flex_shrink_0()
                    .mt(px((FONT * 1.5 - BOX) / 2.0 * z))
                    .size(px(BOX * z))
                    .rounded(px(3.0 * z))
                    .border_1()
                    .border_color(ui.title_dim),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(field(ws, block, input, FONT, z, cx)),
            )
            .test_support()
    });
    let add_words = why.add.clone();
    let add = div()
        .id(("checklist-add", n))
        .flex()
        .items_center()
        .gap(px(8.0 * z))
        .py(px(4.0 * z))
        .text_size(px(FONT * z))
        .text_color(ui.title_dim)
        .cursor_pointer()
        .hover(move |s| s.text_color(ui.title))
        .when(add_words.is_some(), |el| el.opacity(0.4))
        .when_some(add_words, |el, words| {
            el.tooltip(move |window, cx| Tooltip::new(words.clone()).build(window, cx))
        })
        .child(Icon::new(IconName::Plus).size(px(BOX * z)))
        .child("Add an item")
        .drag_over::<ItemDrag>(move |s, _, _, _| s.text_color(ui.accent))
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
            ws.add_item_after(block, id, None, window, cx)
        }))
        .test_support();
    checklist.scroll.follow(z);
    let scrolled = div()
        .id(("checklist-list", n))
        .size_full()
        .overflow_y_scroll()
        .track_scroll(&checklist.scroll.handle)
        .flex()
        .flex_col()
        .px(px(12.0 * z))
        .py(px(8.0 * z))
        .child(
            div()
                .id(("checklist-name", n))
                .pb(px(4.0 * z))
                .text_size(px(NAME_FONT * z))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ui.title)
                .map(|el| match checklist.field(Field::Name) {
                    Some(input) => el.child(field(ws, block, input, NAME_FONT, z, cx)),
                    None => el
                        .line_height(relative(1.5))
                        .cursor_text()
                        .child(list.name_shown().to_string())
                        // A mirrored list's name that can't be renamed now says why.
                        .when_some(why.name.clone(), |el, words| {
                            el.tooltip(move |window, cx| {
                                Tooltip::new(words.clone()).build(window, cx)
                            })
                        })
                        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                            ws.edit_field(block, Field::Name, window, cx)
                        })),
                })
                .test_support(),
        )
        .children(rows)
        .children(new)
        .child(add)
        // Dropped on the list's empty space or on + Add an item: at the end.
        .on_drop(cx.listener(move |ws, drag: &ItemDrag, window, cx| {
            move_item(ws, block, id, drag, None, window, cx)
        }))
        .test_support();
    // The scrollbar goes over the list, not in it: in it, it would scroll away.
    div()
        .relative()
        .size_full()
        // The scrollbar takes every press on its strip, whatever the button, before the block
        // under it: these go ahead of it, in the capture phase. An unpicked block is picked (a
        // picked one isn't again: that would stop its field), and the middle button pans the
        // canvas, as over the rest of the block.
        .capture_any_mouse_down(cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
            if ws.space.focused != Some(block) {
                ws.focus_block(block, window, cx);
            }
            if e.button == MouseButton::Middle {
                ws.begin_pan(e.position);
                cx.stop_propagation();
            }
        }))
        .child(scrolled)
        .child(scrollbar_over(
            &checklist.scroll.handle,
            ("checklist-scrollbar", n).into(),
            z,
        ))
        .into_any_element()
}

/// An item dropped on checklist `list` of `block` goes in the place of item `before` (at the
/// end: None), through `act_on_checklist` (home's on a mirror); one of another block or checklist
/// stays where it is.
pub(crate) fn move_item(
    ws: &mut Workspace,
    block: BlockId,
    list: u64,
    drag: &ItemDrag,
    before: Option<u64>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if drag.block == block && drag.list == list {
        let space = ws.space.id.clone();
        let act = ChecklistAct::Move {
            item: drag.item,
            before,
        };
        ws.act_on_checklist(&space, block, act, window, cx);
    }
    cx.stop_propagation();
}

/// An item's row: its check box, its text (struck through and dimmed once checked), and, under
/// the pointer, a × that deletes it.
#[allow(clippy::too_many_arguments)]
fn row(
    ws: &Workspace,
    block: BlockId,
    list: u64,
    item: &checklists::Item,
    input: Option<&Entity<InputState>>,
    why: &Why,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let (ui, n, id, done) = (ws.ui, block.0 as usize, item.id, item.done);
    // Dragged, it follows the pointer as wide as the rows.
    let preview = SharedString::from(item.text.clone());
    let width = ws
        .space
        .canvas
        .get(block)
        .map_or(0.0, |b| chrome::content_rect(b.rect).w - 24.0)
        * z;
    let group = SharedString::from(format!("checklist-row-{n}-{id}"));
    // The box sits on the text's first line.
    let line = FONT * 1.5;
    let check = div()
        .id(name("checklist-box", n, id))
        .flex_shrink_0()
        .mt(px((line - BOX) / 2.0 * z))
        .size(px(BOX * z))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.0 * z))
        .border_1()
        .border_color(if done { ui.accent } else { ui.title_dim })
        .when(done, |el| {
            el.bg(ui.accent).child(
                Icon::new(IconName::Check)
                    .size(px(10.0 * z))
                    .text_color(ui.surface),
            )
        })
        .cursor_pointer()
        .when(why.check.is_some(), |el| el.opacity(0.4))
        .when_some(why.check.clone(), |el, words| {
            el.tooltip(move |window, cx| Tooltip::new(words.clone()).build(window, cx))
        })
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
            let space = ws.space.id.clone();
            ws.act_on_checklist(&space, block, ChecklistAct::Toggle { item: id }, window, cx)
        }))
        .test_support();
    let text = div()
        .id(name("checklist-text", n, id))
        .flex_1()
        .min_w(px(0.0))
        .text_size(px(FONT * z))
        .text_color(if done { ui.title_dim } else { ui.title })
        .map(|el| match input {
            // One line while it is edited: a long item scrolls sideways in its field.
            Some(input) => el.child(field(ws, block, input, FONT, z, cx)),
            None => el
                .line_height(relative(1.5))
                .when(done, |el| el.line_through())
                .cursor_text()
                .child(item.text.clone())
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.edit_field(block, Field::Item(id), window, cx)
                })),
        })
        .test_support();
    let delete = div()
        .id(name("checklist-item-delete", n, id))
        .flex_shrink_0()
        .mt(px((line - 16.0) / 2.0 * z))
        .size(px(16.0 * z))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0 * z))
        .opacity(0.0)
        .group_hover(group.clone(), |s| {
            s.opacity(if why.delete.is_some() { 0.4 } else { 1.0 })
        })
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(
            Icon::new(IconName::Close)
                .size(px(10.0 * z))
                .text_color(ui.title_dim),
        )
        .when_some(why.delete.clone(), |el, words| {
            el.tooltip(move |window, cx| Tooltip::new(words.clone()).build(window, cx))
        })
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
            let space = ws.space.id.clone();
            ws.act_on_checklist(&space, block, ChecklistAct::Delete { item: id }, window, cx)
        }))
        .test_support();
    div()
        .id(ElementId::Name(group.clone()))
        .group(group)
        .flex()
        .items_start()
        .gap(px(8.0 * z))
        .py(px(4.0 * z))
        .rounded(px(4.0 * z))
        .child(check)
        .child(text)
        .child(delete)
        // A tint, not a border: a border's screen pixels would narrow the text at every zoom.
        .drag_over::<ItemDrag>(move |s, _, _, _| s.bg(ui.accent.opacity(0.18)))
        .on_drag(
            ItemDrag {
                block,
                list,
                item: id,
            },
            move |_, _, _, cx| {
                let text = preview.clone();
                cx.new(|_| DragPreview { text, width, z, ui })
            },
        )
        // Dropped on a row, an item takes its place; the list doesn't get the drop too.
        .on_drop(cx.listener(move |ws, drag: &ItemDrag, window, cx| {
            move_item(ws, block, list, drag, Some(id), window, cx)
        }))
        .test_support()
        .into_any_element()
}

/// The field editing a name or an item, in its text's place: one line of the text's `size` at
/// zoom `z`, its padding and height set to scale with the zoom. Esc stops editing; Backspace in an
/// empty item deletes it.
fn field(
    ws: &Workspace,
    block: BlockId,
    input: &Entity<InputState>,
    size: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(0.0))
        // A press in the field is its own: the block would take the keyboard from it.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        // What the field leaves: Esc with nothing of its own to undo, Backspace at its start.
        .on_action(
            cx.listener(move |ws, _: &Escape, window, cx| ws.escape_field(block, window, cx)),
        )
        .on_action(cx.listener(move |ws, _: &Backspace, window, cx| {
            ws.backspace_at_start(block, window, cx)
        }))
        .child(
            // Not `Input::h`: gpui-kit's own, for a multi-line input only.
            Styled::h(Input::new(input), px(size * 1.5 * z))
                .appearance(false)
                .px(px(0.0))
                .py(px(0.0))
                .text_size(px(size * z))
                .line_height(relative(1.5))
                .text_color(ws.ui.title),
        )
        .into_any_element()
}

/// Why a mirrored checklist's controls can't act now, in words (None: they can, and always on this
/// computer): asked once for the list, not for each row.
#[derive(Default)]
struct Why {
    check: Option<String>,
    delete: Option<String>,
    add: Option<String>,
    name: Option<String>,
}

impl Why {
    fn of(ws: &Workspace, remote: bool) -> Why {
        if !remote {
            return Why::default();
        }
        Why {
            check: ws.why_not(Deed::CheckItem),
            delete: ws.why_not(Deed::DeleteItem),
            add: ws.why_not(Deed::AddItems),
            name: ws.why_not(Deed::EditChecklist),
        }
    }
}

/// A block with no checklist: a new one, or one from the dialog. A mirrored one's buttons are
/// home's, dimmed with the rule's words when the mirror can't.
fn empty(
    ws: &Workspace,
    block: BlockId,
    remote: bool,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let n = block.0 as usize;
    let why = remote.then(|| ws.why_not(Deed::OpenChecklist)).flatten();
    let gated = |el: gpui_kit::Stateful<gpui_kit::Div>| {
        let why = why.clone();
        el.when(why.is_some(), |el| el.opacity(0.4))
            .when_some(why, |el, words| {
                el.tooltip(move |window, cx| Tooltip::new(words.clone()).build(window, cx))
            })
    };
    div()
        .id(("checklist-empty", n))
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0 * z))
        .child(
            div()
                .text_size(px(13.0 * z))
                .text_color(ws.ui.title_dim)
                .child("No checklist open here"),
        )
        .child(
            gated(note::button(
                ("checklist-new-here", n),
                IconName::Plus,
                "New checklist",
                ws,
                z,
            ))
            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                ws.new_checklist(Some(block), window, cx)
            }))
            .test_support(),
        )
        .child(
            gated(note::button(
                ("checklist-open-here", n),
                IconName::ListChecks,
                "Open a checklist…",
                ws,
                z,
            ))
            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                if ws.why_not(Deed::OpenChecklist).is_none() {
                    open_checklists(ws.space.id.clone(), Some(block), window, cx)
                }
            }))
            .test_support(),
        )
        .test_support()
        .into_any_element()
}

/// The Checklists dialog of workspace `space`: every checklist, the most recently changed first,
/// and a field that finds them by their name and their items. A click opens one where it is open
/// already, else in the block `into` it was asked from when that shows none, else in a new block
/// (beside `into`). So does New checklist. Its trash deletes it on a second click, from its block
/// too. On a mirror it lists home's checklists the blocks of the workspace show, and New checklist
/// is home's.
pub(crate) fn open_checklists(
    space: String,
    into: Option<BlockId>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let kind = list_dialog::Kind {
        title: "Checklists",
        prefix: "checklist",
        find: "Find a checklist",
        none: "No checklists yet.",
        no_match: "No checklist holds that.",
        new: "New checklist",
    };
    let (listed, opens, deletes, makes) = (space.clone(), space.clone(), space.clone(), space);
    list_dialog::open(
        kind,
        move |ws, query| rows(ws, &listed, query),
        move |ws, id, window, cx| {
            if ws.still_on(&opens, window, cx) {
                ws.open_checklist(id, into, window, cx)
            }
        },
        move |ws, id, window, cx| match ws.space_named(&deletes) {
            Some(s) if s.is_remote() => {
                ws.warn(crate::workspace::remote::NOT_YET.into(), window, cx)
            }
            Some(_) => ws.delete_checklist(id, window, cx),
            None => {}
        },
        move |ws, window, cx| {
            if ws.still_on(&makes, window, cx) {
                ws.new_checklist(into, window, cx)
            }
        },
        window,
        cx,
    );
}

/// The Checklists dialog's rows for `query`, of workspace `space`: each checklist's name, then its
/// progress, when it changed and where it is open ("2/5 · 5 min ago · Open"). A mirror's are home's
/// that its blocks show (when they changed is home's to know).
fn rows(ws: &Workspace, space: &str, query: &str) -> Vec<list_dialog::Row> {
    let now = notes::now();
    let Some(shown_in) = ws.space_named(space) else {
        return Vec::new();
    };
    let remote = shown_in.is_remote();
    let Some(lists) = ws.checklists_in(space) else {
        return Vec::new();
    };
    lists
        .find(query)
        .into_iter()
        .filter_map(|list| {
            let at = if remote {
                let shows = shown_in.blocks.values().any(|b| {
                    b.checklist_view()
                        .is_some_and(|c| c.remote && c.list == Some(list.id))
                });
                shows.then_some(shown_in)
            } else {
                ws.checklist_open_at(list.id).map(|(space, _)| space)
            };
            if remote && at.is_none() {
                return None;
            }
            let progress = match list.progress() {
                (_, 0) => None,
                (done, all) => Some(format!("{done}/{all}")),
            };
            let detail: Vec<String> = [
                progress,
                (!remote).then(|| notes::ago(list.changed, now)),
                list_dialog::open_at(ws, at),
            ]
            .into_iter()
            .flatten()
            .collect();
            Some(list_dialog::Row {
                id: list.id,
                title: list.name_shown().to_string(),
                detail: detail.join(" · "),
                dot: None,
            })
        })
        .collect()
}
