//! Right-click menus: one at a time, built for what was right-clicked, drawn at the pointer.
//! Each right-clickable element opens its target's menu and stops the event, so the innermost
//! one wins. See docs/specs/context-menus.md.

use crate::agents::AgentEntry;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, DismissEvent, Entity, Focusable as _, Pixels, Point as PixelPoint,
    ScrollWheelEvent, SharedString, Subscription, Window, anchored, deferred, div, px,
};
use templum_canvas::{BlockId, Point};

use crate::kanban;
use crate::launcher;
use crate::prompt;
use crate::workspace::{Body, Workspace};

/// What was right-clicked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    /// The empty canvas, at a world point: blocks are added there.
    Canvas(Point),
    /// A block, whatever it holds: the menu is built for its kind.
    Block(BlockId),
    /// A row of a kanban block's board list.
    Board {
        block: BlockId,
        board: u64,
    },
    Column {
        board: u64,
        column: u64,
    },
    Card {
        board: u64,
        card: u64,
    },
    /// A workspace's tile in the sidebar, by its id.
    Space(String),
}

/// The open menu.
pub(crate) struct OpenMenu {
    view: Entity<PopupMenu>,
    /// Window position of the right-click.
    at: PixelPoint<Pixels>,
    _dismissed: Subscription,
}

impl Workspace {
    /// Opens `target`'s menu at `at`, replacing any open one.
    pub(crate) fn open_menu(
        &mut self,
        target: Target,
        at: PixelPoint<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ws = cx.entity();
        // Esc or a choice gives the keyboard back to what had it.
        let back = window
            .focused(cx)
            .unwrap_or_else(|| self.focus_handle.clone());
        let Some(items) = self.items(&target) else {
            return;
        };
        let view = PopupMenu::build(window, cx, move |menu, window, cx| {
            items.build(&ws, menu.action_context(back), window, cx)
        });
        let _dismissed = cx.subscribe_in(&view, window, |ws, view, _: &DismissEvent, _, cx| {
            if ws.menu.as_ref().is_some_and(|m| m.view == *view) {
                ws.menu = None;
                cx.notify();
            }
        });
        view.focus_handle(cx).focus(window, cx);
        self.menu = Some(OpenMenu {
            view,
            at,
            _dismissed,
        });
        cx.notify();
    }

    /// What the menu for `target` needs to know, read now: building it can't read the workspace.
    fn items(&self, target: &Target) -> Option<Items> {
        Some(match *target {
            Target::Canvas(at) => Items::Canvas {
                at,
                agents: self
                    .agents
                    .iter()
                    .filter(|a| a.show)
                    .map(|a| (a.clone(), templum_terminal::installed(a.program())))
                    .collect(),
            },
            Target::Block(block) => match self.space.blocks.get(&block)? {
                Body::Terminal(pane) => Items::Terminal {
                    block,
                    selection: pane.term.shared().selection_text().is_some(),
                },
                Body::Kanban(k) => Items::Kanban {
                    block,
                    listing: k.shown(self).is_none() && k.board.is_some(),
                },
            },
            Target::Board { block, board } => Items::Board {
                block,
                board,
                name: self
                    .boards
                    .board(board)
                    .map_or_else(String::new, |b| b.name.clone()),
                cards: self.boards.board(board).map_or(0, |b| b.card_count()),
            },
            Target::Column { board, column } => {
                let found = self
                    .boards
                    .board(board)
                    .and_then(|b| b.columns.iter().find(|c| c.id == column));
                Items::Column {
                    board,
                    column,
                    title: found.map_or_else(String::new, |c| c.title.clone()),
                    cards: found.map_or(0, |c| c.cards.len()),
                }
            }
            Target::Card { board, card } => Items::Card { board, card },
            Target::Space(ref id) => Items::Space {
                id: id.clone(),
                name: self
                    .spaces()
                    .find(|s| s.id == *id)
                    .map_or_else(String::new, |s| s.name.clone()),
                only_one: self.others.is_empty(),
            },
        })
    }
}

/// A target's menu, with what it shows read from the workspace.
enum Items {
    Canvas {
        at: Point,
        /// Each agent the menu lists, and whether its program was found.
        agents: Vec<(AgentEntry, bool)>,
    },
    Terminal {
        block: BlockId,
        selection: bool,
    },
    Kanban {
        block: BlockId,
        /// The board list shows over a board the block has.
        listing: bool,
    },
    Board {
        block: BlockId,
        board: u64,
        name: String,
        cards: usize,
    },
    Column {
        board: u64,
        column: u64,
        title: String,
        cards: usize,
    },
    Card {
        board: u64,
        card: u64,
    },
    Space {
        id: String,
        name: String,
        only_one: bool,
    },
}

/// An item whose click updates the workspace.
fn item(
    ws: &Entity<Workspace>,
    label: impl Into<SharedString>,
    icon: Option<IconName>,
    act: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) -> PopupMenuItem {
    let ws = ws.clone();
    PopupMenuItem::new(label)
        .when_some(icon, |item, icon| item.icon(Icon::new(icon)))
        .on_click(move |_, window, cx| ws.update(cx, |ws, cx| act(ws, window, cx)))
}

/// What deleting `n` cards along with their board or column says.
fn cards(n: usize) -> String {
    if n == 1 {
        "Its card is deleted too.".to_string()
    } else {
        format!("Its {n} cards are deleted too.")
    }
}

impl Items {
    fn build(
        self,
        ws: &Entity<Workspace>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        match self {
            Items::Canvas { at, agents } => {
                let add = ws.clone();
                let ws = ws.clone();
                menu.submenu_with_icon(
                    Some(Icon::new(IconName::Bot)),
                    "AI agent",
                    window,
                    cx,
                    move |menu, _, _| {
                        // Installed first; the others say how to install them when chosen.
                        let (found, missing): (Vec<_>, Vec<_>) = agents
                            .iter()
                            .cloned()
                            .partition(|(_, installed)| *installed);
                        let entry = |menu: PopupMenu, (agent, installed): (AgentEntry, bool)| {
                            let title = SharedString::from(agent.title.clone());
                            menu.item(item(&ws, title, None, move |ws, window, cx| {
                                launcher::ask_folder(ws, agent.clone(), at, installed, window, cx)
                            }))
                        };
                        // Twenty agents: the list scrolls in a short window.
                        let menu = found
                            .into_iter()
                            .fold(menu.scrollable(true).max_h(px(420.0)), entry);
                        if missing.is_empty() {
                            return menu;
                        }
                        missing
                            .into_iter()
                            .fold(menu.separator().label("Not installed"), entry)
                    },
                )
                .submenu_with_icon(
                    Some(Icon::new(IconName::Plus)),
                    "Add",
                    window,
                    cx,
                    move |menu, _, _| {
                        let ws = &add;
                        menu.item(item(
                            ws,
                            "Terminal",
                            Some(IconName::SquareTerminal),
                            move |ws, window, cx| ws.add_terminal_at(at, window, cx),
                        ))
                        .item(item(
                            ws,
                            "Kanban board",
                            Some(IconName::LayoutDashboard),
                            move |ws, window, cx| ws.add_kanban_at(at, window, cx),
                        ))
                    },
                )
            }
            Items::Terminal { block, selection } => menu
                .item(
                    item(ws, "Copy", Some(IconName::Copy), |ws, _, cx| {
                        ws.copy_selection(cx)
                    })
                    .disabled(!selection),
                )
                .item(item(ws, "Paste", None, |ws, _, cx| ws.paste_clipboard(cx)))
                .separator()
                .item(item(
                    ws,
                    "Close terminal",
                    Some(IconName::Close),
                    move |ws, _, cx| ws.close(block, cx),
                )),
            Items::Kanban { block, listing } => menu
                .item(item(
                    ws,
                    if listing {
                        "Back to the board"
                    } else {
                        "Board list"
                    },
                    Some(IconName::ChevronsUpDown),
                    move |ws, _, cx| ws.pick_board(block, cx),
                ))
                .separator()
                .item(item(
                    ws,
                    "Close",
                    Some(IconName::Close),
                    move |ws, _, cx| ws.close(block, cx),
                )),
            Items::Board {
                block,
                board,
                name,
                cards: n,
            } => {
                let rename_name = name.clone();
                menu.item(item(ws, "Open", None, move |ws, _, cx| {
                    ws.show_board(block, board, cx)
                }))
                .item(item(ws, "Rename…", None, move |_, window, cx| {
                    let ws = cx.entity();
                    prompt::ask_text(
                        "Rename board",
                        "Rename",
                        rename_name.clone(),
                        move |text, _, cx| rename_board(&ws, board, &text, cx),
                        window,
                        cx,
                    )
                }))
                .item(item(
                    ws,
                    "Delete…",
                    Some(IconName::Delete),
                    move |_, window, cx| {
                        let ws = cx.entity();
                        prompt::confirm(
                            format!("Delete {name}?"),
                            cards(n),
                            "Delete",
                            move |_, cx| ws.update(cx, |ws, cx| ws.delete_board(board, cx)),
                            window,
                            cx,
                        )
                    },
                ))
            }
            Items::Column {
                board,
                column,
                title,
                cards: n,
            } => menu
                .item(item(ws, "Rename…", None, move |_, window, cx| {
                    let ws = cx.entity();
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
                                ws.boards.rename_column(board, column, &title);
                                cx.notify();
                            });
                            true
                        },
                        window,
                        cx,
                    )
                }))
                .item(item(
                    ws,
                    "Add a card",
                    Some(IconName::Plus),
                    move |ws, window, cx| kanban::add_card(ws, board, column, window, cx),
                ))
                .separator()
                .item(item(
                    ws,
                    "Delete",
                    Some(IconName::Delete),
                    move |ws, window, cx| {
                        if n == 0 {
                            ws.boards.remove_column(board, column);
                            cx.notify();
                            return;
                        }
                        let entity = cx.entity();
                        prompt::confirm(
                            "Delete this column?",
                            cards(n),
                            "Delete",
                            move |_, cx| {
                                entity.update(cx, |ws, cx| {
                                    ws.boards.remove_column(board, column);
                                    cx.notify();
                                })
                            },
                            window,
                            cx,
                        );
                    },
                )),
            Items::Card { board, card } => menu
                .item(item(ws, "Open", None, move |ws, window, cx| {
                    if let Some(current) = ws.boards.card(board, card).cloned() {
                        kanban::open_card(board, current, false, window, cx);
                    }
                }))
                .separator()
                .item(item(
                    ws,
                    "Delete card",
                    Some(IconName::Delete),
                    move |ws, _, cx| {
                        ws.boards.remove_card(board, card);
                        cx.notify();
                    },
                )),
            Items::Space { id, name, only_one } => {
                let delete_id = id.clone();
                let delete_name = name.clone();
                menu.item(item(ws, "Rename…", None, move |_, window, cx| {
                    let ws = cx.entity();
                    let id = id.clone();
                    prompt::ask_text(
                        "Rename workspace",
                        "Rename",
                        name.clone(),
                        move |text, window, cx| {
                            ws.update(cx, |ws, cx| ws.rename_space(&id, &text, window, cx))
                                .is_ok()
                        },
                        window,
                        cx,
                    )
                }))
                .item(
                    item(
                        ws,
                        "Delete…",
                        Some(IconName::Delete),
                        move |_, window, cx| {
                            let ws = cx.entity();
                            let id = delete_id.clone();
                            prompt::confirm(
                                format!("Delete {delete_name}?"),
                                "Its terminals will close.",
                                "Delete",
                                move |window, cx| {
                                    ws.update(cx, |ws, cx| ws.delete_space(&id, window, cx))
                                },
                                window,
                                cx,
                            )
                        },
                    )
                    .disabled(only_one),
                )
            }
        }
    }
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

/// The open menu, above everything, kept inside the window; the wheel doesn't reach what is
/// under it while it is open.
pub(crate) fn render(menu: &OpenMenu, window: &Window) -> AnyElement {
    let size = window.bounds().size;
    deferred(
        anchored().child(
            div()
                .w(size.width)
                .h(size.height)
                .on_scroll_wheel(|_: &ScrollWheelEvent, _, cx| cx.stop_propagation())
                .child(
                    anchored()
                        .position(menu.at)
                        .snap_to_window_with_margin(px(8.0))
                        .child(menu.view.clone()),
                ),
        ),
    )
    .with_priority(gpui_kit::base::POPUP_PRIORITY)
    .into_any_element()
}
