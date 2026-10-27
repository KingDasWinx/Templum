//! A kanban block's body: a board's columns and cards with drag and drop, or a word that it shows
//! none; the Boards dialog, and the dialog of an open card. Everything is drawn at the block's zoom `z`, like a
//! terminal's text. See docs/specs/kanban.md.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Cancel, Confirm};
use gpui_kit::component::input::{
    Input, InputEvent, InputState, Paste, Position, Textarea, TextareaState,
};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarHandle};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, BoxShadow, ClickEvent, ClipboardEntry, ClipboardItem, Context, Div,
    DragMoveEvent, ElementId, Entity, ExternalPaths, FontWeight, Hsla, Image, MouseButton,
    MouseDownEvent, ObjectFit, PathPromptOptions, Render, ScrollHandle, ScrollWheelEvent,
    SharedString, Stateful, TestSupportExt, Window, div, img, point, px, relative,
};
use templum_canvas::{BlockId, EDGE_ZONE, Point, Rect, Size, edge_scroll};

use crate::attachments;
use crate::board::{
    Assignee, Attachment, Board, Boards, Card, Column, Comment, Draft, Label, Priority,
};
use crate::checklists::{self, Item};
use crate::git_block::Check;
use crate::git_block::widgets::{check_box, field, icon_button};
use crate::grid::hsla;
use crate::list_dialog;
use crate::menu::{Gates, Target};
use crate::note;
use crate::notes;
use crate::prompt::{self, Choice};
use crate::theme::Ui;
use crate::workspace::remote::Deed;
use crate::workspace::{BoardAct, Workspace, show_notification};

/// A new kanban block's size, and the smallest one (world units).
pub(crate) const SIZE: Size = Size { w: 900.0, h: 540.0 };
pub(crate) const MIN: Size = Size { w: 420.0, h: 280.0 };
const COLUMN_W: f32 = 260.0;

/// A kanban block: the board it shows, if any. `remote`: a block of a mirrored workspace, whose
/// board is home's, in `RemoteSpace::boards`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Kanban {
    pub board: Option<u64>,
    pub remote: bool,
}

impl Kanban {
    /// The board on screen: none when it was deleted.
    pub fn shown<'a>(&self, ws: &'a Workspace) -> Option<&'a Board> {
        ws.boards_of(self)?.board(self.board?)
    }

    /// The block's title.
    pub fn title(&self, ws: &Workspace) -> SharedString {
        match self.shown(ws) {
            Some(board) => board.name.clone().into(),
            None => "Boards".into(),
        }
    }
}

/// A card being dragged.
struct CardDrag {
    board: u64,
    card: u64,
}

/// A column being dragged by its title.
struct ColumnDrag {
    board: u64,
    column: u64,
}

/// What follows the pointer while dragging: the card or column's title (a checklist's item), at
/// the canvas's zoom.
pub(crate) struct DragPreview {
    pub text: SharedString,
    pub width: f32,
    pub z: f32,
    pub ui: Ui,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (z, ui) = (self.z, self.ui);
        div()
            .w(px(self.width))
            .px(px(10.0 * z))
            .py(px(8.0 * z))
            .rounded(px(8.0 * z))
            .bg(ui.surface)
            .border_1()
            .border_color(ui.accent)
            .shadow(vec![BoxShadow {
                color: ui.shadow,
                offset: point(px(0.0), px(8.0 * z)),
                blur_radius: px(20.0 * z),
                spread_radius: px(0.0),
                inset: false,
            }])
            .text_size(px(13.0 * z))
            .text_color(ui.title)
            .opacity(0.92)
            .child(self.text.clone())
    }
}

/// How fast a drag at `pointer` scrolls a container at `bounds` (both in pixels) along its
/// axis, in world units a second, positive toward the end: the canvas's edge rule measured in
/// world units, so the zone and the speed scale with the zoom `z`. Zero when the pointer is
/// outside the container across the axis (over another column), or more than a zone past its
/// ends: every container hears every drag, another block's too.
pub(crate) fn drag_speed(pointer: Point, bounds: Rect, vertical: bool, z: f32) -> f32 {
    let local = Point::new((pointer.x - bounds.x) / z, (pointer.y - bounds.y) / z);
    let size = Size {
        w: bounds.w / z,
        h: bounds.h / z,
    };
    let ((across, width), (along, length)) = if vertical {
        ((local.x, size.w), (local.y, size.h))
    } else {
        ((local.y, size.h), (local.x, size.w))
    };
    if !(0.0..=width).contains(&across) || !(-EDGE_ZONE..=length + EDGE_ZONE).contains(&along) {
        return 0.0;
    }
    let speed = edge_scroll(local, size);
    if vertical { speed.y } else { speed.x }
}

/// A scroll container that scrolls while a drag holds the pointer near its edge: its handle,
/// the speed the last drag move gave (world units a second), and when it last moved.
#[derive(Default)]
struct EdgeScroll {
    handle: ScrollHandle,
    speed: f32,
    at: Option<Instant>,
}

/// The `EdgeScroll` of the container `key`, kept across frames, moved by this frame's step;
/// asks for the next frame while it scrolls.
fn edge_scroll_state(
    key: impl Into<ElementId>,
    vertical: bool,
    z: f32,
    window: &mut Window,
    cx: &mut App,
) -> Entity<EdgeScroll> {
    let edge = window.use_keyed_state(key, cx, |_, _| EdgeScroll::default());
    let dragging = cx.has_active_drag();
    if step(&edge, vertical, z, dragging, Instant::now(), cx) {
        window.request_animation_frame();
    }
    edge
}

/// One frame of scrolling while a drag is near an edge: the offset moves by the speed × `z` ×
/// the time since the last frame (at most 0.05 s, so a stalled frame doesn't jump; none the
/// first time), within the container's ends. True while it scrolls. A drag that ended, a
/// pointer out of the zone, or an end reached stops it at once.
fn step(
    edge: &Entity<EdgeScroll>,
    vertical: bool,
    z: f32,
    dragging: bool,
    now: Instant,
    cx: &mut App,
) -> bool {
    edge.update(cx, |e, _| {
        if !dragging || e.speed == 0.0 {
            e.speed = 0.0;
            e.at = None;
            return false;
        }
        let dt =
            e.at.map_or(0.0, |at| now.duration_since(at).as_secs_f32().min(0.05));
        e.at = Some(now);
        let (offset, max) = (e.handle.offset(), e.handle.max_offset());
        let by = px(e.speed * z * dt);
        let moved = if vertical {
            point(offset.x, (offset.y - by).clamp(-max.y, px(0.0)))
        } else {
            point((offset.x - by).clamp(-max.x, px(0.0)), offset.y)
        };
        e.handle.set_offset(moved);
        // At an end it stops, asking for no more frames until the pointer moves again.
        dt == 0.0 || moved != offset
    })
}

/// A drag move over a container of `block`: its speed from where the pointer is; none while
/// another block is on top there (every container hears every drag).
fn on_edge<T: 'static>(
    edge: &Entity<EdgeScroll>,
    block: BlockId,
    vertical: bool,
    z: f32,
    cx: &mut Context<Workspace>,
) -> impl Fn(&DragMoveEvent<T>, &mut Window, &mut App) + 'static {
    let edge = edge.clone();
    cx.listener(move |ws, e: &DragMoveEvent<T>, window, cx| {
        let (p, b) = (e.event.position, e.bounds);
        let pointer = Point::new(p.x.into(), p.y.into());
        let bounds = Rect {
            x: b.origin.x.into(),
            y: b.origin.y.into(),
            w: b.size.width.into(),
            h: b.size.height.into(),
        };
        let speed = if ws.block_under(p).is_some_and(|top| top != block) {
            0.0
        } else {
            drag_speed(pointer, bounds, vertical, z)
        };
        edge.update(cx, |e, _| e.speed = speed);
        if speed != 0.0 {
            window.refresh();
        }
    })
}

fn name(prefix: &str, n: usize, id: u64) -> ElementId {
    ElementId::Name(format!("{prefix}-{n}-{id}").into())
}

/// The body below the header: the board, or a word that there is none.
pub(crate) fn body(
    ws: &Workspace,
    block: BlockId,
    kanban: Kanban,
    z: f32,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    // A mirror may refuse what a control does: read once a frame, as the toolbar's.
    let gates = if kanban.remote {
        Gates::of(
            ws,
            &[
                Deed::EditColumn,
                Deed::CreateCard,
                Deed::MoveCard,
                Deed::ShowBoard,
            ],
        )
    } else {
        Gates::default()
    };
    let view = match (ws.boards_of(&kanban), kanban.shown(ws)) {
        (Some(boards), Some(board)) => board_view(ws, boards, &gates, block, board, z, window, cx),
        _ => empty(ws, &gates, block, z, cx),
    };
    // The wheel scrolls what is in the block, and stops there; with Ctrl it zooms the canvas.
    div()
        .size_full()
        .on_scroll_wheel(|e: &ScrollWheelEvent, _, cx| {
            if !e.modifiers.control {
                cx.stop_propagation();
            }
        })
        .child(view)
        .into_any_element()
}

/// A control a mirror may refuse: dimmed, its words its tooltip.
fn gated<T: Styled + StatefulInteractiveElement>(control: T, why: &Option<SharedString>) -> T {
    let Some(why) = why.clone() else {
        return control;
    };
    control
        .opacity(0.5)
        .tooltip(move |window, cx| Tooltip::new(why.clone()).build(window, cx))
}

/// A block with no board: a new one, or one from the dialog.
fn empty(
    ws: &Workspace,
    gates: &Gates,
    block: BlockId,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let why = gates.why(Deed::ShowBoard);
    let n = block.0 as usize;
    div()
        .id(("kanban-empty", n))
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
                .child("No board open here"),
        )
        .child(
            gated(
                note::button(("board-new-here", n), IconName::Plus, "New board", ws, z),
                &why,
            )
            .on_click(cx.listener({
                let why = why.clone();
                move |ws, _: &ClickEvent, window, cx| {
                    if why.is_some() {
                        return;
                    }
                    let space = ws.space.id.clone();
                    ws.act_on_boards(&space, BoardAct::Show { block, board: None }, window, cx)
                }
            }))
            .test_support(),
        )
        .child(
            gated(
                note::button(
                    ("board-open-here", n),
                    IconName::LayoutDashboard,
                    "Open a board…",
                    ws,
                    z,
                ),
                &why,
            )
            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                if why.is_some() {
                    return;
                }
                let space = ws.space.id.clone();
                open_boards(space, block, window, cx)
            }))
            .test_support(),
        )
        .test_support()
        .into_any_element()
}

/// The Boards dialog: every board, and a field that finds them by their name and their cards'
/// titles. A click, or New board, shows the board in the block `into`; its trash deletes it on a
/// second click, and the blocks that showed it show none.
pub(crate) fn open_boards(
    space: String,
    into: BlockId,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let kind = list_dialog::Kind {
        title: "Boards",
        prefix: "board",
        find: "Find a board",
        none: "No boards yet.",
        no_match: "No board holds that.",
        new: "New board",
    };
    let (listed, shows, deletes, makes) = (space.clone(), space.clone(), space.clone(), space);
    list_dialog::open(
        kind,
        move |ws, query| rows(ws, &listed, query),
        move |ws, id, window, cx| {
            let show = BoardAct::Show {
                block: into,
                board: Some(id),
            };
            ws.act_on_boards(&shows, show, window, cx)
        },
        move |ws, id, window, cx| {
            ws.act_on_boards(&deletes, BoardAct::DeleteBoard { board: id }, window, cx)
        },
        move |ws, window, cx| {
            let show = BoardAct::Show {
                block: into,
                board: None,
            };
            ws.act_on_boards(&makes, show, window, cx)
        },
        window,
        cx,
    );
}

/// The Boards dialog's rows for `query`, of the boards of workspace `space` (the one the dialog
/// was opened on): each board's name, then its cards and where it is open ("12 cards · Open").
fn rows(ws: &Workspace, space: &str, query: &str) -> Vec<list_dialog::Row> {
    let Some(boards) = ws.boards_in(space) else {
        return Vec::new();
    };
    let remote = ws.space_named(space).is_some_and(|s| s.is_remote());
    boards
        .find(query)
        .into_iter()
        .map(|board| {
            let cards = match board.card_count() {
                1 => "1 card".to_string(),
                n => format!("{n} cards"),
            };
            // Where a board is open is this computer's shown workspace's: home's ids are not.
            let at = ws
                .board_open_at(board.id)
                .filter(|_| !remote && ws.space.id == space);
            let detail: Vec<String> = [Some(cards), list_dialog::open_at(ws, at)]
                .into_iter()
                .flatten()
                .collect();
            list_dialog::Row {
                id: board.id,
                title: board.name.clone(),
                detail: detail.join(" · "),
                dot: None,
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn board_view(
    ws: &Workspace,
    boards: &Boards,
    gates: &Gates,
    block: BlockId,
    board: &Board,
    z: f32,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let n = block.0 as usize;
    let board_id = board.id;
    let columns: Vec<AnyElement> = board
        .columns
        .iter()
        .map(|column| column_view(ws, boards, gates, n, board_id, column, z, window, cx))
        .collect();
    let edge = edge_scroll_state(("columns-scroll", n), false, z, window, cx);
    let why = gates.why(Deed::EditColumn);
    let add = gated(
        div()
            .id(("add-column", n))
            .flex_shrink_0()
            .w(px(COLUMN_W * z))
            .flex()
            .items_center()
            .gap(px(6.0 * z))
            .px(px(12.0 * z))
            .py(px(10.0 * z))
            .rounded(px(10.0 * z))
            .border_1()
            .border_color(ui.hairline)
            .text_size(px(13.0 * z))
            .text_color(ui.title)
            .cursor_pointer()
            .hover(move |s| s.bg(ui.hover))
            .active(move |s| s.bg(ui.pressed))
            .child(Icon::new(IconName::Plus).size(px(14.0 * z)))
            .child("Add a column"),
        &why,
    )
    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
        if why.is_some() {
            return;
        }
        let space = ws.space.id.clone();
        ws.act_on_boards(&space, BoardAct::AddColumn { board: board_id }, window, cx);
    }))
    .test_support();
    // The columns are the scroll container's own children: GPUI measures what there is to
    // scroll from them.
    div()
        .id(("columns", n))
        .size_full()
        .flex()
        .items_start()
        .gap(px(10.0 * z))
        .p(px(4.0 * z))
        .overflow_x_scroll()
        .track_scroll(&edge.read(cx).handle)
        // A card, or a column by its title, dragged near the left or right scrolls the columns.
        .on_drag_move(on_edge::<CardDrag>(&edge, block, false, z, cx))
        .on_drag_move(on_edge::<ColumnDrag>(&edge, block, false, z, cx))
        .children(columns)
        .child(add)
        .test_support()
        .into_any_element()
}

/// How wide a scrollbar's track is at zoom 1: the strip that takes its presses.
pub(crate) const TRACK: f32 = 16.0;

/// A scrollbar over a list that scrolls on `handle` (a column's cards, the ask_user dialog's
/// body, a terminal's history): gpui-component's, as the notes' and the menus' are, which the
/// theme shows on hover or while scrolling and only once the content overflows. Its track and
/// thumb (the widths gpui-component draws at zoom 1) scale with the zoom `z` like everything in
/// the block: unscaled, the track would be a quarter of a column at 0.25.
pub(crate) fn scrollbar_over(
    handle: &(impl ScrollbarHandle + Clone),
    id: ElementId,
    z: f32,
) -> AnyElement {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .child(scrollbar(handle, z))
        .test_support()
        .into_any_element()
}

/// `scrollbar_over`'s scrollbar, over its parent's bounds.
pub(crate) fn scrollbar(handle: &(impl ScrollbarHandle + Clone), z: f32) -> Scrollbar {
    Scrollbar::vertical(handle)
        .id("thumb")
        .viewport_from_layout()
        .styles(|s| {
            s.track(|t| t.width(px(TRACK * z)))
                .thumb(|t| {
                    t.width(px(6.0 * z))
                        .inset(px(4.0 * z))
                        .min_length(px(48.0 * z))
                })
                .thumb_hover(|t| t.width(px(8.0 * z)))
                .thumb_active(|t| t.width(px(8.0 * z)))
        })
}

#[allow(clippy::too_many_arguments)]
fn column_view(
    ws: &Workspace,
    boards: &Boards,
    gates: &Gates,
    n: usize,
    board: u64,
    column: &Column,
    z: f32,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let col = column.id;
    let title = SharedString::from(column.title.clone());
    let preview_title = title.clone();
    let count = column.cards.len();
    let header = div()
        .id(name("column-title", n, col))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.0 * z))
        .px(px(10.0 * z))
        .pt(px(10.0 * z))
        .pb(px(6.0 * z))
        .cursor_grab()
        .child(
            div()
                .truncate()
                .text_size(px(13.0 * z))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.clone()),
        )
        .child(
            div()
                .px(px(6.0 * z))
                .rounded_full()
                .bg(ui.hover)
                .text_size(px(11.0 * z))
                .text_color(ui.title_dim)
                .child(count.to_string()),
        )
        .on_drag(ColumnDrag { board, column: col }, move |_, _, _, cx| {
            let text = preview_title.clone();
            cx.new(|_| DragPreview {
                text,
                width: COLUMN_W * z,
                z,
                ui,
            })
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                // Its block first, so Ctrl+Z after the menu undoes on the board.
                ws.focus_block(BlockId(n as u64), window, cx);
                let target = Target::Column { board, column: col };
                ws.open_menu(target, e.position, window, cx);
                cx.stop_propagation();
            }),
        );
    let cards: Vec<AnyElement> = column
        .cards
        .iter()
        .map(|card| card_view(ws, boards, n, board, col, card, z, cx))
        .collect();
    let edge = edge_scroll_state(name("cards-scroll", n, col), true, z, window, cx);
    let why = gates.why(Deed::CreateCard);
    let add = gated(
        div()
            .id(name("add-card", n, col))
            .flex()
            .items_center()
            .gap(px(6.0 * z))
            .mx(px(6.0 * z))
            .mb(px(6.0 * z))
            .px(px(8.0 * z))
            .py(px(6.0 * z))
            .rounded(px(6.0 * z))
            .text_size(px(12.0 * z))
            .text_color(ui.title_dim)
            .cursor_pointer()
            .hover(move |s| s.bg(ui.hover).text_color(ui.title))
            .active(move |s| s.bg(ui.pressed))
            .child(Icon::new(IconName::Plus).size(px(13.0 * z)))
            .child(if boards.draft(col).is_some() {
                "Continue draft"
            } else {
                "Add a card"
            }),
        &why,
    )
    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
        if why.is_none() {
            add_card(ws, board, col, window, cx)
        }
    }))
    .test_support();
    div()
        .id(name("column", n, col))
        .flex_shrink_0()
        .w(px(COLUMN_W * z))
        .max_h_full()
        .flex()
        .flex_col()
        .rounded(px(10.0 * z))
        .border_1()
        .border_color(ui.hairline)
        .bg(ui.card)
        .drag_over::<CardDrag>(move |s, _, _, _| s.border_color(ui.accent))
        .drag_over::<ColumnDrag>(move |s, _, _, _| s.border_color(ui.accent))
        // A card dropped on the column's empty space goes to its end.
        .on_drop(cx.listener(move |ws, drag: &CardDrag, window, cx| {
            let space = ws.space.id.clone();
            let act = BoardAct::MoveCard {
                board: drag.board,
                card: drag.card,
                column: col,
                to: None,
            };
            ws.act_on_boards(&space, act, window, cx);
        }))
        .on_drop(cx.listener(move |ws, drag: &ColumnDrag, window, cx| {
            let space = ws.space.id.clone();
            let act = BoardAct::MoveColumn {
                board: drag.board,
                column: drag.column,
                to: col,
            };
            ws.act_on_boards(&space, act, window, cx);
        }))
        // A vertical wheel scrolls the cards, never the columns sideways.
        .on_scroll_wheel(|e: &ScrollWheelEvent, _, cx| {
            let d = e.delta.pixel_delta(px(16.0));
            if !e.modifiers.control && d.y.abs() >= d.x.abs() {
                cx.stop_propagation();
            }
        })
        .child(header)
        .child(
            // The scrollbar goes over the list, not in it: in it, it would scroll away.
            div()
                .relative()
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .flex_col()
                .child(
                    div()
                        .id(name("cards", n, col))
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .track_scroll(&edge.read(cx).handle)
                        // The scrollbar over the list takes every press on its strip, whatever the
                        // button, before the card and the block under it: these go ahead of it, in
                        // the capture phase. The block is picked, and the middle button pans the
                        // canvas as it does over the rest of the block. A right-click opens the
                        // card's menu from the card (`card_view`).
                        .capture_any_mouse_down(cx.listener(
                            move |ws, e: &MouseDownEvent, window, cx| {
                                ws.focus_block(BlockId(n as u64), window, cx);
                                if e.button == MouseButton::Middle {
                                    ws.begin_pan(e.position);
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        // A card dragged near the top or bottom of the list scrolls it.
                        .on_drag_move(on_edge::<CardDrag>(&edge, BlockId(n as u64), true, z, cx))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.0 * z))
                                .px(px(6.0 * z))
                                .pb(px(6.0 * z))
                                .children(cards),
                        )
                        .test_support(),
                )
                .child(scrollbar_over(
                    &edge.read(cx).handle,
                    name("scrollbar", n, col),
                    z,
                )),
        )
        .child(add)
        .test_support()
        .into_any_element()
}

/// Who `assignee` is on a card of workspace `space`, as the card and its dialog show it: a
/// block's title (its id when no workspace has it), or "Any Codex". On a mirrored workspace the
/// block and the agent are home's.
pub(crate) fn assignee_label(ws: &Workspace, space: &str, assignee: &Assignee, cx: &App) -> String {
    let home = ws.space_named(space).and_then(|s| s.remote.as_deref());
    match assignee {
        Assignee::Block(uid) => ws
            .uid_title_in(space, uid, cx)
            .unwrap_or_else(|| uid.clone()),
        Assignee::Agent(id) => match home {
            Some(remote) => format!(
                "Any {}",
                remote
                    .agents
                    .iter()
                    .flatten()
                    .find(|a| a.id == *id)
                    .map_or(id.as_str(), |a| a.title.as_str())
            ),
            None => format!("Any {}", ws.agent_title(id)),
        },
    }
}

/// A priority's signal icon, in its color.
fn priority_icon(ws: &Workspace, priority: Priority, size: f32) -> Icon {
    let palette = ws.theme.palette;
    let (icon, color) = match priority {
        Priority::Low => (IconName::SignalLow, ws.ui.title_dim),
        Priority::Medium => (IconName::SignalMedium, hsla(palette.ansi[3], 1.0)),
        Priority::High => (IconName::SignalHigh, hsla(palette.ansi[1], 1.0)),
    };
    Icon::new(icon).size(px(size)).text_color(color)
}

/// A card's meta row, on every card: its id, priority, checklist, description, comments and,
/// at the right, who it is for.
fn meta_row(
    ws: &Workspace,
    boards: &Boards,
    n: usize,
    card: &Card,
    z: f32,
    cx: &App,
) -> impl IntoElement {
    let ui = ws.ui;
    let id = card.id;
    let (done, all) = card.progress();
    let count = card.comments.len();
    let icon = |name: IconName| Icon::new(name).size(px(12.0 * z)).text_color(ui.title_dim);
    div()
        .flex()
        .items_center()
        .gap(px(8.0 * z))
        .text_size(px(11.0 * z))
        .text_color(ui.title_dim)
        .child(
            div()
                .id(name("card-id", n, id))
                .child(format!("#{id}"))
                .test_support(),
        )
        .when_some(card.priority, |el, p| {
            el.child(
                div()
                    .id(name("priority", n, id))
                    .flex()
                    .child(priority_icon(ws, p, 12.0 * z))
                    .test_support(),
            )
        })
        .when(all > 0, |el| {
            let color = if done == all { ui.accent } else { ui.title_dim };
            el.child(
                div()
                    .id(name("checklist", n, id))
                    .flex()
                    .items_center()
                    .gap(px(3.0 * z))
                    .text_color(color)
                    .child(
                        Icon::new(IconName::ListChecks)
                            .size(px(12.0 * z))
                            .text_color(color),
                    )
                    .child(format!("{done}/{all}"))
                    .test_support(),
            )
        })
        .when(!card.description.is_empty(), |el| {
            el.child(icon(IconName::FileText))
        })
        .when(!card.attachments.is_empty(), |el| {
            el.child(
                div()
                    .id(name("attachments", n, id))
                    .flex()
                    .items_center()
                    .gap(px(3.0 * z))
                    .child(icon(IconName::Paperclip))
                    .child(card.attachments.len().to_string())
                    .test_support(),
            )
        })
        .when(boards.draft(id).is_some(), |el| {
            el.child(
                div()
                    .id(name("draft", n, id))
                    .flex()
                    .child(icon(IconName::Pencil))
                    .tooltip(|window, cx| Tooltip::new("Unsaved draft").build(window, cx))
                    .test_support(),
            )
        })
        .when(count > 0, |el| {
            el.child(
                div()
                    .id(name("comments", n, id))
                    .flex()
                    .items_center()
                    .gap(px(3.0 * z))
                    .child(icon(IconName::MessageSquare))
                    .child(count.to_string())
                    .test_support(),
            )
        })
        .when_some(card.assignee.as_ref(), |el, a| {
            el.child(
                div()
                    .id(name("assignee", n, id))
                    .ml_auto()
                    .min_w_0()
                    .truncate()
                    .child(assignee_label(ws, &ws.space.id, a, cx))
                    .test_support(),
            )
        })
}

#[allow(clippy::too_many_arguments)]
fn card_view(
    ws: &Workspace,
    boards: &Boards,
    n: usize,
    board: u64,
    column: u64,
    card: &Card,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let palette = ws.theme.palette;
    let id = card.id;
    let title = SharedString::from(card.title.clone());
    let preview_title = title.clone();
    div()
        .id(name("card", n, id))
        .flex()
        .flex_col()
        .gap(px(6.0 * z))
        .px(px(10.0 * z))
        .py(px(8.0 * z))
        .rounded(px(8.0 * z))
        .border_1()
        .border_color(ui.hairline)
        .bg(ui.surface)
        .shadow(vec![BoxShadow {
            color: ui.shadow.opacity(0.35),
            offset: point(px(0.0), px(1.0 * z)),
            blur_radius: px(2.0 * z),
            spread_radius: px(0.0),
            inset: false,
        }])
        .cursor_pointer()
        .hover(move |s| s.border_color(ui.hover_border))
        .when(!card.labels.is_empty(), |el| {
            el.child(
                div()
                    .flex()
                    .gap(px(4.0 * z))
                    .children(card.labels.iter().map(|label| {
                        div()
                            .w(px(32.0 * z))
                            .h(px(6.0 * z))
                            .rounded_full()
                            .bg(hsla(palette.ansi[label.ansi()], 1.0))
                    })),
            )
        })
        .child(div().text_size(px(13.0 * z)).child(title))
        .child(meta_row(ws, boards, n, card, z, cx))
        .drag_over::<CardDrag>(move |s, _, _, _| s.border_color(ui.accent))
        .on_drag(CardDrag { board, card: id }, move |_, _, _, cx| {
            let text = preview_title.clone();
            cx.new(|_| DragPreview {
                text,
                width: (COLUMN_W - 12.0) * z,
                z,
                ui,
            })
        })
        // Dropped on a card, a card takes its place; the column doesn't get the drop too.
        .on_drop(cx.listener(move |ws, drag: &CardDrag, window, cx| {
            let space = ws.space.id.clone();
            let act = BoardAct::MoveCard {
                board: drag.board,
                card: drag.card,
                column,
                to: Some(id),
            };
            ws.act_on_boards(&space, act, window, cx);
            cx.stop_propagation();
        }))
        .on_click(
            cx.listener(move |ws, _: &ClickEvent, window, cx| open_card(ws, board, id, window, cx)),
        )
        // In the capture phase, ahead of a long column's scrollbar (see `column_view`). The list
        // has picked the block by then.
        .capture_any_mouse_down(cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
            if e.button == MouseButton::Right {
                ws.open_menu(Target::Card { board, card: id }, e.position, window, cx);
                cx.stop_propagation();
            }
        }))
        .test_support()
        .into_any_element()
}

/// "New card" at the end of `column`, in a dialog that creates nothing until Done. The column's
/// kept draft, if it has one, is what it opens with.
pub(crate) fn add_card(
    ws: &mut Workspace,
    board: u64,
    column: u64,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let kept = ws
        .boards_in(&ws.space.id)
        .and_then(|b| b.draft(column))
        .cloned();
    let draft = kept.unwrap_or_else(|| Draft {
        at: column,
        new: true,
        base: Card::new(0),
        work: Card::new(0),
    });
    let space = ws.space.id.clone();
    open_draft(ws, space, board, draft, window, cx);
}

/// The open card: what the person changes is kept in the dialog, apart from the board, until Done.
/// A kept draft of the card is what it opens with, over the card as it is now.
pub(crate) fn open_card(
    ws: &mut Workspace,
    board: u64,
    card: u64,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(boards) = ws.boards_in(&ws.space.id) else {
        return;
    };
    let Some(mut base) = boards.card(board, card).cloned() else {
        return;
    };
    // Its comments are sent at once, on the board: they stay out of the work.
    base.comments.clear();
    let mut work = base.clone();
    if let Some(kept) = boards.draft(card) {
        kept.merge_into(&mut work);
    }
    let draft = Draft {
        at: card,
        new: false,
        base,
        work,
    };
    let space = ws.space.id.clone();
    open_draft(ws, space, board, draft, window, cx);
}

/// The dialog of a card of the mirrored workspace `space` again, with the work a refused Done
/// would have lost: a mirror keeps no draft.
pub(crate) fn reopen_card(
    ws: &mut Workspace,
    space: &str,
    board: u64,
    draft: Draft,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    open_draft(ws, space.to_string(), board, draft, window, cx);
}

/// An open card dialog's work, and the same as it was when opened: Esc and a press outside ask
/// only when the person changed something since.
pub(crate) struct Work {
    /// The workspace the dialog was opened on: where its acts go, whatever is shown later.
    space: String,
    now: Draft,
    opened: Draft,
    /// The names of the files being copied in: they show as "Copying…" and count as a change.
    copying: Vec<String>,
    /// How the dialog ended: a copy that ends after it goes where this says.
    outcome: Outcome,
}

/// What became of a card dialog's work.
#[derive(Clone, Copy, PartialEq)]
enum Outcome {
    /// Still open.
    Open,
    /// Done: it reached the card.
    Done,
    /// Save draft: it is a kept draft.
    Kept,
    /// Discard or Delete card: nobody wants it.
    Dropped,
}

/// Whether the card the dialog `open` is on can't take `deed` now, on a mirrored workspace: its
/// words are said once. A card not made yet has no call of home's to refuse, nor has a local one.
fn refused(
    ws: &Entity<Workspace>,
    open: &Open,
    deed: Deed,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let (space, new) = {
        let open = open.borrow();
        (open.space.clone(), open.now.new)
    };
    if new {
        return false;
    }
    ws.update(cx, |ws, cx| match ws.why_not_in(&space, deed) {
        Some(words) => {
            ws.warn(words, window, cx);
            true
        }
        None => false,
    })
}

/// A dialog's work, shared by its fields and buttons, and kept by the workspace while it is open.
pub(crate) type Open = Rc<RefCell<Work>>;

impl Work {
    /// The workspace the dialog was opened on.
    pub(crate) fn space(&self) -> &str {
        &self.space
    }

    /// The work as it is now.
    #[cfg(test)]
    pub(crate) fn draft(&self) -> &Draft {
        &self.now
    }

    /// The work, for a test to change as the dialog's fields would.
    #[cfg(test)]
    pub(crate) fn draft_mut(&mut self) -> &mut Draft {
        &mut self.now
    }

    fn changed(&self) -> bool {
        self.now.work != self.opened.work || !self.copying.is_empty()
    }

    /// Keeps the work as a draft, if the person changed anything: what the window's closing does
    /// to a dialog still open.
    pub(crate) fn keep(&self, boards: &mut Boards) {
        if self.changed() {
            boards.keep_draft(self.now.clone());
        }
    }
}

/// Before a click changes the work of a card that exists: the work is rebased on the card as it
/// is now, so the click acts on what the dialog shows (an agent may have ticked an item or
/// claimed the card meanwhile) and the person's changes stay over it.
fn rebase(ws: &Workspace, open: &Open, board: u64) {
    let mut open = open.borrow_mut();
    if open.now.new {
        return;
    }
    let live = ws
        .boards_in(&open.space)
        .and_then(|b| b.card(board, open.now.at));
    if let Some(live) = live {
        open.now.rebase(live);
        open.opened.rebase(live);
    }
}

/// Closes the card's dialog: its work is no longer the window's to keep.
fn close_card(ws: &Entity<Workspace>, window: &mut Window, cx: &mut App) {
    ws.update(cx, |ws, _| ws.card_work = None);
    window.close_dialog(cx);
}

/// The open card's comments: on the board for a card that exists, in the work of one not created
/// yet. None: the card is gone.
fn thread(ws: &Workspace, open: &Open, board: u64) -> Option<Vec<Comment>> {
    let open_space = open.borrow().space.clone();
    let open = &open.borrow().now;
    if open.new {
        return Some(open.work.comments.clone());
    }
    let card = ws
        .boards_in(&open_space)
        .and_then(|b| b.card(board, open.at))?;
    Some(card.comments.clone())
}

/// A change of the open card's comments.
enum Thread {
    Add(String),
    Edit(u64, String),
    Delete(u64),
    Restore(usize, Comment),
}

/// Changes the open card's comments: at once on the board (a step of Ctrl+Z, or home's call) for
/// a card that exists, in the work of one not created yet.
fn edit_thread(
    ws: &mut Workspace,
    open: &Open,
    board: u64,
    change: Thread,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let (new, card, space) = {
        let open = open.borrow();
        (open.now.new, open.now.at, open.space.clone())
    };
    if new {
        let id = matches!(change, Thread::Add(_)).then(|| ws.fresh_id(&space));
        let work = &mut open.borrow_mut().now.work;
        match change {
            Thread::Add(text) => work.comments.push(Comment {
                id: id.expect("made for an added comment"),
                by: "you".into(),
                text,
                at: notes::now(),
                edited: false,
            }),
            Thread::Edit(id, text) => work.edit_comment(id, &text),
            Thread::Delete(id) => work.comments.retain(|c| c.id != id),
            Thread::Restore(index, comment) => {
                work.restore_comment(index, comment);
            }
        }
        return;
    }
    let act = match change {
        Thread::Add(text) => BoardAct::Comment { board, card, text },
        Thread::Edit(comment, text) => BoardAct::EditComment {
            board,
            card,
            comment,
            text,
        },
        Thread::Delete(comment) => BoardAct::DeleteComment {
            board,
            card,
            comment,
        },
        Thread::Restore(index, comment) => BoardAct::RestoreComment {
            board,
            card,
            index,
            comment,
        },
    };
    ws.act_on_boards(&space, act, window, cx);
}

/// Adds `items` to the open card's checklist, in its work.
fn add_items(ws: &mut Workspace, open: &Open, board: u64, items: &[(String, bool)]) {
    rebase(ws, open, board);
    let space = open.borrow().space.clone();
    let items: Vec<Item> = items
        .iter()
        .map(|(text, done)| Item {
            id: ws.fresh_id(&space),
            text: text.clone(),
            done: *done,
        })
        .collect();
    open.borrow_mut().now.work.checklist.extend(items);
}

/// Done: the work goes to the board, as one step of Ctrl+Z, and its draft goes.
fn finish_card(
    ws: &mut Workspace,
    open: &Open,
    board: u64,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let (space, draft) = {
        let open = open.borrow();
        (open.space.clone(), open.now.clone())
    };
    ws.act_on_boards(&space, BoardAct::Finish { board, draft }, window, cx);
}

/// The id of the card the open dialog is for. A card not created yet has none until its first
/// attachment: one is taken from the boards, and set in the work and in the dialog's start alike
/// (so the id alone is no change). Done gives the card that id, so its files are where they were
/// put.
fn card_id(ws: &mut Workspace, open: &Open) -> u64 {
    let open = &mut *open.borrow_mut();
    if open.now.work.id == 0 {
        let id = ws.fresh_id(&open.space);
        for card in [
            &mut open.now.base,
            &mut open.now.work,
            &mut open.opened.base,
            &mut open.opened.work,
        ] {
            card.id = id;
        }
    }
    open.now.work.id
}

/// What a person puts on a card: a file on disk, or a pasted image.
pub(crate) enum Source {
    File(PathBuf),
    Image(Image),
}

impl Source {
    /// The name it shows while it is copied.
    fn label(&self) -> String {
        match self {
            Source::File(path) => path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into(),
            ),
            Source::Image(image) => format!("Pasted image.{}", image.format.extension()),
        }
    }
}

/// Attaches `from` to the open card: each is copied into the card's folder on the background
/// executor, and then joins the work. All the ways in end here.
pub(crate) fn attach(
    ws: &mut Workspace,
    open: &Open,
    board: u64,
    from: Vec<Source>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(data) = ws.data_dir.clone() else {
        return;
    };
    // Attachments are home's: files dropped, pasted or picked on a mirror's card attach nothing.
    if ws
        .space_named(open.borrow().space())
        .is_none_or(|s| s.is_remote())
    {
        return;
    }
    if from.is_empty() || !ws.card_work.as_ref().is_some_and(|w| Rc::ptr_eq(w, open)) {
        return;
    }
    let card = card_id(ws, open);
    let jobs: Vec<(String, Source)> = from.into_iter().map(|s| (s.label(), s)).collect();
    open.borrow_mut()
        .copying
        .extend(jobs.iter().map(|(label, _)| label.clone()));
    cx.notify();
    let open = open.clone();
    cx.spawn_in(window, async move |this, cx| {
        // The copies run side by side; their results are taken in the order attached.
        let copies: Vec<_> = jobs
            .into_iter()
            .map(|(label, source)| {
                let (data, name) = (data.clone(), label.clone());
                let copy = cx.background_executor().spawn(async move {
                    match source {
                        Source::File(path) => attachments::copy_in(&data, card, &path),
                        Source::Image(image) => {
                            attachments::write_in(&data, card, &name, &image.bytes)
                        }
                    }
                });
                (label, copy)
            })
            .collect();
        let mut refused = Vec::new();
        for (label, copy) in copies {
            let result = copy.await;
            if let Err(why) = &result {
                refused.push(format!("{label} ({why})"));
            }
            this.update(cx, |ws, cx| {
                land(ws, &open, board, card, &label, result);
                cx.notify();
            })
            .ok();
        }
        if !refused.is_empty() {
            let message = format!("Not attached: {}.", refused.join(", "));
            cx.update(|window, cx| show_notification(window, cx, Notification::warning(message)))
                .ok();
        }
    })
    .detach();
}

/// A copy of `card`'s ended: it joins the work of the dialog open on that card; else the kept
/// draft of the card (Save draft); else, after Done, the card itself, as a step of Ctrl+Z of its
/// own. After Discard or Delete card it joins nothing, and the next start deletes the file.
fn land(
    ws: &mut Workspace,
    open: &Open,
    board: u64,
    card: u64,
    label: &str,
    result: Result<Attachment, String>,
) {
    {
        let mut open = open.borrow_mut();
        if let Some(i) = open.copying.iter().position(|c| c == label) {
            open.copying.remove(i);
        }
    }
    let Ok(file) = result else {
        return;
    };
    // The outcome of the dialog that started the copy decides first: a dialog reopened on the
    // card since must not take a file the earlier one let go (Discard) or already handed over
    // (Done: the card has it, and the reopened dialog sees it there).
    let outcome = open.borrow().outcome;
    match outcome {
        Outcome::Dropped => {}
        Outcome::Done => {
            let board = ws.boards.find_card(card).map_or(board, |(b, _)| b);
            ws.edit_boards(|b| b.edit_card(board, card, |c| c.attachments.push(file)));
        }
        Outcome::Open | Outcome::Kept => {
            let current = ws
                .card_work
                .clone()
                .filter(|w| w.borrow().now.work.id == card);
            if let Some(current) = current {
                current.borrow_mut().now.work.attachments.push(file);
            } else if let Some(draft) = ws.boards.drafts.iter_mut().find(|d| d.work.id == card) {
                draft.work.attachments.push(file);
            }
        }
    }
}

/// The image on the clipboard when it holds one and no text that isn't blank: a screenshot is
/// attached; anything with words is pasted into the field as text.
fn pasted_image(item: &ClipboardItem) -> Option<Image> {
    if item.text().is_some_and(|t| !t.trim().is_empty()) {
        return None;
    }
    item.entries().iter().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(image.clone()),
        _ => None,
    })
}

/// Attach: the system's file picker, several files at once. Cancel attaches nothing.
fn pick_files(
    ws: Entity<Workspace>,
    open: Open,
    board: u64,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, window, cx| {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        let (ws, open) = (ws.clone(), open.clone());
        window
            .spawn(cx, async move |cx| {
                let files = match picked.await {
                    Ok(Ok(Some(paths))) => paths,
                    Ok(Ok(None)) | Err(_) => return,
                    Ok(Err(err)) => {
                        let message = format!("Couldn't open the file picker: {err}");
                        eprintln!("templum: {message}");
                        cx.update(|window, cx| {
                            show_notification(window, cx, Notification::warning(message))
                        })
                        .ok();
                        return;
                    }
                };
                ws.update_in(cx, |ws, window, cx| {
                    let from = files.into_iter().map(Source::File).collect();
                    attach(ws, &open, board, from, window, cx)
                })
                .ok();
            })
            .detach();
    }
}

/// The open card's assignee button, showing `who`: its menu gives the card to anyone, one
/// agent session running now and wired to a kanban block of the board, or any session of an
/// agent. A choice is made in the card's work.
fn assignee_menu(
    ws: &Entity<Workspace>,
    open: &Open,
    board: u64,
    current: Option<Assignee>,
    who: String,
) -> impl IntoElement {
    let (ws, open) = (ws.clone(), open.clone());
    Button::new("card-assignee")
        .small()
        .outline()
        .dropdown_caret(true)
        .label(who)
        .dropdown_menu(move |menu, window, cx| {
            let set = |to| give(ws.clone(), open.clone(), board, to);
            let ws_now = ws.read(cx);
            let space = open.borrow().space.clone();
            // On a mirror the choices are home's: its terminals and its agents.
            let mirrored = ws_now.space_named(&space).and_then(|s| s.remote.as_deref());
            let mut sessions = match mirrored {
                Some(remote) => ws_now.home_sessions(remote),
                None => ws_now.agent_sessions(board, cx),
            };
            // The current block first when it runs no agent now (closed, or its agent quit).
            if let Some(Assignee::Block(uid)) = &current
                && !sessions.iter().any(|(s, _)| s == uid)
            {
                let label = assignee_label(ws_now, &space, &Assignee::Block(uid.clone()), cx);
                sessions.insert(0, (uid.clone(), label));
            }
            let agents: Vec<(String, String)> = match mirrored {
                Some(remote) => remote
                    .agents
                    .iter()
                    .flatten()
                    .map(|a| (a.id.clone(), a.title.clone()))
                    .collect(),
                None => ws_now
                    .agents
                    .iter()
                    .map(|a| (a.id.clone(), a.title.clone()))
                    .collect(),
            };
            let mut menu = menu.item(
                PopupMenuItem::new("Anyone")
                    .checked(current.is_none())
                    .on_click(set(None)),
            );
            if !sessions.is_empty() {
                menu = menu.separator().label("Sessions");
                for (uid, label) in sessions {
                    let to = Assignee::Block(uid);
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .checked(current.as_ref() == Some(&to))
                            .on_click(set(Some(to))),
                    );
                }
            }
            let (ws, open, current) = (ws.clone(), open.clone(), current.clone());
            menu.separator()
                .submenu("Any session of", window, cx, move |mut menu, _, _| {
                    for (id, title) in &agents {
                        let to = Assignee::Agent(id.clone());
                        menu = menu.item(
                            PopupMenuItem::new(title.clone())
                                .checked(current.as_ref() == Some(&to))
                                .on_click(give(ws.clone(), open.clone(), board, Some(to))),
                        );
                    }
                    menu
                })
        })
}

/// A paste into "Add an item": several lines are an item each, added at once (the field is
/// one line); a single line is pasted into it as usual.
fn paste_items(
    ws: Entity<Workspace>,
    open: Open,
    board: u64,
) -> impl Fn(&ClipboardItem, &mut Window, &mut App) -> bool + 'static {
    move |clip, _, cx| {
        let Some(text) = clip.text().filter(|t| t.contains('\n')) else {
            return false;
        };
        let items = checklists::parse_items(&text);
        ws.update(cx, |ws, cx| {
            add_items(ws, &open, board, &items);
            cx.notify();
        });
        true
    }
}

/// A click that gives the open card to `to`.
fn give(
    ws: Entity<Workspace>,
    open: Open,
    board: u64,
    to: Option<Assignee>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        ws.update(cx, |ws, cx| {
            rebase(ws, &open, board);
            open.borrow_mut().now.work.assignee = to.clone();
            cx.notify();
        })
    }
}

/// Where a card is, as its dialog says it: "In {column} on {board}". `held`: the column it is in,
/// or the one a new card is for.
fn place(boards: &Boards, board: u64, held: impl Fn(&Column) -> bool) -> String {
    let Some(b) = boards.board(board) else {
        return String::new();
    };
    let column = b
        .columns
        .iter()
        .find(|c| held(c))
        .map_or("", |c| c.title.as_str());
    format!("In {column} on {}", b.name)
}

/// The card dialog's width, and the room at the sides of its header, body and footer. It is the
/// window's dialog: its sizes are pixels at the window's scale, whatever the canvas's zoom.
const DIALOG_W: f32 = 600.0;
const SIDE: f32 = 20.0;

/// A section's header in the card's dialog: its icon and name at the left and, at the right, what
/// it holds.
/// A mirrored card's attachments, by name and size: they are home's, so nothing here draws a
/// thumbnail, opens a file or attaches one (the button stands dimmed, saying where they are).
fn remote_attachments(ui: Ui, card: &Card, computer: &str) -> Div {
    let words = SharedString::from(format!("Attachments are on {computer}: open them there."));
    let n = card.attachments.len();
    let rows = card.attachments.iter().map(|a| {
        div()
            .id(ElementId::Name(format!("attachment-{}", a.name).into()))
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(6.0))
            .py(px(5.0))
            .child(
                Icon::new(IconName::File)
                    .size(px(14.0))
                    .text_color(ui.title_dim),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(13.0))
                    .child(a.name.clone()),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(ui.title_dim)
                    .child(attachments::size_text(a.size)),
            )
            .test_support()
    });
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .child(
                    section(
                        ui,
                        IconName::Paperclip,
                        "Attachments",
                        (n > 0).then(|| n.to_string()),
                    )
                    .flex_1()
                    .pr(px(8.0)),
                )
                .child(
                    div()
                        .id("attach-files")
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .px(px(8.0))
                        .py(px(4.0))
                        .text_size(px(12.0))
                        .text_color(ui.title_dim)
                        .opacity(0.5)
                        .child(Icon::new(IconName::Plus).size(px(14.0)))
                        .child("Attach")
                        .tooltip(move |window, cx| Tooltip::new(words.clone()).build(window, cx))
                        .test_support(),
                ),
        )
        .child(
            div()
                .id("card-attachments")
                .flex()
                .flex_col()
                .children(rows)
                .test_support(),
        )
}

fn section(ui: Ui, icon: IconName, label: &'static str, count: Option<String>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(Icon::new(icon).size(px(13.0)).text_color(ui.title_dim))
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(ui.title)
                .child(label),
        )
        .when_some(count, |el, count| {
            el.child(
                div()
                    .ml_auto()
                    .text_size(px(11.0))
                    .text_color(ui.title)
                    .child(count),
            )
        })
}

/// A property's row in the card's dialog: its icon and name, then what the caller adds.
fn property(ui: Ui, icon: IconName, label: &'static str) -> Div {
    div()
        .min_h(px(32.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .w(px(92.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_size(px(12.0))
                .text_color(ui.title)
                .child(Icon::new(icon).size(px(14.0)).text_color(ui.title_dim))
                .child(label),
        )
}

/// A button of the dialog's footer and comment field: `label` in `color` on `bg`, with an icon
/// before it, if any.
pub(crate) fn pill(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: Option<IconName>,
    bg: Hsla,
    color: Hsla,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.0))
        .px(px(14.0))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .rounded(px(8.0))
        .bg(bg)
        .text_color(color)
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|s| s.opacity(0.85))
        .when_some(icon, |el, icon| {
            el.child(Icon::new(icon).size(px(14.0)).text_color(color))
        })
        .child(label.into())
}

/// One segment of a strip in a dialog (the card's Priority): lit in the theme's accent while `on`.
/// The caller adds what it shows.
pub(crate) fn segment(ui: Ui, id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(24.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(5.0))
        .rounded(px(6.0))
        .text_size(px(12.0))
        .cursor_pointer()
        .map(|el| {
            if on {
                el.bg(ui.accent.opacity(0.16))
                    .text_color(ui.accent)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                el.text_color(ui.title).hover(move |s| s.bg(ui.hover))
            }
        })
}

/// The rounded strip that holds a dialog's segments.
pub(crate) fn strip(ui: Ui) -> Div {
    div()
        .flex()
        .gap(px(2.0))
        .p(px(2.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(ui.hairline)
}

/// A dialog's red ghost button with the trash icon (Delete card, Delete account).
pub(crate) fn danger(id: impl Into<ElementId>, label: &'static str, red: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .rounded(px(8.0))
        .text_size(px(12.5))
        .text_color(red)
        .cursor_pointer()
        .hover(move |s| s.bg(red.opacity(0.12)))
        .child(Icon::new(IconName::Trash).size(px(14.0)).text_color(red))
        .child(label)
}

/// The comment of an open card being edited in its place, by its number, and its field.
type Editing = Rc<RefCell<Option<(u64, Entity<TextareaState>)>>>;
/// Ends that edit: saved (true) or not.
type Finish = Rc<dyn Fn(bool, &mut Window, &mut App)>;
/// Done: false when a mirror's home can't take the work, and the dialog stays open.
type Submit = Rc<dyn Fn(&mut Window, &mut App) -> bool>;
/// The comments the person deleted from an open card, each with its place in the thread, in the
/// order they were deleted: a row stands where each was, with Undo, until the dialog closes.
type Deleted = Rc<RefCell<Vec<(usize, Comment)>>>;

/// Saves what the person typed in the comment being edited, if it holds text, and ends the edit:
/// a dialog that closes, or another comment's pencil, mustn't drop it silently. The same words
/// change nothing.
fn save_draft(
    ws: &Entity<Workspace>,
    editing: &Editing,
    open: &Open,
    board: u64,
    window: &mut Window,
    cx: &mut App,
) {
    let Some((id, input)) = editing.borrow_mut().take() else {
        return;
    };
    let text = input.read(cx).value().trim().to_string();
    if !text.is_empty() {
        ws.update(cx, |ws, cx| {
            edit_thread(ws, open, board, Thread::Edit(id, text), window, cx);
            cx.notify();
        });
    }
}

/// What stands where the person deleted comment `id`: it says so, and Undo (`undo`) puts the
/// comment back.
fn removed_comment(
    id: u64,
    ui: Ui,
    undo: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(ElementId::Name(format!("comment-deleted-{id}").into()))
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(12.0))
        .py(px(6.0))
        .rounded(px(8.0))
        .border_1()
        .border_dashed()
        .border_color(ui.hairline)
        .text_size(px(12.5))
        .text_color(ui.title)
        .child(
            Icon::new(IconName::Trash)
                .size(px(13.0))
                .text_color(ui.title_dim),
        )
        .child("Comment deleted")
        .child(div().text_color(ui.title_dim).child("·"))
        .child(
            div()
                .id(ElementId::Name(format!("comment-undo-{id}").into()))
                .px(px(6.0))
                .py(px(2.0))
                .rounded(px(5.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ui.accent)
                .cursor_pointer()
                .hover(move |s| s.bg(ui.accent.opacity(0.12)))
                .child("Undo")
                .on_click(undo)
                .test_support(),
        )
}

/// A comment in the thread: its author's round mark, then who and when above what was said (its
/// `body`: the bubble, or the field it is edited in). `actions`: the buttons that show at the
/// right of the first line while the pointer is over the comment.
fn comment_view(
    ui: Ui,
    tint: Hsla,
    who: &str,
    when: String,
    actions: Div,
    body: impl IntoElement,
) -> Div {
    let initial = who
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    div()
        .flex()
        .items_start()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .size(px(26.0))
                .rounded_full()
                .bg(tint.opacity(0.16))
                .text_size(px(11.0))
                .font_weight(FontWeight::BOLD)
                .text_color(tint)
                .child(initial),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    // The buttons are laid over the line's right end, so showing them moves nothing.
                    div()
                        .relative()
                        .flex()
                        .items_baseline()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(who.to_string()),
                        )
                        .child(div().text_size(px(11.0)).text_color(ui.title).child(when))
                        .child(actions.absolute().top(px(-4.0)).right_0()),
                )
                .child(body),
        )
}

/// What a comment said, in its bubble: the person's own in the theme's accent.
fn bubble(ui: Ui, mine: bool, text: String) -> Div {
    div()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .bg(if mine {
            ui.accent.opacity(0.10)
        } else {
            ui.card
        })
        .text_size(px(13.0))
        .line_height(relative(1.5))
        .child(text)
}

/// A small icon button of a comment, shown while the pointer is over its `group`.
fn comment_action(
    id: String,
    icon: IconName,
    tip: &'static str,
    group: &SharedString,
    ui: Ui,
) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .flex()
        .items_center()
        .justify_center()
        .size(px(22.0))
        .rounded(px(5.0))
        .opacity(0.0)
        .group_hover(group.clone(), |s| s.opacity(1.0))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(Icon::new(icon).size(px(12.0)).text_color(ui.title_dim))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}

/// Esc or a press outside a card whose work has moved: Save (Done), Save draft (kept aside, off the
/// board) or Discard (the kept draft goes too). Esc in the question goes back to the card.
fn ask_to_leave(
    ws: &Entity<Workspace>,
    open: &Open,
    editing: &Editing,
    board: u64,
    submit: Submit,
    window: &mut Window,
    cx: &mut App,
) {
    let at = open.borrow().now.at;
    // A mirror keeps no draft: Save and Discard only.
    let remote = ws
        .read(cx)
        .space_named(open.borrow().space())
        .is_some_and(|s| s.is_remote());
    let (save, keep, discard) = (ws.clone(), ws.clone(), ws.clone());
    let (open, editing) = (open.clone(), editing.clone());
    let (open_keep, editing_keep, open_discard) = (open.clone(), editing.clone(), open.clone());
    let mut choices = vec![
        Choice {
            id: "leave-save",
            label: "Save".into(),
            primary: true,
            act: Rc::new(move |window, cx| {
                if submit(window, cx) {
                    close_card(&save, window, cx);
                }
            }),
        },
        Choice {
            id: "leave-save-draft",
            label: "Save draft".into(),
            primary: false,
            act: Rc::new(move |window, cx| {
                save_draft(&keep, &editing_keep, &open_keep, board, window, cx);
                open_keep.borrow_mut().outcome = Outcome::Kept;
                keep.update(cx, |ws, cx| {
                    let (space, draft) = {
                        let open = open_keep.borrow();
                        (open.space.clone(), open.now.clone())
                    };
                    ws.act_on_boards(&space, BoardAct::KeepDraft(draft), window, cx);
                });
                close_card(&keep, window, cx);
            }),
        },
        Choice {
            id: "leave-discard",
            label: "Discard".into(),
            primary: false,
            act: Rc::new(move |window, cx| {
                editing.borrow_mut().take();
                open_discard.borrow_mut().outcome = Outcome::Dropped;
                discard.update(cx, |ws, cx| {
                    let space = open_discard.borrow().space.clone();
                    ws.act_on_boards(&space, BoardAct::DropDraft(at), window, cx);
                });
                close_card(&discard, window, cx);
            }),
        },
    ];
    if remote {
        choices.retain(|c| c.id != "leave-save-draft");
    }
    prompt::choose(
        "Save your changes?",
        "A draft is kept aside: the board doesn't change until Done.",
        choices,
        window,
        cx,
    );
}

/// The dialog of a card: `draft`'s work is what its fields show and change.
fn open_draft(
    ws: &mut Workspace,
    space: String,
    board: u64,
    draft: Draft,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let (new, card) = (draft.new, draft.at);
    let opened = draft.work.clone();
    let select = new && draft.work == draft.base;
    let open: Open = Rc::new(RefCell::new(Work {
        space,
        opened: draft.clone(),
        now: draft,
        copying: Vec::new(),
        outcome: Outcome::Open,
    }));
    ws.card_work = Some(open.clone());
    let ws = cx.entity();
    let title = cx.new(|cx| InputState::new(window, cx).default_value(opened.title.clone()));
    let title_focus = title.clone();
    let description = cx.new(|cx| {
        TextareaState::new(window, cx)
            .rows(4)
            .placeholder("Add a description")
            .default_value(opened.description.clone())
    });
    let comment = cx.new(|cx| {
        TextareaState::new(window, cx)
            .rows(2)
            .placeholder("Write a comment")
    });
    let item = cx.new(|cx| InputState::new(window, cx).placeholder("Add an item"));
    // Enter adds what is typed (a line per item) and empties the field; it needs the window.
    {
        let (ws, open) = (ws.clone(), open.clone());
        window
            .subscribe(&item, cx, move |input, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let items = checklists::parse_items(&input.read(cx).value());
                    if items.is_empty() {
                        return;
                    }
                    ws.update(cx, |ws, cx| {
                        add_items(ws, &open, board, &items);
                        cx.notify();
                    });
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                }
            })
            .detach();
    }
    // Subscribed through the app: they need no window. Enter in the title confirms the dialog by
    // itself.
    {
        let open = open.clone();
        App::subscribe(cx, &title, move |input, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                open.borrow_mut().now.work.title = input.read(cx).value().to_string();
            }
        })
        .detach();
    }
    {
        let open = open.clone();
        App::subscribe(cx, &description, move |input, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                open.borrow_mut().now.work.description = input.read(cx).value().to_string();
            }
        })
        .detach();
    }
    // What is typed in the comment field is no change of the card, but it lights Comment up.
    {
        let ws = ws.clone();
        App::subscribe(cx, &comment, move |_, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                ws.update(cx, |_, cx| cx.notify())
            }
        })
        .detach();
    }
    // The comment being edited in its place, and its field: one at a time. It ends with Save
    // (Enter), Cancel (Escape), or its comment being gone.
    let editing: Editing = Rc::default();
    let deleted: Deleted = Rc::default();
    let finish: Finish = {
        let (ws, editing, comment) = (ws.clone(), editing.clone(), comment.clone());
        let open = open.clone();
        Rc::new(move |save, window, cx| {
            let Some((id, input)) = editing.borrow().clone() else {
                return;
            };
            if save {
                let text = input.read(cx).value().trim().to_string();
                // A comment needs text: the trash button takes one away.
                if text.trim().is_empty() {
                    return;
                }
                // The same words are no edit, and no step of Ctrl+Z.
                ws.update(cx, |ws, cx| {
                    edit_thread(ws, &open, board, Thread::Edit(id, text), window, cx)
                });
            }
            *editing.borrow_mut() = None;
            comment.update(cx, |c, cx| c.focus(window, cx));
            ws.update(cx, |_, cx| cx.notify());
        })
    };
    // A card gone meanwhile (an agent, another block, Ctrl+Z) draws its last values.
    let last = RefCell::new((opened.clone(), String::new()));
    // Done: a comment edit in the middle is kept, then the work goes to the board.
    // False: a mirror's home can't take it, and says why; the dialog stays open.
    let submit: Submit = {
        let (ws, editing, open) = (ws.clone(), editing.clone(), open.clone());
        Rc::new(move |window, cx| {
            save_draft(&ws, &editing, &open, board, window, cx);
            let refused = ws.update(cx, |ws, cx| {
                let (space, draft) = {
                    let open = open.borrow();
                    (open.space.clone(), open.now.clone())
                };
                let words = ws.finish_refusal(&space, board, &draft)?;
                ws.warn(words, window, cx);
                Some(())
            });
            if refused.is_some() {
                return false;
            }
            open.borrow_mut().outcome = Outcome::Done;
            ws.update(cx, |ws, cx| {
                finish_card(ws, &open, board, window, cx);
                cx.notify();
            });
            true
        })
    };
    // A dialog, not an alert: a press outside it asks, or closes it.
    window.open_dialog(cx, move |dialog, _, cx| {
        let ws_now = ws.read(cx);
        let ui = ws_now.ui;
        let palette = ws_now.theme.palette;
        let space = open.borrow().space.clone();
        let boards_now = ws_now.boards_in(&space);
        // The card as it is now with the person's changes over it: an agent's changes to what
        // they left alone show at once. A card not created yet is its work.
        let now = if new {
            Some(open.borrow().now.work.clone())
        } else {
            boards_now.and_then(|b| b.card(board, card)).map(|c| {
                let mut c = c.clone();
                open.borrow().now.merge_into(&mut c);
                c
            })
        };
        if let Some(c) = now {
            let held = |col: &Column| {
                if new {
                    col.id == card
                } else {
                    col.cards.iter().any(|x| x.id == card)
                }
            };
            if let Some(boards) = boards_now {
                *last.borrow_mut() = (c, place(boards, board, held));
            }
        }
        let (shown, place) = last.borrow().clone();
        // A comment that is in the thread again (Ctrl+Z, an agent) needs no Undo.
        deleted
            .borrow_mut()
            .retain(|(_, c)| shown.comments.iter().all(|x| x.id != c.id));
        // A change from the dialog is made in the work, and shows at once.
        let edit = {
            let (ws, open) = (ws.clone(), open.clone());
            move |f: Rc<dyn Fn(&mut Card)>| {
                let (ws, open) = (ws.clone(), open.clone());
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    ws.update(cx, |ws, cx| {
                        rebase(ws, &open, board);
                        f(&mut open.borrow_mut().now.work);
                        cx.notify();
                    })
                }
            }
        };
        // The six labels: a ring of the label's color, filled and checked while the card has it.
        let toggles = Label::ALL.map(|label| {
            let on = shown.labels.contains(&label);
            let color = hsla(palette.ansi[label.ansi()], 1.0);
            let name = label.name();
            div()
                .id(ElementId::Name(format!("label-{name}").into()))
                .flex()
                .items_center()
                .justify_center()
                .size(px(22.0))
                .rounded_full()
                .border_2()
                .border_color(color)
                .when(on, |el| {
                    el.bg(color).child(
                        Icon::new(IconName::Check)
                            .size(px(12.0))
                            .text_color(hsla(palette.background, 1.0)),
                    )
                })
                .cursor_pointer()
                .hover(move |s| s.bg(color.opacity(if on { 0.85 } else { 0.25 })))
                .tooltip(move |window, cx| {
                    Tooltip::new(format!("Toggle the {name} label")).build(window, cx)
                })
                .on_click(edit(Rc::new(move |c| c.toggle_label(label))))
                .test_support()
        });
        // The priorities side by side in one rounded strip; the card's own is lit.
        let chips = std::iter::once(None)
            .chain(Priority::ALL.map(Some))
            .map(|p| {
                let word = p.map_or("None", Priority::title);
                segment(
                    ui,
                    ElementId::Name(
                        format!("priority-{}", p.map_or("none", Priority::name)).into(),
                    ),
                    shown.priority == p,
                )
                .when_some(p, |el, p| el.child(priority_icon(ws_now, p, 12.0)))
                .child(word)
                .on_click(edit(Rc::new(move |c| c.priority = p)))
                .test_support()
            });
        let now = notes::now();
        let author = |by: &str| {
            if by == "you" {
                "You".to_string()
            } else if by == templum_agent::ASKER {
                "Templum Agent".to_string()
            } else {
                ws_now.uid_title(by, cx).unwrap_or_else(|| by.to_string())
            }
        };
        let agent_tint = hsla(palette.ansi[5], 1.0);
        // A comment gone meanwhile (an agent, Ctrl+Z) ends its edit.
        if editing
            .borrow()
            .as_ref()
            .is_some_and(|(id, _)| !shown.comments.iter().any(|c| c.id == *id))
        {
            *editing.borrow_mut() = None;
        }
        let editing_now = editing.borrow().clone();
        let comments: Vec<_> = shown
            .comments
            .iter()
            .map(|c| {
                let (id, mine) = (c.id, c.by == "you");
                let tint = if mine { ui.accent } else { agent_tint };
                let mut when = notes::ago(c.at, now);
                if c.edited {
                    when.push_str(" · edited");
                }
                let group = SharedString::from(format!("comment-group-{id}"));
                let field = editing_now.as_ref().filter(|(e, _)| *e == id);
                let mut actions = div().flex().gap(px(2.0));
                if field.is_none() {
                    // The person may delete any comment: the board is theirs, and a row with Undo
                    // stands where it was, as well as Ctrl+Z. They may edit only their own: an
                    // agent's words changed by someone else would still wear its name.
                    if mine {
                        let begin = {
                            let (ws, editing, finish) =
                                (ws.clone(), editing.clone(), finish.clone());
                            let open = open.clone();
                            let text = c.text.clone();
                            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                if refused(&ws, &open, Deed::EditComment, window, cx) {
                                    return;
                                }
                                // Another comment's draft is kept, not dropped.
                                save_draft(&ws, &editing, &open, board, window, cx);
                                let input = cx.new(|cx| {
                                    TextareaState::new(window, cx)
                                        .auto_grow(2, 8)
                                        .submit_on_enter(true)
                                        .default_value(text.clone())
                                });
                                // Enter saves (Shift+Enter adds a line); typing lights Save up.
                                // The field outlives the edit when the dialog closes in the middle of
                                // it: its subscription holds `finish` (and so the field) weakly.
                                let (ws_typing, finish) = (ws.clone(), Rc::downgrade(&finish));
                                window
                                    .subscribe(
                                        &input,
                                        cx,
                                        move |_, event: &InputEvent, window, cx| match event {
                                            InputEvent::PressEnter { shift: false, .. } => {
                                                if let Some(finish) = finish.upgrade() {
                                                    finish(true, window, cx)
                                                }
                                            }
                                            InputEvent::Change => {
                                                ws_typing.update(cx, |_, cx| cx.notify())
                                            }
                                            _ => {}
                                        },
                                    )
                                    .detach();
                                // The caret goes after the last word, as to go on writing.
                                let line = text.lines().count().saturating_sub(1) as u32;
                                let column =
                                    text.lines().last().map_or(0, |l| l.encode_utf16().count());
                                input.update(cx, |input, cx| {
                                    input.set_cursor_position(
                                        Position::new(line, column as u32),
                                        window,
                                        cx,
                                    )
                                });
                                *editing.borrow_mut() = Some((id, input));
                                ws.update(cx, |_, cx| cx.notify());
                            }
                        };
                        actions = actions.child(
                            comment_action(
                                format!("comment-edit-{id}"),
                                IconName::Pencil,
                                "Edit the comment",
                                &group,
                                ui,
                            )
                            .on_click(begin)
                            .test_support(),
                        );
                    }
                    actions = actions.child(
                        comment_action(
                            format!("comment-delete-{id}"),
                            IconName::Trash,
                            "Delete the comment",
                            &group,
                            ui,
                        )
                        .on_click({
                            let (ws, deleted, open) = (ws.clone(), deleted.clone(), open.clone());
                            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                if refused(&ws, &open, Deed::DeleteComment, window, cx) {
                                    return;
                                }
                                ws.update(cx, |ws, cx| {
                                    let thread = thread(ws, &open, board);
                                    let Some((index, comment)) = thread.and_then(|t| {
                                        let i = t.iter().position(|c| c.id == id)?;
                                        Some((i, t[i].clone()))
                                    }) else {
                                        return;
                                    };
                                    edit_thread(ws, &open, board, Thread::Delete(id), window, cx);
                                    // A mirror's deleted comment has no Undo: putting it back
                                    // would change its author and time. Home's Ctrl+Z does.
                                    let mirrored = ws
                                        .space_named(open.borrow().space())
                                        .is_some_and(|s| s.is_remote());
                                    if !mirrored {
                                        deleted.borrow_mut().push((index, comment));
                                    }
                                    cx.notify();
                                })
                            }
                        })
                        .test_support(),
                    );
                }
                let body = match field {
                    None => bubble(ui, mine, c.text.clone()).into_any_element(),
                    Some((_, input)) => {
                        let can_save = !input.read(cx).value().trim().is_empty();
                        let (bg, color) = if can_save {
                            (ui.accent, ui.surface)
                        } else {
                            (ui.hover, ui.title_dim)
                        };
                        let (cancel, save) = (finish.clone(), finish.clone());
                        let escape = finish.clone();
                        div()
                            .id(ElementId::Name(format!("comment-editor-{id}").into()))
                            .flex()
                            .flex_col()
                            .rounded(px(8.0))
                            .border_1()
                            .border_color(ui.accent.opacity(0.6))
                            .bg(ui.card)
                            // Enter saves: it mustn't confirm (close) the dialog. Escape, which the
                            // field hands on as the dialog's cancel, only ends the edit.
                            .on_action(|_: &Confirm, _, cx| cx.stop_propagation())
                            .on_action(move |_: &Cancel, window, cx| {
                                escape(false, window, cx);
                                cx.stop_propagation();
                            })
                            .child(
                                div()
                                    .id(ElementId::Name(format!("comment-edit-field-{id}").into()))
                                    .child(Textarea::new(input).appearance(false).bordered(false))
                                    .test_support(),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(px(6.0))
                                    .px(px(8.0))
                                    .pb(px(8.0))
                                    .child(
                                        div()
                                            .id(ElementId::Name(
                                                format!("comment-cancel-{id}").into(),
                                            ))
                                            .h(px(26.0))
                                            .px(px(10.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(8.0))
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(ui.title)
                                            .cursor_pointer()
                                            .hover(move |s| s.bg(ui.hover))
                                            .child("Cancel")
                                            .on_click(move |_: &ClickEvent, window, cx| {
                                                cancel(false, window, cx)
                                            })
                                            .test_support(),
                                    )
                                    .child(
                                        pill(
                                            ElementId::Name(format!("comment-save-{id}").into()),
                                            "Save",
                                            Some(IconName::Check),
                                            bg,
                                            color,
                                        )
                                        .h(px(26.0))
                                        .px(px(10.0))
                                        .when(!can_save, |el| el.cursor_default())
                                        .on_click(move |_: &ClickEvent, window, cx| {
                                            save(true, window, cx)
                                        })
                                        .test_support(),
                                    ),
                            )
                            .into_any_element()
                    }
                };
                comment_view(ui, tint, &author(&c.by), when, actions, body)
                    .id(ElementId::Name(format!("comment-{id}").into()))
                    .group(group)
                    .test_support()
            })
            .collect();
        // The comments the person deleted stand as a row with Undo where each was.
        let gone = deleted.borrow().clone();
        let len = shown.comments.len();
        let mut live = comments.into_iter();
        let mut comments = Vec::new();
        for at in 0..=len {
            for (_, c) in gone.iter().filter(|(index, _)| (*index).min(len) == at) {
                let id = c.id;
                let (ws, deleted, open) = (ws.clone(), deleted.clone(), open.clone());
                let row = removed_comment(id, ui, move |_, window, cx| {
                    let taken = {
                        let mut gone = deleted.borrow_mut();
                        gone.iter()
                            .position(|(_, c)| c.id == id)
                            .map(|i| gone.remove(i))
                    };
                    if let Some((index, comment)) = taken {
                        ws.update(cx, |ws, cx| {
                            edit_thread(
                                ws,
                                &open,
                                board,
                                Thread::Restore(index, comment),
                                window,
                                cx,
                            );
                            cx.notify();
                        });
                    }
                });
                comments.push(row.test_support());
            }
            comments.extend(live.next());
        }
        let can_send = !comment.read(cx).value().trim().is_empty();
        // A mirror's home may refuse what Done, a comment or Delete card does: dimmed, its words
        // the tooltip. A card not made yet has no call of home's to refuse a comment.
        let why_done = ws_now.why_not_in(
            &space,
            if new {
                Deed::CreateCard
            } else {
                Deed::EditCard
            },
        );
        let why_comment = (!new)
            .then(|| ws_now.why_not_in(&space, Deed::CommentCard))
            .flatten();
        let why_delete = ws_now.why_not_in(&space, Deed::DeleteCard);
        let add = {
            let (ws, comment, open) = (ws.clone(), comment.clone(), open.clone());
            let (bg, color) = if can_send {
                (ui.accent, ui.surface)
            } else {
                (ui.hover, ui.title_dim)
            };
            let tip = SharedString::from(
                why_comment
                    .clone()
                    .unwrap_or_else(|| "Add the comment".to_string()),
            );
            pill("add-comment", "Comment", Some(IconName::ArrowUp), bg, color)
                .h(px(26.0))
                .px(px(10.0))
                .when(!can_send || why_comment.is_some(), |el| el.cursor_default())
                .when(why_comment.is_some(), |el| el.opacity(0.5))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .on_click(move |_: &ClickEvent, window, cx| {
                    let text = comment.read(cx).value().trim().to_string();
                    if text.is_empty() || refused(&ws, &open, Deed::CommentCard, window, cx) {
                        return;
                    }
                    ws.update(cx, |ws, cx| {
                        edit_thread(ws, &open, board, Thread::Add(text), window, cx);
                        cx.notify();
                    });
                    comment.update(cx, |c, cx| c.set_value("", window, cx));
                })
                .test_support()
        };
        // A card that doesn't exist yet can't be deleted: the button throws its work away.
        let delete = {
            let ws = ws.clone();
            let red = hsla(palette.ansi[1], 1.0);
            if new {
                let open = open.clone();
                danger("discard-card", "Discard", red)
                    .on_click(move |_: &ClickEvent, window, cx| {
                        open.borrow_mut().outcome = Outcome::Dropped;
                        ws.update(cx, |ws, cx| {
                            let space = open.borrow().space.clone();
                            ws.act_on_boards(&space, BoardAct::DropDraft(card), window, cx);
                        });
                        close_card(&ws, window, cx);
                    })
                    .test_support()
            } else {
                let open = open.clone();
                let tip = why_delete.clone();
                danger("delete-card", "Delete card", red)
                    .when_some(tip, |el, why| {
                        el.opacity(0.5)
                            .tooltip(move |window, cx| Tooltip::new(why.clone()).build(window, cx))
                    })
                    .on_click(move |_: &ClickEvent, window, cx| {
                        if refused(&ws, &open, Deed::DeleteCard, window, cx) {
                            return;
                        }
                        open.borrow_mut().outcome = Outcome::Dropped;
                        ws.update(cx, |ws, cx| {
                            let space = open.borrow().space.clone();
                            let act = BoardAct::DeleteCard { board, card };
                            ws.act_on_boards(&space, act, window, cx);
                        });
                        close_card(&ws, window, cx);
                    })
                    .test_support()
            }
        };
        let who = shown.assignee.as_ref().map_or_else(
            || "Anyone".to_string(),
            |a| assignee_label(ws_now, &space, a, cx),
        );
        let unassign = shown.assignee.is_some().then(|| {
            icon_button("unassign", IconName::Close, ui, 1.0)
                .tooltip(|window, cx| Tooltip::new("Give it to anyone").build(window, cx))
                .on_click(edit(Rc::new(move |c| c.assignee = None)))
                .test_support()
        });
        let count = shown.comments.len();
        let (done, all) = shown.progress();
        let items = shown.checklist.iter().map(|i| {
            let (id, done) = (i.id, i.done);
            let group = SharedString::from(format!("card-item-row-{id}"));
            let check = check_box(
                ElementId::Name(format!("card-item-box-{id}").into()),
                if done {
                    Check::Checked
                } else {
                    Check::Unchecked
                },
                ui,
                1.0,
            )
            .mt(px((13.0 * 1.5 - 16.0) / 2.0))
            .on_click(edit(Rc::new(move |c| {
                if let Some(i) = c.checklist.iter_mut().find(|i| i.id == id) {
                    i.done = !i.done;
                }
            })))
            .test_support();
            let delete = div()
                .id(ElementId::Name(format!("card-item-delete-{id}").into()))
                .flex_shrink_0()
                .mt(px((13.0 * 1.5 - 18.0) / 2.0))
                .size(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .opacity(0.0)
                .group_hover(group.clone(), |s| s.opacity(1.0))
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .child(
                    Icon::new(IconName::Close)
                        .size(px(11.0))
                        .text_color(ui.title_dim),
                )
                .tooltip(|window, cx| Tooltip::new("Remove the item").build(window, cx))
                .on_click(edit(Rc::new(move |c| c.checklist.retain(|i| i.id != id))))
                .test_support();
            div()
                .id(ElementId::Name(group.clone()))
                .group(group)
                .flex()
                .items_start()
                .gap(px(10.0))
                .px(px(6.0))
                .py(px(5.0))
                .rounded(px(6.0))
                .hover(move |s| s.bg(ui.hover))
                .child(check)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .text_size(px(13.0))
                        .line_height(relative(1.5))
                        .when(done, |el| el.text_color(ui.title_dim).line_through())
                        .child(i.text.clone()),
                )
                .child(delete)
                .test_support()
        });
        let progress = (all > 0).then(|| {
            div()
                .h(px(4.0))
                .mb(px(6.0))
                .rounded_full()
                .bg(ui.hover)
                .child(
                    div()
                        .h_full()
                        .w(relative(done as f32 / all as f32))
                        .rounded_full()
                        .bg(ui.accent),
                )
        });
        // Attachments: thumbnails of the images, then a row each for the other files; a file
        // being copied is a row with no ✕. A click opens a file with the system's app.
        let mirrored = ws_now.space_named(&space).and_then(|s| s.remote.as_deref());
        let files = if let Some(remote) = mirrored {
            Some(remote_attachments(
                ui,
                &shown,
                &crate::mirror::computer_of(remote),
            ))
        } else {
            ws_now.data_dir.as_ref().map(|data| {
                let folder = attachments::dir(data, shown.id);
                let remove = |name: &str| {
                    let name = name.to_string();
                    let rm = edit(Rc::new(move |c| c.attachments.retain(|a| a.name != name)));
                    move |e: &ClickEvent, window: &mut Window, cx: &mut App| {
                        rm(e, window, cx);
                        cx.stop_propagation();
                    }
                };
                let cross = |group: &SharedString, name: &str, corner: bool| {
                    let group = group.clone();
                    let button = div()
                        .id(ElementId::Name(format!("attachment-remove-{name}").into()))
                        .flex_shrink_0()
                        .size(px(18.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(if corner { 9.0 } else { 4.0 }))
                        .opacity(0.0)
                        .group_hover(group, |s| s.opacity(1.0))
                        .cursor_pointer()
                        .hover(move |s| s.bg(ui.hover))
                        .child(
                            Icon::new(IconName::Close)
                                .size(px(11.0))
                                .text_color(ui.title_dim),
                        )
                        .tooltip(|window, cx| {
                            Tooltip::new("Remove the attachment").build(window, cx)
                        })
                        .on_click(remove(name))
                        .test_support();
                    if corner {
                        button.absolute().top(px(4.0)).right(px(4.0)).bg(ui.surface)
                    } else {
                        button
                    }
                };
                let opener = |name: &str| {
                    let path = folder.join(name);
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| cx.open_with_system(&path)
                };
                let item = |a: &Attachment| {
                    let name = a.name.clone();
                    let group = SharedString::from(format!("attachment-group-{name}"));
                    let tip = format!("{name} · {}", attachments::size_text(a.size));
                    let el = div()
                        .id(ElementId::Name(format!("attachment-{name}").into()))
                        .group(group.clone())
                        .cursor_pointer()
                        .on_click(opener(&name));
                    if attachments::thumbnail(a) {
                        let missing = move || {
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(ui.card)
                                .child(
                                    Icon::new(IconName::Image)
                                        .size(px(20.0))
                                        .text_color(ui.title_dim),
                                )
                                .into_any_element()
                        };
                        el.relative()
                            .size(px(72.0))
                            .rounded(px(6.0))
                            .overflow_hidden()
                            .border_1()
                            .border_color(ui.hairline)
                            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                            .child(
                                img(folder.join(&name))
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                                    .with_fallback(missing),
                            )
                            .child(cross(&group, &name, true))
                            .test_support()
                            .into_any_element()
                    } else {
                        el.flex()
                            .items_center()
                            .gap(px(10.0))
                            .px(px(6.0))
                            .py(px(5.0))
                            .rounded(px(6.0))
                            .hover(move |s| s.bg(ui.hover))
                            .child(
                                Icon::new(IconName::File)
                                    .size(px(14.0))
                                    .text_color(ui.title_dim),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_size(px(13.0))
                                    .child(name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(ui.title_dim)
                                    .child(attachments::size_text(a.size)),
                            )
                            .child(cross(&group, &name, false))
                            .test_support()
                            .into_any_element()
                    }
                };
                let (thumbs, rows): (Vec<_>, Vec<_>) = shown
                    .attachments
                    .iter()
                    .partition(|a| attachments::thumbnail(a));
                let copying = open.borrow().copying.clone();
                let copying = copying.iter().map(|name| {
                    div()
                        .id(ElementId::Name(format!("attachment-copying-{name}").into()))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .px(px(6.0))
                        .py(px(5.0))
                        .child(
                            Icon::new(IconName::File)
                                .size(px(14.0))
                                .text_color(ui.title_dim),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(13.0))
                                .child(name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(ui.title_dim)
                                .child("Copying…"),
                        )
                        .test_support()
                });
                let empty = shown.attachments.is_empty() && open.borrow().copying.is_empty();
                let list = if empty {
                    div()
                        .id("card-attachments")
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(6.0))
                        .py(px(14.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_dashed()
                        .border_color(ui.hairline)
                        .text_size(px(12.0))
                        .text_color(ui.title)
                        .child(
                            Icon::new(IconName::Paperclip)
                                .size(px(13.0))
                                .text_color(ui.title_dim),
                        )
                        .child("Drop files here or paste a screenshot")
                } else {
                    div()
                        .id("card-attachments")
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .when(!thumbs.is_empty(), |el| {
                            el.child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.0))
                                    .children(thumbs.into_iter().map(&item)),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .children(rows.into_iter().map(&item))
                                .children(copying),
                        )
                };
                let n = shown.attachments.len();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                section(
                                    ui,
                                    IconName::Paperclip,
                                    "Attachments",
                                    (n > 0).then(|| n.to_string()),
                                )
                                .flex_1()
                                .pr(px(8.0)),
                            )
                            .child(
                                Button::new("attach-files")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Plus)
                                    .label("Attach")
                                    .on_click(pick_files(ws.clone(), open.clone(), board)),
                            ),
                    )
                    .child(list.test_support())
            })
        };
        let thread = if comments.is_empty() {
            div()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .py(px(14.0))
                .rounded(px(8.0))
                .border_1()
                .border_dashed()
                .border_color(ui.hairline)
                .text_size(px(12.0))
                .text_color(ui.title)
                .child(
                    Icon::new(IconName::MessageSquare)
                        .size(px(13.0))
                        .text_color(ui.title_dim),
                )
                .child("No comments yet")
        } else {
            div().flex().flex_col().gap(px(14.0)).children(comments)
        };
        let header = div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(SIDE))
            .pt(px(16.0))
            .pb(px(14.0))
            .border_b_1()
            .border_color(ui.hairline)
            .child(
                div()
                    .id("card-number")
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(px(6.0))
                    .bg(ui.accent.opacity(0.16))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ui.accent)
                    .child(Icon::new(IconName::Kanban).size(px(12.0)))
                    .child(if new {
                        "New card".to_string()
                    } else {
                        format!("Card #{card}")
                    })
                    .test_support(),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(ui.title)
                    .child(place),
            );
        let footer = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px(px(SIDE))
            .py(px(12.0))
            .border_t_1()
            .border_color(ui.hairline)
            .child(delete)
            .child(
                pill("card-done", "Done", None, ui.accent, ui.surface)
                    .when_some(why_done, |el, why| {
                        el.opacity(0.5)
                            .tooltip(move |window, cx| Tooltip::new(why.clone()).build(window, cx))
                    })
                    .on_click({
                        let (ws, submit) = (ws.clone(), submit.clone());
                        move |_, window, cx| {
                            if submit(window, cx) {
                                close_card(&ws, window, cx)
                            }
                        }
                    })
                    .test_support(),
            );
        dialog
            .title(header)
            .close_button(false)
            // Enter in the title confirms the dialog, as Done does. Escape or a press outside it
            // closes it when the work is as it was opened; else it asks. An edit in the middle
            // keeps what was typed.
            .on_ok({
                let (ws, submit) = (ws.clone(), submit.clone());
                move |_, window, cx| {
                    if !submit(window, cx) {
                        return false;
                    }
                    ws.update(cx, |ws, _| ws.card_work = None);
                    true
                }
            })
            .on_cancel({
                let (ws, editing, open, submit) =
                    (ws.clone(), editing.clone(), open.clone(), submit.clone());
                move |_, window, cx| {
                    // A comment being edited in a new card's thread is part of its work.
                    let pending = open.borrow().now.new
                        && editing
                            .borrow()
                            .as_ref()
                            .is_some_and(|(_, field)| !field.read(cx).value().trim().is_empty());
                    if open.borrow().changed() || pending {
                        ask_to_leave(&ws, &open, &editing, board, submit.clone(), window, cx);
                        return false;
                    }
                    save_draft(&ws, &editing, &open, board, window, cx);
                    ws.update(cx, |ws, _| ws.card_work = None);
                    true
                }
            })
            .width(px(DIALOG_W))
            // A block's own color, not the canvas's: the fields and the faint fills read on it.
            .bg(ui.surface)
            .pt(px(0.0))
            .pb(px(0.0))
            .pl(px(0.0))
            .pr(px(0.0))
            .footer(footer)
            .child(
                div()
                    .id("card-body")
                    .flex()
                    .flex_col()
                    .gap(px(22.0))
                    .px(px(SIDE - 2.0))
                    .pt(px(8.0))
                    .pb(px(12.0))
                    // A ring while files are dragged over the card: the border is always there,
                    // clear, so the card doesn't move when it lights.
                    .border_2()
                    .border_color(gpui_kit::transparent_black())
                    .rounded(px(8.0))
                    .drag_over::<ExternalPaths>(move |s, _, _, _| s.border_color(ui.accent))
                    .on_drop({
                        let (ws, open) = (ws.clone(), open.clone());
                        move |dropped: &ExternalPaths, window, cx| {
                            let from = dropped.paths().iter().cloned().map(Source::File);
                            ws.update(cx, |ws, cx| {
                                attach(ws, &open, board, from.collect(), window, cx)
                            });
                        }
                    })
                    // A screenshot on the clipboard is attached, whichever field has the
                    // keyboard: in the capture phase, ahead of any field's own paste.
                    .capture_action({
                        let (ws, open) = (ws.clone(), open.clone());
                        move |_: &Paste, window, cx| {
                            let Some(image) =
                                cx.read_from_clipboard().as_ref().and_then(pasted_image)
                            else {
                                return;
                            };
                            cx.stop_propagation();
                            ws.update(cx, |ws, cx| {
                                let from = vec![Source::Image(image)];
                                attach(ws, &open, board, from, window, cx)
                            });
                        }
                    })
                    .child(
                        div()
                            .id("card-title")
                            .child(Styled::h(
                                Input::new(&title).text_size(px(15.0)).px(px(12.0)),
                                px(38.0),
                            ))
                            .test_support(),
                    )
                    .child(
                        div()
                            .id("card-properties")
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .px(px(12.0))
                            .py(px(6.0))
                            .rounded(px(8.0))
                            .bg(ui.card)
                            .child(
                                property(ui, IconName::Bot, "Assignee")
                                    .child(assignee_menu(
                                        &ws,
                                        &open,
                                        board,
                                        shown.assignee.clone(),
                                        who,
                                    ))
                                    .children(unassign),
                            )
                            .child(
                                property(ui, IconName::SignalHigh, "Priority")
                                    .child(strip(ui).children(chips)),
                            )
                            .child(
                                property(ui, IconName::Tag, "Labels")
                                    .child(div().flex().gap(px(8.0)).children(toggles)),
                            )
                            .test_support(),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .child(section(ui, IconName::FileText, "Description", None))
                            .child(Textarea::new(&description)),
                    )
                    .children(files)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .child(section(
                                ui,
                                IconName::ListChecks,
                                "Checklist",
                                (all > 0).then(|| format!("{done}/{all}")),
                            ))
                            .child(
                                div()
                                    .id("card-checklist")
                                    .flex()
                                    .flex_col()
                                    .children(progress)
                                    .children(items)
                                    .child(
                                        div()
                                            .id("card-item-field")
                                            .mt(px(6.0))
                                            // Enter adds the item: it mustn't confirm (close) the dialog.
                                            .on_action(|_: &Confirm, _, cx| cx.stop_propagation())
                                            .child(
                                                field(&item, 1.0)
                                                    .prefix(
                                                        Icon::new(IconName::Plus)
                                                            .size(px(13.0))
                                                            .text_color(ui.title_dim),
                                                    )
                                                    .on_paste(paste_items(
                                                        ws.clone(),
                                                        open.clone(),
                                                        board,
                                                    )),
                                            )
                                            .test_support(),
                                    )
                                    .test_support(),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.0))
                            .child(section(
                                ui,
                                IconName::MessageSquare,
                                "Comments",
                                (count > 0).then(|| count.to_string()),
                            ))
                            .child(thread)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .rounded(px(8.0))
                                    .border_1()
                                    .border_color(ui.hairline)
                                    .bg(ui.card)
                                    .child(
                                        div()
                                            .id("comment-field")
                                            .child(
                                                Textarea::new(&comment)
                                                    .appearance(false)
                                                    .bordered(false),
                                            )
                                            .test_support(),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .justify_end()
                                            .px(px(8.0))
                                            .pb(px(8.0))
                                            .child(add),
                                    ),
                            ),
                    ),
            )
    });
    // After opening: the dialog takes the focus when it opens.
    title_focus.update(cx, |input, cx| {
        input.focus(window, cx);
        if select {
            input.select_all(window, cx);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use templum_canvas::EDGE_SPEED;

    #[test]
    fn only_an_image_with_no_text_is_a_pasted_image() {
        use gpui_kit::{ClipboardEntry, ClipboardString, ImageFormat};
        let image = Image::from_bytes(ImageFormat::Png, vec![1, 2, 3]);
        let text = |t: &str| ClipboardEntry::String(ClipboardString::new(t.into()));
        let item = |entries| ClipboardItem { entries };
        let image_entry = || ClipboardEntry::Image(image.clone());
        assert_eq!(
            pasted_image(&item(vec![image_entry()])),
            Some(image.clone())
        );
        assert_eq!(pasted_image(&item(vec![image_entry(), text("x")])), None);
        assert_eq!(pasted_image(&item(vec![text("x"), image_entry()])), None);
        assert_eq!(
            pasted_image(&item(vec![image_entry(), text("  \n")])),
            Some(image.clone()),
            "blank text is no text"
        );
        assert_eq!(pasted_image(&item(vec![text("x")])), None);
        assert_eq!(pasted_image(&item(vec![])), None);
    }

    #[test]
    fn a_drag_scrolls_faster_near_an_edge_in_world_units_and_never_across_another_column() {
        let column = Rect {
            x: 100.0,
            y: 50.0,
            w: 260.0,
            h: 400.0,
        };
        let at = Point::new;
        assert_eq!(
            drag_speed(at(230.0, 250.0), column, true, 1.0),
            0.0,
            "the middle"
        );
        let near = drag_speed(at(230.0, 440.0), column, true, 1.0);
        assert!(near > 0.0 && near < EDGE_SPEED);
        assert!(
            drag_speed(at(230.0, 445.0), column, true, 1.0) > near,
            "faster nearer"
        );
        assert_eq!(
            drag_speed(at(230.0, 470.0), column, true, 1.0),
            EDGE_SPEED,
            "just past the edge"
        );
        // Every container hears a drag: one far past its edge (another block's) stays still.
        assert_eq!(
            drag_speed(at(230.0, 600.0), column, true, 1.0),
            0.0,
            "far past the edge"
        );
        assert_eq!(
            drag_speed(at(230.0, 520.0), column, true, 2.0),
            EDGE_SPEED,
            "the zone past it is in world units too"
        );
        assert_eq!(
            drag_speed(at(230.0, 60.0), column, true, 1.0),
            -EDGE_SPEED * 0.75,
            "up"
        );
        assert_eq!(
            drag_speed(at(500.0, 445.0), column, true, 1.0),
            0.0,
            "over another column"
        );
        assert!(
            drag_speed(at(355.0, 250.0), column, false, 1.0) > 0.0,
            "sideways, for the row"
        );
        // The zone is 40 world units: 60 px from the bottom is in it at zoom 2 only.
        let tall = Rect { h: 800.0, ..column };
        assert_eq!(drag_speed(at(230.0, 790.0), tall, true, 1.0), 0.0);
        assert!(drag_speed(at(230.0, 790.0), tall, true, 2.0) > 0.0);
        // A list shorter than both zones (a small block far away) stays still at its middle.
        let short = Rect { h: 60.0, ..column };
        assert_eq!(drag_speed(at(230.0, 80.0), short, true, 1.0), 0.0);
    }

    #[gpui_kit::test]
    fn a_drag_held_at_a_lists_end_or_ended_asks_for_no_more_frames(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.update(|cx| {
            let edge = cx.new(|_| EdgeScroll {
                speed: 500.0,
                ..EdgeScroll::default()
            });
            let t0 = Instant::now();
            let later = t0 + std::time::Duration::from_millis(16);
            assert!(
                step(&edge, true, 1.0, true, t0, cx),
                "the first frame starts the clock"
            );
            // An unlaid list has nothing to scroll: it is at its end, and stops there.
            assert!(!step(&edge, true, 1.0, true, later, cx), "at the end");
            assert!(!step(&edge, true, 1.0, false, later, cx), "the drag ended");
            assert_eq!(edge.read(cx).speed, 0.0);
        });
    }
}
