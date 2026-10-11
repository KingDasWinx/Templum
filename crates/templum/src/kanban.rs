//! A kanban block's body: the board list, or a board's columns and cards with drag and drop;
//! and the dialog of an open card. Everything is drawn at the block's zoom `z`, like a
//! terminal's text. See docs/specs/kanban.md.

use gpui_kit::assets::IconName;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{Icon, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, BoxShadow, ClickEvent, Context, ElementId, Entity, FontWeight, Render,
    SharedString, TestSupportExt, Window, div, point, px,
};
use templum_canvas::{BlockId, Size};

use crate::board::{Board, Card, Column, Label};
use crate::grid::hsla;
use crate::prompt;
use crate::theme::Ui;
use crate::workspace::Workspace;

/// A new kanban block's size, and the smallest one (world units).
pub(crate) const SIZE: Size = Size { w: 900.0, h: 540.0 };
pub(crate) const MIN: Size = Size { w: 420.0, h: 280.0 };
const COLUMN_W: f32 = 260.0;

/// A kanban block: the board it shows, if any, and whether it shows the board list instead.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Kanban {
    pub board: Option<u64>,
    pub picking: bool,
}

impl Kanban {
    /// The board on screen: none while picking, or when it was deleted.
    pub fn shown<'a>(&self, ws: &'a Workspace) -> Option<&'a Board> {
        self.board
            .filter(|_| !self.picking)
            .and_then(|id| ws.boards.board(id))
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

/// What follows the pointer while dragging: the card or column's title, at the canvas's zoom.
struct DragPreview {
    text: SharedString,
    width: f32,
    z: f32,
    ui: Ui,
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

fn name(prefix: &str, n: usize, id: u64) -> ElementId {
    ElementId::Name(format!("{prefix}-{n}-{id}").into())
}

/// The body below the header: the board, or the board list.
pub(crate) fn body(
    ws: &Workspace,
    block: BlockId,
    kanban: Kanban,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    match kanban.shown(ws) {
        Some(board) => board_view(ws, block, board, z, cx),
        None => list_view(ws, block, z, cx),
    }
}

fn list_view(ws: &Workspace, block: BlockId, z: f32, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.ui;
    let n = block.0 as usize;
    let entity = cx.entity();
    let rows: Vec<AnyElement> = ws
        .boards
        .boards
        .iter()
        .map(|board| {
            let id = board.id;
            let cards = board.card_count();
            let row = div()
                .id(name("board-row", n, id))
                .flex()
                .items_center()
                .justify_between()
                .px(px(12.0 * z))
                .py(px(10.0 * z))
                .rounded(px(8.0 * z))
                .border_1()
                .border_color(ui.hairline)
                .bg(ui.surface)
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover).border_color(ui.hover_border))
                .child(
                    div()
                        .text_size(px(14.0 * z))
                        .font_weight(FontWeight::MEDIUM)
                        .child(board.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.0 * z))
                        .text_color(ui.title_dim)
                        .child(if cards == 1 {
                            "1 card".to_string()
                        } else {
                            format!("{cards} cards")
                        }),
                )
                .on_click(
                    cx.listener(move |ws, _: &ClickEvent, _, cx| ws.show_board(block, id, cx)),
                )
                .test_support();
            let (rename, delete) = (entity.clone(), entity.clone());
            let board_name = SharedString::from(board.name.clone());
            div()
                .id(name("board-menu", n, id))
                .child(row)
                .context_menu(move |menu, _, _| {
                    let (rename, delete, board_name) =
                        (rename.clone(), delete.clone(), board_name.clone());
                    let delete_name = board_name.clone();
                    menu.item(
                        PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
                            let ws = rename.clone();
                            rename.update(cx, |_, cx| {
                                prompt::ask_text(
                                    "Rename board",
                                    "Rename",
                                    board_name.clone(),
                                    move |text, _, cx| rename_board(&ws, id, &text, cx),
                                    window,
                                    cx,
                                )
                            })
                        }),
                    )
                    .item(
                        PopupMenuItem::new("Delete…").on_click(move |_, window, cx| {
                            let ws = delete.clone();
                            let what = if cards == 1 {
                                "Its card is deleted too.".to_string()
                            } else {
                                format!("Its {cards} cards are deleted too.")
                            };
                            prompt::confirm(
                                format!("Delete {delete_name}?"),
                                what,
                                "Delete",
                                move |_, cx| ws.update(cx, |ws, cx| ws.delete_board(id, cx)),
                                window,
                                cx,
                            )
                        }),
                    )
                })
                .into_any_element()
        })
        .collect();
    let new = div()
        .id(("new-board", n))
        .flex()
        .items_center()
        .gap(px(6.0 * z))
        .px(px(12.0 * z))
        .py(px(10.0 * z))
        .rounded(px(8.0 * z))
        .border_1()
        .border_color(ui.hairline)
        .text_size(px(13.0 * z))
        .text_color(ui.title)
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .active(move |s| s.bg(ui.pressed))
        .child(Icon::new(IconName::Plus).size(px(14.0 * z)))
        .child("New board")
        .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.new_board(block, cx)))
        .test_support();
    div()
        .id(("board-list", n))
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0 * z))
                .p(px(12.0 * z))
                .child(
                    div()
                        .text_size(px(18.0 * z))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Boards"),
                )
                .child(
                    div()
                        .mb(px(6.0 * z))
                        .text_size(px(12.0 * z))
                        .text_color(ui.title_dim)
                        .child("Pick the board this block shows. Every block can show any board."),
                )
                .children(rows)
                .child(new),
        )
        .test_support()
        .into_any_element()
}

fn rename_board(ws: &Entity<Workspace>, board: u64, text: &str, cx: &mut App) -> bool {
    let name = text.trim();
    if name.is_empty() {
        return false;
    }
    ws.update(cx, |ws, cx| {
        ws.boards.rename_board(board, name);
        cx.notify();
    });
    true
}

fn board_view(
    ws: &Workspace,
    block: BlockId,
    board: &Board,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let n = block.0 as usize;
    let board_id = board.id;
    let columns: Vec<AnyElement> = board
        .columns
        .iter()
        .map(|column| column_view(ws, n, board_id, column, z, cx))
        .collect();
    let add = div()
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
        .child("Add a column")
        .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| {
            ws.boards.add_column(board_id);
            cx.notify();
        }))
        .test_support();
    div()
        .id(("columns", n))
        .size_full()
        .overflow_x_scroll()
        .child(
            div()
                .h_full()
                .flex()
                .items_start()
                .gap(px(10.0 * z))
                .p(px(4.0 * z))
                .children(columns)
                .child(add),
        )
        .test_support()
        .into_any_element()
}

fn column_view(
    ws: &Workspace,
    n: usize,
    board: u64,
    column: &Column,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let col = column.id;
    let entity = cx.entity();
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
        .context_menu({
            let entity = entity.clone();
            move |menu, _, _| {
                let (rename, delete, title) = (entity.clone(), entity.clone(), title.clone());
                menu.item(
                    PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
                        let ws = rename.clone();
                        rename.update(cx, |_, cx| {
                            prompt::ask_text(
                                "Rename column",
                                "Rename",
                                title.clone(),
                                move |text, _, cx| {
                                    let title = text.trim().to_string();
                                    if title.is_empty() {
                                        return false;
                                    }
                                    ws.update(cx, |ws, cx| {
                                        ws.boards.rename_column(board, col, &title);
                                        cx.notify();
                                    });
                                    true
                                },
                                window,
                                cx,
                            )
                        })
                    }),
                )
                .item(PopupMenuItem::new("Delete").on_click(move |_, window, cx| {
                    let ws = delete.clone();
                    let remove = move |_: &mut Window, cx: &mut App| {
                        ws.update(cx, |ws, cx| {
                            ws.boards.remove_column(board, col);
                            cx.notify();
                        })
                    };
                    if count == 0 {
                        remove(window, cx);
                    } else {
                        prompt::confirm(
                            "Delete this column?",
                            if count == 1 {
                                "Its card is deleted too.".to_string()
                            } else {
                                format!("Its {count} cards are deleted too.")
                            },
                            "Delete",
                            remove,
                            window,
                            cx,
                        );
                    }
                }))
            }
        });
    let cards: Vec<AnyElement> = column
        .cards
        .iter()
        .map(|card| card_view(ws, n, board, col, card, z, cx))
        .collect();
    let add = div()
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
        .child("Add a card")
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
            if let Some(card) = ws.boards.add_card(board, col)
                && let Some(current) = ws.boards.card(board, card).cloned()
            {
                cx.notify();
                open_card(board, current, true, window, cx);
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
        .on_drop(cx.listener(move |ws, drag: &CardDrag, _, cx| {
            ws.boards.move_card(drag.board, drag.card, col, None);
            cx.notify();
        }))
        .on_drop(cx.listener(move |ws, drag: &ColumnDrag, _, cx| {
            ws.boards.move_column(drag.board, drag.column, col);
            cx.notify();
        }))
        .child(header)
        .child(
            div()
                .id(name("cards", n, col))
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.0 * z))
                        .px(px(6.0 * z))
                        .pb(px(6.0 * z))
                        .children(cards),
                ),
        )
        .child(add)
        .test_support()
        .into_any_element()
}

fn card_view(
    ws: &Workspace,
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
        .when(!card.description.is_empty(), |el| {
            el.child(
                Icon::new(IconName::FileText)
                    .size(px(12.0 * z))
                    .text_color(ui.title_dim),
            )
        })
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
        .on_drop(cx.listener(move |ws, drag: &CardDrag, _, cx| {
            ws.boards.move_card(drag.board, drag.card, column, Some(id));
            cx.stop_propagation();
            cx.notify();
        }))
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
            if let Some(current) = ws.boards.card(board, id).cloned() {
                open_card(board, current, false, window, cx)
            }
        }))
        .test_support()
        .into_any_element()
}

/// The open card: title, description and labels apply as they change. `new`: the title is
/// selected, to type over "New card".
fn open_card(
    board: u64,
    current: Card,
    new: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let card = current.id;
    let ws = cx.entity();
    let title = cx.new(|cx| InputState::new(window, cx).default_value(current.title));
    let description = cx.new(|cx| {
        TextareaState::new(window, cx)
            .rows(5)
            .default_value(current.description)
    });
    {
        let ws = ws.clone();
        cx.subscribe_in(
            &title,
            window,
            move |_, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let text = input.read(cx).value().to_string();
                    ws.update(cx, |ws, cx| {
                        ws.boards.edit_card(board, card, |c| c.title = text);
                        cx.notify();
                    })
                }
                InputEvent::PressEnter { .. } => window.close_dialog(cx),
                _ => {}
            },
        )
        .detach();
    }
    {
        let ws = ws.clone();
        cx.subscribe(&description, move |_, input, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let text = input.read(cx).value().to_string();
                ws.update(cx, |ws, cx| {
                    ws.boards.edit_card(board, card, |c| c.description = text);
                    cx.notify();
                })
            }
        })
        .detach();
    }
    title.update(cx, |input, cx| {
        input.focus(window, cx);
        if new {
            input.select_all(window, cx);
        }
    });
    window.open_alert_dialog(cx, move |alert, _, cx| {
        let ws_now = ws.read(cx);
        let ui = ws_now.ui;
        let palette = ws_now.theme.palette;
        let labels = ws_now
            .boards
            .card(board, card)
            .map(|c| c.labels.clone())
            .unwrap_or_default();
        let toggles = Label::ALL.map(|label| {
            let on = labels.contains(&label);
            let ws = ws.clone();
            div()
                .id(ElementId::Name(format!("label-{}", label.name()).into()))
                .w(px(40.0))
                .h(px(22.0))
                .rounded(px(6.0))
                .bg(hsla(
                    palette.ansi[label.ansi()],
                    if on { 1.0 } else { 0.35 },
                ))
                .border_2()
                .border_color(if on {
                    hsla(palette.foreground, 0.8)
                } else {
                    gpui_kit::transparent_black()
                })
                .cursor_pointer()
                .on_click(move |_: &ClickEvent, _, cx| {
                    ws.update(cx, |ws, cx| {
                        ws.boards.toggle_label(board, card, label);
                        cx.notify();
                    })
                })
                .test_support()
        });
        let delete = {
            let ws = ws.clone();
            div()
                .id("delete-card")
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .text_size(px(12.0))
                .text_color(hsla(palette.ansi[1], 1.0))
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .child("Delete card")
                .on_click(move |_: &ClickEvent, window, cx| {
                    ws.update(cx, |ws, cx| {
                        ws.boards.remove_card(board, card);
                        cx.notify();
                    });
                    window.close_dialog(cx);
                })
                .test_support()
        };
        let field = |label: &'static str| {
            div()
                .text_size(px(12.0))
                .text_color(ui.title_dim)
                .child(label)
        };
        alert
            .title("Card")
            .w(px(520.0))
            .button_props(DialogButtonProps::default().ok_text("Done"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(field("Title"))
                    .child(Input::new(&title))
                    .child(field("Description"))
                    .child(Textarea::new(&description))
                    .child(field("Labels"))
                    .child(div().flex().gap(px(6.0)).children(toggles))
                    .child(div().flex().justify_end().child(delete)),
            )
    });
}
