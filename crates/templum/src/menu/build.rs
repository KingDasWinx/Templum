//! Each target's menu: its items and what choosing them does.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::agents::AgentEntry;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{App, ClipboardItem, Context, Entity, SharedString, Window, px};
use templum_canvas::{BlockId, Point, Wire};

use super::{Items, SendTo, Shells, Tail};
use crate::board::Label;
use crate::git_block;
use crate::kanban;
use crate::launcher;
use crate::prompt;
use crate::workspace::Workspace;

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

/// Adding blocks, the canvas menu's and a tile's: AI agent ▸ and Add ▸ (Terminal ▸, Kanban board,
/// Note, Checklist, Editor), their top-left corner at `at` on the canvas. In grid mode they take half of
/// the focused tile.
fn adding(
    ws: &Entity<Workspace>,
    menu: PopupMenu,
    agents: Vec<(AgentEntry, bool)>,
    shells: Shells,
    at: Point,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (agent, add) = (ws.clone(), ws.clone());
    menu.submenu_with_icon(
        Some(Icon::new(IconName::Bot)),
        "AI agent",
        window,
        cx,
        move |menu, _, _| agent_items(&agent, menu, agents.clone(), at),
    )
    .submenu_with_icon(
        Some(Icon::new(IconName::Plus)),
        "Add",
        window,
        cx,
        move |menu, window, cx| {
            let ws = &add;
            let shells = shells.clone();
            let terminal = ws.clone();
            menu.submenu_with_icon(
                Some(Icon::new(IconName::SquareTerminal)),
                "Terminal",
                window,
                cx,
                move |menu, _, _| shell_items(&terminal, menu, shells.clone(), Some(at)),
            )
            .item(item(
                ws,
                "Kanban board",
                Some(IconName::LayoutDashboard),
                move |ws, window, cx| ws.add_kanban_at(at, window, cx),
            ))
            .item(item(
                ws,
                "Note",
                Some(IconName::StickyNote),
                move |ws, window, cx| ws.add_note_at(at, window, cx),
            ))
            .item(item(
                ws,
                "Checklist",
                Some(IconName::ListChecks),
                move |ws, window, cx| ws.add_checklist_at(at, window, cx),
            ))
            .item(item(
                ws,
                "Editor",
                Some(IconName::FileCode),
                move |ws, window, cx| ws.ask_editor_folder(at, window, cx),
            ))
        },
    )
}

/// **Default (*name*)**, then each other shell found by its label (docs/specs/shells.md, The
/// menus): the chosen one opens a terminal at `at` (None: where a new terminal goes).
fn shell_items(
    ws: &Entity<Workspace>,
    menu: PopupMenu,
    shells: Shells,
    at: Option<Point>,
) -> PopupMenu {
    let Shells { default, others } = shells;
    // Macs list eight shells.
    let menu = menu.scrollable(true).max_h(px(420.0)).item(item(
        ws,
        format!("Default ({default})"),
        Some(IconName::SquareTerminal),
        move |ws, window, cx| ws.open_shell(None, at, window, cx),
    ));
    if others.is_empty() {
        return menu;
    }
    others.into_iter().fold(menu.separator(), |menu, shell| {
        let label = SharedString::from(shell.label.clone());
        menu.item(item(ws, label, None, move |ws, window, cx| {
            ws.open_shell(Some(shell.clone()), at, window, cx)
        }))
    })
}

/// The agents, installed first; the others are greyed out (Settings ▸ Agents says how to install
/// them). `at`: where the agent's block goes.
fn agent_items(
    ws: &Entity<Workspace>,
    menu: PopupMenu,
    agents: Vec<(AgentEntry, bool)>,
    at: Point,
) -> PopupMenu {
    let (found, missing): (Vec<_>, Vec<_>) =
        agents.into_iter().partition(|(_, installed)| *installed);
    let entry = |menu: PopupMenu, (agent, installed): (AgentEntry, bool)| {
        let title = SharedString::from(agent.title.clone());
        menu.item(
            item(ws, title, None, move |ws, window, cx| {
                launcher::ask_folder(ws, agent.clone(), at, window, cx)
            })
            .disabled(!installed),
        )
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
}

/// One item per terminal (`Workspace::terminal_list`); `act` on the chosen one.
fn terminal_items(
    ws: &Entity<Workspace>,
    menu: PopupMenu,
    terminals: &[(BlockId, String)],
    act: impl Fn(&mut Workspace, BlockId, &mut Window, &mut Context<Workspace>) + Clone + 'static,
) -> PopupMenu {
    terminals.iter().fold(menu, |menu, (id, label)| {
        let (id, act) = (*id, act.clone());
        menu.item(item(
            ws,
            label.clone(),
            Some(IconName::SquareTerminal),
            move |ws, window, cx| act(ws, id, window, cx),
        ))
    })
}

/// Send to ▸ the workspace's terminals: the text is typed into the chosen one, never run.
/// Nothing without a terminal or a text.
fn send_to(
    ws: &Entity<Workspace>,
    menu: PopupMenu,
    send: SendTo,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    if send.to.is_empty() || send.text.trim().is_empty() {
        return menu;
    }
    let ws = ws.clone();
    let text: Rc<str> = send.text.into();
    menu.submenu_with_icon(
        Some(Icon::new(IconName::SquareTerminal)),
        "Send to",
        window,
        cx,
        move |menu, _, _| {
            let text = text.clone();
            terminal_items(&ws, menu, &send.to, move |ws, id, window, cx| {
                ws.send_to(id, &text, window, cx)
            })
        },
    )
}

/// Connect to ▸ (`on`) the blocks listed, or Disconnect from ▸ them; nothing when there are none.
fn wiring(
    ws: &Entity<Workspace>,
    menu: PopupMenu,
    block: BlockId,
    blocks: Vec<(BlockId, SharedString)>,
    on: bool,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    if blocks.is_empty() {
        return menu;
    }
    let (icon, label) = if on {
        (IconName::Cable, "Connect to")
    } else {
        (IconName::Unplug, "Disconnect from")
    };
    let ws = ws.clone();
    menu.submenu_with_icon(
        Some(Icon::new(icon)),
        label,
        window,
        cx,
        move |menu, _, _| {
            blocks.iter().fold(menu, |menu, (other, title)| {
                let other = *other;
                menu.item(item(&ws, title.clone(), None, move |ws, _, cx| {
                    if on {
                        ws.connect(block, other, cx);
                    } else {
                        ws.disconnect(Wire { a: block, b: other }, cx);
                    }
                }))
            })
        },
    )
}

/// Cancel schedules ▸ each schedule the block's agent asked for, and All; nothing when there are
/// none (docs/specs/agent-tools.md, What the person sees).
fn cancelling(
    ws: &Entity<Workspace>,
    menu: PopupMenu,
    schedules: Vec<(u64, String)>,
    terminal: Option<String>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(terminal) = terminal.filter(|_| !schedules.is_empty()) else {
        return menu;
    };
    let ws = ws.clone();
    menu.submenu_with_icon(
        Some(Icon::new(IconName::Clock)),
        "Cancel schedules",
        window,
        cx,
        move |menu, _, _| {
            let terminal = terminal.clone();
            schedules
                .iter()
                .fold(menu, |menu, (id, about)| {
                    let id = *id;
                    menu.item(item(&ws, about.clone(), None, move |ws, _, cx| {
                        ws.cancel_schedule_now(id, cx)
                    }))
                })
                .item(item(&ws, "All", None, move |ws, _, cx| {
                    ws.cancel_schedules_of(&terminal, cx)
                }))
        },
    )
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
    pub(super) fn build(
        self,
        ws: &Entity<Workspace>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        match self {
            // Every block's menu ends the same way: grid mode's items, then Close.
            Items::Block {
                block,
                inner,
                tail,
                close,
                moving,
                spaces,
                schedules,
                terminal,
            } => {
                let menu = inner.build(ws, menu, window, cx).separator();
                let menu = match tail {
                    Tail::Canvas {
                        group,
                        connect,
                        disconnect,
                    } => {
                        let menu = menu.item(item(
                            ws,
                            "Fullscreen",
                            Some(IconName::Maximize2),
                            move |ws, window, cx| {
                                let ids = if group.contains(&block) {
                                    group.clone()
                                } else {
                                    vec![block]
                                };
                                ws.enter_grid(ids, window, cx)
                            },
                        ));
                        let menu = wiring(ws, menu, block, connect, true, window, cx);
                        let menu = wiring(ws, menu, block, disconnect, false, window, cx);
                        cancelling(ws, menu, schedules, terminal, window, cx)
                    }
                    Tail::Tile {
                        flip,
                        others,
                        at,
                        agents,
                        shells,
                    } => {
                        let bring = ws.clone();
                        // New blocks take half of this tile: it has the focus.
                        adding(ws, menu, agents, shells, at, window, cx)
                            .when(!others.is_empty(), |menu| {
                                menu.submenu("From the canvas", window, cx, move |menu, _, _| {
                                    others.iter().fold(menu, |menu, (id, title)| {
                                        let id = *id;
                                        menu.item(item(
                                            &bring,
                                            title.clone(),
                                            None,
                                            move |ws, window, cx| {
                                                ws.add_to_grid(id, block, window, cx)
                                            },
                                        ))
                                    })
                                })
                            })
                            .separator()
                            .when(flip, |menu| {
                                menu.item(item(ws, "Flip the split", None, move |ws, _, cx| {
                                    ws.flip_tile(block, cx)
                                }))
                            })
                            .item(item(ws, "Leave the grid", None, move |ws, window, cx| {
                                ws.leave_tile(block, window, cx)
                            }))
                            .item(item(
                                ws,
                                "Back to the canvas",
                                Some(IconName::Minimize2),
                                |ws, window, cx| ws.leave_grid(window, cx),
                            ))
                    }
                };
                let mover = ws.clone();
                menu.submenu("Move to workspace", window, cx, move |menu, _, _| {
                    let to_new = moving.clone();
                    spaces
                        .iter()
                        .fold(menu, |menu, (id, name, takes)| {
                            let (id, ids) = (id.clone(), moving.clone());
                            menu.item(
                                item(&mover, name.clone(), None, move |ws, window, cx| {
                                    ws.move_blocks(ids.clone(), Some(&id), window, cx)
                                })
                                .disabled(!*takes),
                            )
                        })
                        .when(!spaces.is_empty(), |menu| menu.separator())
                        .item(item(
                            &mover,
                            "New workspace",
                            Some(IconName::Plus),
                            move |ws, window, cx| ws.move_blocks(to_new.clone(), None, window, cx),
                        ))
                })
                .item(item(
                    ws,
                    close,
                    Some(IconName::Close),
                    move |ws, window, cx| ws.close(block, window, cx),
                ))
            }
            Items::Canvas { at, agents, shells } => {
                let notes = item(
                    ws,
                    "Notes…",
                    Some(IconName::NotebookText),
                    |_, window, cx| crate::note::open_notes(None, window, cx),
                );
                let checklists = item(
                    ws,
                    "Checklists…",
                    Some(IconName::ListChecks),
                    |_, window, cx| crate::checklist::open_checklists(None, window, cx),
                );
                adding(ws, menu, agents, shells, at, window, cx)
                    .item(notes)
                    .item(checklists)
            }
            Items::Checklist { block } => menu.item(item(
                ws,
                "All checklists…",
                Some(IconName::ListChecks),
                move |_, window, cx| crate::checklist::open_checklists(Some(block), window, cx),
            )),
            Items::Terminal { selection } => menu
                .item(
                    item(ws, "Copy", Some(IconName::Copy), |ws, window, cx| {
                        ws.copy_selection(window, cx)
                    })
                    .disabled(!selection),
                )
                .item(item(ws, "Paste", None, |ws, _, cx| ws.paste_clipboard(cx))),
            Items::Kanban { block, board } => {
                let menu = menu.item(item(
                    ws,
                    "All boards…",
                    Some(IconName::LayoutDashboard),
                    move |_, window, cx| crate::kanban::open_boards(block, window, cx),
                ));
                let Some((board, name, n)) = board else {
                    return menu;
                };
                let rename_name = name.clone();
                menu.item(item(ws, "Rename board…", None, move |_, window, cx| {
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
                    "Delete board…",
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
            Items::Agents { at, agents } => agent_items(ws, menu, agents, at),
            Items::Shells(shells) => shell_items(ws, menu, shells, None),
            Items::Tree {
                block,
                folder,
                path,
                is_dir,
                entries,
            } => {
                // Where a new entry goes: the folder clicked, the file's folder, or the root.
                let dir = match &path {
                    Some(p) if is_dir => p.clone(),
                    Some(p) => p.parent().map_or(folder.clone(), Path::to_path_buf),
                    None => folder.clone(),
                };
                let name = path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                let reveal = |ws: &Entity<Workspace>, p: PathBuf| {
                    item(
                        ws,
                        "Reveal in file manager",
                        Some(IconName::ExternalLink),
                        move |_, _, cx| cx.reveal_path(&p),
                    )
                };
                let mut menu = menu;
                if let Some(p) = path.clone().filter(|_| !is_dir) {
                    let (open, history, blame) = (p.clone(), p.clone(), p.clone());
                    menu = menu
                        .item(item(ws, "Open", None, move |ws, window, cx| {
                            ws.open_file(block, &open, None, window, cx)
                        }))
                        .item(reveal(ws, p))
                        .item(item(ws, "File history", None, move |ws, window, cx| {
                            ws.history_from_tree(history.clone(), false, window, cx)
                        }))
                        .item(item(ws, "Blame", None, move |ws, window, cx| {
                            ws.history_from_tree(blame.clone(), true, window, cx)
                        }));
                }
                if path.is_none() || is_dir {
                    let (file_dir, folder_dir) = (dir.clone(), dir.clone());
                    menu = menu
                        .item(item(
                            ws,
                            "New file…",
                            Some(IconName::FilePlus),
                            move |_, window, cx| {
                                let ws = cx.entity();
                                let dir = file_dir.clone();
                                prompt::ask_text(
                                    "New file",
                                    "Create",
                                    "",
                                    move |text, window, cx| {
                                        ws.update(cx, |ws, cx| {
                                            ws.new_entry(block, &dir, false, &text, window, cx)
                                        })
                                    },
                                    window,
                                    cx,
                                )
                            },
                        ))
                        .item(item(
                            ws,
                            "New folder…",
                            Some(IconName::FolderPlus),
                            move |_, window, cx| {
                                let ws = cx.entity();
                                let dir = folder_dir.clone();
                                prompt::ask_text(
                                    "New folder",
                                    "Create",
                                    "",
                                    move |text, window, cx| {
                                        ws.update(cx, |ws, cx| {
                                            ws.new_entry(block, &dir, true, &text, window, cx)
                                        })
                                    },
                                    window,
                                    cx,
                                )
                            },
                        ));
                }
                if let Some(p) = path.clone().filter(|_| is_dir) {
                    menu = menu.item(item(
                        ws,
                        "Open a terminal here",
                        Some(IconName::SquareTerminal),
                        move |ws, window, cx| {
                            ws.terminal_beside(Some(block), &p, None, window, cx);
                        },
                    ));
                }
                if let Some(p) = path.clone() {
                    let (rename, rename_name) = (p.clone(), name.clone());
                    let (delete, delete_name) = (p.clone(), name.clone());
                    menu = menu
                        .item(item(ws, "Rename…", None, move |_, window, cx| {
                            let ws = cx.entity();
                            let p = rename.clone();
                            prompt::ask_text(
                                "Rename",
                                "Rename",
                                rename_name.clone(),
                                move |text, window, cx| {
                                    ws.update(cx, |ws, cx| {
                                        ws.rename_entry(block, &p, &text, window, cx)
                                    })
                                },
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
                                let p = delete.clone();
                                let what = if is_dir {
                                    format!("It has {entries} entries. This can't be undone.")
                                } else {
                                    "This can't be undone.".to_string()
                                };
                                prompt::confirm(
                                    format!("Delete {delete_name}?"),
                                    what,
                                    "Delete",
                                    move |window, cx| {
                                        ws.update(cx, |ws, cx| {
                                            ws.delete_entry(block, &p, window, cx)
                                        })
                                    },
                                    window,
                                    cx,
                                )
                            },
                        ));
                    if is_dir {
                        menu = menu.item(reveal(ws, p));
                    }
                }
                if path.is_none() {
                    menu = menu.item(item(
                        ws,
                        "Refresh",
                        Some(IconName::RefreshCw),
                        move |ws, _, cx| ws.refresh_tree(block, cx),
                    ));
                }
                menu
            }
            Items::GitMore { busy, trash } => menu
                .item(
                    item(ws, "Fetch", None, move |ws, window, cx| {
                        ws.fetch(window, cx)
                    })
                    .disabled(busy),
                )
                .item(
                    item(ws, "Pull", None, move |ws, window, cx| {
                        ws.pull(templum_git::Pull::FastForward, window, cx)
                    })
                    .disabled(busy),
                )
                .item(
                    item(ws, "Push", Some(IconName::Upload), move |ws, window, cx| {
                        ws.push(window, cx)
                    })
                    .disabled(busy),
                )
                .separator()
                .when(trash, |menu| {
                    menu.item(item(
                        ws,
                        "Recently discarded…",
                        Some(IconName::ArchiveRestore),
                        move |ws, window, cx| ws.show_discarded(window, cx),
                    ))
                })
                .item(item(
                    ws,
                    "Clone a repository…",
                    None,
                    move |ws, window, cx| ws.ask_clone(window, cx),
                )),
            Items::GitRemotes {
                at,
                branch,
                remotes,
            } => remotes.into_iter().fold(menu, |menu, remote| {
                let (branch, label, at) =
                    (branch.clone(), format!("Publish to {remote}"), at.clone());
                menu.item(item(
                    ws,
                    label,
                    Some(IconName::Upload),
                    move |ws, window, cx| {
                        if ws.show_git(&at, window, cx) {
                            ws.publish_to(remote.clone(), branch.clone(), window, cx)
                        }
                    },
                ))
            }),
            Items::GitCommitMenu { label, can, amend } => menu
                .item(
                    item(ws, label.clone(), None, move |ws, window, cx| {
                        ws.git_commit(false, window, cx)
                    })
                    .disabled(!can),
                )
                .item(
                    item(
                        ws,
                        format!("{label} & Push"),
                        Some(IconName::Upload),
                        move |ws, window, cx| ws.git_commit(true, window, cx),
                    )
                    .disabled(!can),
                )
                .separator()
                .item(
                    item(ws, "Amend the last commit", None, move |ws, window, cx| {
                        ws.set_amend(!amend, window, cx)
                    })
                    .checked(amend),
                ),
            Items::GitGenerateMenu { models, model } => {
                let listed = !models.is_empty();
                let menu = models.into_iter().fold(menu, |menu, m| {
                    let checked = m == model;
                    let chosen = m.clone();
                    menu.item(
                        item(ws, m, None, move |ws, _, cx| {
                            ws.set_commit_model(&chosen, cx)
                        })
                        .checked(checked),
                    )
                });
                let menu = if listed { menu.separator() } else { menu };
                menu.item(item(
                    ws,
                    "Instructions for this repository…",
                    None,
                    move |ws, window, cx| ws.ask_repo_instructions(window, cx),
                ))
            }
            Items::GitRow {
                list,
                entry,
                labels: (first, second),
            } => {
                use git_block::List;
                let path = entry.path.clone();
                let mut menu = menu;
                match list {
                    List::Conflicts => {
                        let (open, ours, theirs, resolved) =
                            (path.clone(), path.clone(), path.clone(), path.clone());
                        menu = menu
                            .item(item(
                                ws,
                                "Open in editor",
                                Some(IconName::FileCode),
                                move |ws, window, cx| ws.open_from_git(&open, window, cx),
                            ))
                            .item(item(
                                ws,
                                format!("Use the version from {first}"),
                                None,
                                move |ws, window, cx| {
                                    ws.resolve_file(
                                        ours.clone(),
                                        templum_git::Side::Ours,
                                        window,
                                        cx,
                                    )
                                },
                            ))
                            .item(item(
                                ws,
                                format!("Use the version from {second}"),
                                None,
                                move |ws, window, cx| {
                                    ws.resolve_file(
                                        theirs.clone(),
                                        templum_git::Side::Theirs,
                                        window,
                                        cx,
                                    )
                                },
                            ))
                            .item(item(
                                ws,
                                "Mark resolved",
                                Some(IconName::Check),
                                move |ws, window, cx| {
                                    ws.mark_resolved(resolved.clone(), window, cx)
                                },
                            ));
                    }
                    List::Files => {
                        // Stage what isn't staged yet, Unstage what is: a file staged in part
                        // offers both.
                        if entry.unstaged.is_some() {
                            let stage = path.clone();
                            menu = menu.item(item(
                                ws,
                                "Stage",
                                Some(IconName::Plus),
                                move |ws, window, cx| ws.stage(vec![stage.clone()], window, cx),
                            ));
                        }
                        if entry.staged.is_some() {
                            let unstage = path.clone();
                            menu = menu.item(item(
                                ws,
                                "Unstage",
                                Some(IconName::Minus),
                                move |ws, window, cx| ws.unstage(vec![unstage.clone()], window, cx),
                            ));
                        }
                        if entry.unstaged.is_some() {
                            let discard = path.clone();
                            menu = menu.item(item(
                                ws,
                                "Discard changes…",
                                Some(IconName::Undo2),
                                move |ws, window, cx| ws.discard(vec![discard.clone()], window, cx),
                            ));
                        }
                        let open = path.clone();
                        menu = menu.item(item(
                            ws,
                            "Open",
                            Some(IconName::FileCode),
                            move |ws, window, cx| ws.open_from_git(&open, window, cx),
                        ));
                    }
                }
                // A new file has no history yet.
                if list != List::Conflicts && entry.unstaged != Some(templum_git::Change::Untracked)
                {
                    let (history, blame) = (path.clone(), path.clone());
                    menu = menu
                        .item(item(ws, "File history", None, move |ws, window, cx| {
                            ws.file_history(history.clone(), window, cx)
                        }))
                        .item(item(ws, "Blame", None, move |ws, window, cx| {
                            ws.blame_file(blame.clone(), None, window, cx)
                        }));
                }
                let (copy, ignore) = (path.clone(), path.clone());
                menu.item(item(
                    ws,
                    "Copy path",
                    Some(IconName::Copy),
                    move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())),
                ))
                .item(item(ws, "Ignore", None, move |ws, window, cx| {
                    ws.ignore_path(ignore.clone(), window, cx)
                }))
            }
            Items::GitLine {
                working,
                path,
                line,
                selection,
                line_text,
                hunk_text,
                send,
            } => {
                let has = selection.is_some();
                let copy = selection.unwrap_or_default();
                let menu = menu
                    .item(
                        item(ws, "Copy", Some(IconName::Copy), move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                        })
                        .disabled(!has),
                    )
                    .item(item(ws, "Copy line", None, move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(line_text.clone()))
                    }))
                    .item(item(ws, "Copy hunk", None, move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(hunk_text.clone()))
                    }));
                let menu = send_to(ws, menu, send, window, cx);
                menu.when(working, |menu| {
                    menu.separator().item(item(
                        ws,
                        "Open in editor at this line",
                        Some(IconName::FileCode),
                        move |ws, window, cx| ws.open_from_git_at(&path, Some(line), window, cx),
                    ))
                })
            }
            Items::GitCommit {
                commit,
                undo,
                restore,
            } => {
                let (checkout, branch, tag, revert, copy) = (
                    commit.hash.clone(),
                    commit.hash.clone(),
                    commit.hash.clone(),
                    commit.hash.clone(),
                    commit.hash.clone(),
                );
                let (short, restore_hash) = (commit.short.clone(), commit.hash.clone());
                let message = if commit.body.is_empty() {
                    commit.subject.clone()
                } else {
                    format!("{}\n\n{}", commit.subject, commit.body)
                };
                menu.item(item(ws, "Checkout…", None, move |ws, window, cx| {
                    ws.checkout_commit(checkout.clone(), window, cx)
                }))
                .item(item(
                    ws,
                    "New branch here…",
                    Some(IconName::GitBranchPlus),
                    move |ws, window, cx| {
                        ws.open_branch_dialog(
                            git_block::switcher::Opened::Commit(branch.clone()),
                            window,
                            cx,
                        )
                    },
                ))
                .item(item(
                    ws,
                    "Create tag…",
                    Some(IconName::Tag),
                    move |ws, window, cx| ws.tag_commit(tag.clone(), window, cx),
                ))
                .item(item(
                    ws,
                    "Revert…",
                    Some(IconName::Undo2),
                    move |ws, window, cx| ws.revert_commit(revert.clone(), window, cx),
                ))
                .when(undo, |menu| {
                    menu.item(item(
                        ws,
                        "Undo this commit",
                        Some(IconName::Undo2),
                        move |ws, window, cx| ws.undo_commit(short.clone(), None, window, cx),
                    ))
                })
                .item(item(
                    ws,
                    "Copy hash",
                    Some(IconName::Copy),
                    move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())),
                ))
                .item(item(
                    ws,
                    "Copy message",
                    Some(IconName::Copy),
                    move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(message.clone()))
                    },
                ))
                .when_some(restore, |menu, then| {
                    menu.item(item(
                        ws,
                        "Restore this version…",
                        Some(IconName::Undo2),
                        move |ws, window, cx| {
                            ws.restore_version(restore_hash.clone(), then.clone(), window, cx)
                        },
                    ))
                })
            }
            Items::GitCommitFile { hash, path } => {
                let (history, blame) = (path.clone(), path.clone());
                menu.item(item(ws, "File history", None, move |ws, window, cx| {
                    ws.file_history(history.clone(), window, cx)
                }))
                .item(item(ws, "Blame", None, move |ws, window, cx| {
                    ws.blame_file(blame.clone(), Some(hash.clone()), window, cx)
                }))
                .item(item(
                    ws,
                    "Copy path",
                    Some(IconName::Copy),
                    move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(path.clone())),
                ))
            }
            Items::GitBranch {
                branch,
                current,
                base,
            } => {
                let name = branch.name.clone();
                let into = current.unwrap_or_else(|| "HEAD".to_string());
                let mut menu = menu;
                if let Some(base) = base.filter(|b| *b != name) {
                    let compare = name.clone();
                    menu = menu.item(item(
                        ws,
                        format!("Compare with {}", git_block::words::local_name(&base)),
                        Some(IconName::GitCompare),
                        move |ws, window, cx| ws.compare_branch(compare.clone(), window, cx),
                    ));
                }
                if !branch.current {
                    let merge = name.clone();
                    menu = menu.item(item(
                        ws,
                        format!("Merge into {into}…"),
                        Some(IconName::GitMerge),
                        move |ws, window, cx| ws.merge_branch(merge.clone(), window, cx),
                    ));
                }
                if !branch.remote {
                    let rename = name.clone();
                    menu = menu.item(item(ws, "Rename…", None, move |ws, window, cx| {
                        ws.rename_branch(rename.clone(), window, cx)
                    }));
                }
                if !branch.remote && !branch.current {
                    let delete = name.clone();
                    menu = menu.item(item(
                        ws,
                        "Delete…",
                        Some(IconName::Delete),
                        move |ws, window, cx| ws.delete_branch(delete.clone(), false, window, cx),
                    ));
                }
                // No upstream, or one deleted on the server: Publish sends it (again).
                if !branch.remote && (branch.upstream.is_none() || branch.gone) {
                    let publish = name.clone();
                    menu = menu.item(item(
                        ws,
                        "Publish",
                        Some(IconName::Upload),
                        move |ws, window, cx| ws.publish_branch(publish.clone(), window, cx),
                    ));
                }
                menu.item(item(
                    ws,
                    "Copy name",
                    Some(IconName::Copy),
                    move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(name.clone())),
                ))
            }
            Items::GitStash { index } => menu
                .item(item(ws, "Apply", None, move |ws, window, cx| {
                    ws.stash_apply(index, false, window, cx)
                }))
                .item(item(ws, "Pop", None, move |ws, window, cx| {
                    ws.stash_apply(index, true, window, cx)
                }))
                .item(item(
                    ws,
                    "Drop…",
                    Some(IconName::Delete),
                    move |ws, window, cx| ws.stash_drop(index, window, cx),
                ))
                .item(item(
                    ws,
                    "Show",
                    Some(IconName::FileDiff),
                    move |ws, window, cx| ws.show_stash(index, window, cx),
                )),
            Items::GitFolders { folders, folder } => {
                let listed = !folders.is_empty();
                let menu = folders.into_iter().fold(menu, |menu, f| {
                    let label = f.file_name().map_or_else(
                        || f.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    menu.item(item(
                        ws,
                        label,
                        Some(IconName::Folder),
                        move |ws, window, cx| {
                            ws.set_git_folder(&f, window, cx);
                        },
                    ))
                });
                let menu = if listed { menu.separator() } else { menu };
                menu.item(item(
                    ws,
                    "Choose another folder…",
                    Some(IconName::FolderOpen),
                    move |ws, window, cx| ws.choose_git_folder(window, cx),
                ))
                .item(item(
                    ws,
                    "Refresh",
                    Some(IconName::RefreshCw),
                    move |ws, window, cx| ws.refresh_git(window, cx),
                ))
                .when_some(folder.filter(|f| f.is_dir()), |menu, folder| {
                    menu.item(item(
                        ws,
                        "Open a terminal here",
                        Some(IconName::SquareTerminal),
                        move |ws, window, cx| {
                            if let Some(id) = ws.terminal_beside(None, &folder, None, window, cx) {
                                ws.close_git(window, cx);
                                let space = ws.space.id.clone();
                                ws.reveal(&space, id, window, cx);
                            }
                        },
                    ))
                })
            }
            Items::Editor {
                block,
                tree,
                wrap,
                send,
            } => menu
                .item(item(
                    ws,
                    "Change folder…",
                    Some(IconName::FolderOpen),
                    move |ws, window, cx| ws.choose_editor_folder(block, window, cx),
                ))
                .item(item(
                    ws,
                    if tree {
                        "Hide the file tree"
                    } else {
                        "Show the file tree"
                    },
                    Some(IconName::PanelLeft),
                    move |ws, _, cx| ws.toggle_tree(block, cx),
                ))
                .item(item(
                    ws,
                    if wrap {
                        "Turn soft wrap off"
                    } else {
                        "Turn soft wrap on"
                    },
                    None,
                    move |ws, window, cx| ws.toggle_wrap(block, window, cx),
                ))
                .map(|menu| send_to(ws, menu, send, window, cx)),
            Items::Reveal { terminals } => {
                terminal_items(ws, menu, &terminals, |ws, id, window, cx| {
                    let space = ws.space.id.clone();
                    ws.reveal(&space, id, window, cx);
                })
            }
            Items::Ink { index } => menu.item(item(
                ws,
                "Delete",
                Some(IconName::Delete),
                move |ws, _, cx| ws.delete_ink(index, cx),
            )),
            Items::Wire { wire } => menu
                .item(item(ws, "Rename wire…", None, move |ws, window, cx| {
                    let label = ws.space.label(wire).unwrap_or_default().to_string();
                    let entity = cx.entity();
                    prompt::ask_text(
                        "Rename wire",
                        "Rename",
                        label,
                        move |text, _, cx| {
                            entity.update(cx, |ws, cx| ws.rename_wire(wire, &text, cx));
                            true
                        },
                        window,
                        cx,
                    )
                }))
                .item(item(
                    ws,
                    "Disconnect",
                    Some(IconName::Unplug),
                    move |ws, _, cx| {
                        ws.disconnect(wire, cx);
                    },
                )),
            Items::Note {
                block,
                color,
                rename,
                send,
            } => menu
                .item(item(
                    ws,
                    "All notes…",
                    Some(IconName::NotebookText),
                    move |_, window, cx| crate::note::open_notes(Some(block), window, cx),
                ))
                .when_some(rename, |menu, (note, name)| {
                    menu.item(item(ws, "Rename…", None, move |_, window, cx| {
                        let ws = cx.entity();
                        prompt::ask_text(
                            "Rename note",
                            "Rename",
                            name.clone(),
                            move |text, _, cx| {
                                ws.update(cx, |ws, cx| ws.rename_note(note, &text, cx));
                                true
                            },
                            window,
                            cx,
                        )
                    }))
                })
                .item(
                    item(ws, "Copy all", Some(IconName::Copy), move |ws, _, cx| {
                        if let Some(note) = ws.space.blocks.get(&block).and_then(|b| b.note()) {
                            let text = note.text(cx);
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }
                    })
                    .disabled(color.is_none()),
                )
                .map(|menu| send_to(ws, menu, send, window, cx))
                .when_some(color, |menu, color| {
                    let ws = ws.clone();
                    menu.submenu_with_icon(
                        Some(Icon::new(IconName::Palette)),
                        "Color",
                        window,
                        cx,
                        move |menu, _, _| {
                            let none = item(&ws, "None", None, move |ws, _, cx| {
                                ws.set_note_color(block, None, cx)
                            })
                            .checked(color.is_none());
                            Label::ALL.into_iter().fold(menu.item(none), |menu, label| {
                                let name = label.name();
                                let title = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
                                menu.item(
                                    item(&ws, title, None, move |ws, _, cx| {
                                        ws.set_note_color(block, Some(label), cx)
                                    })
                                    .checked(color == Some(label)),
                                )
                            })
                        },
                    )
                }),
            Items::Column {
                board,
                column,
                title,
                cards: n,
                draft,
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
                                ws.edit_boards(|b| b.rename_column(board, column, &title));
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
                    if draft {
                        "Continue draft"
                    } else {
                        "Add a card"
                    },
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
                            ws.edit_boards(|b| b.remove_column(board, column));
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
                                    ws.edit_boards(|b| b.remove_column(board, column));
                                    cx.notify();
                                })
                            },
                            window,
                            cx,
                        );
                    },
                )),
            Items::Card { board, card, send } => menu
                .item(item(ws, "Open", None, move |ws, window, cx| {
                    kanban::open_card(ws, board, card, window, cx);
                }))
                .map(|menu| send_to(ws, menu, send, window, cx))
                .separator()
                .item(item(
                    ws,
                    "Delete card",
                    Some(IconName::Delete),
                    move |ws, window, cx| {
                        let title = ws
                            .boards
                            .card(board, card)
                            .map_or_else(String::new, |c| c.title.clone());
                        let ws = cx.entity();
                        prompt::confirm(
                            "Delete this card?",
                            title,
                            "Delete",
                            move |_, cx| {
                                ws.update(cx, |ws, cx| {
                                    ws.edit_boards(|b| b.remove_card(board, card));
                                    cx.notify();
                                })
                            },
                            window,
                            cx,
                        );
                    },
                )),
            Items::Space {
                id,
                name,
                only_one,
                active,
                stoppable,
                unsaved,
                busy,
            } => {
                let stop_id = id.clone();
                let delete_id = id.clone();
                let delete_name = name.clone();
                // What closing it loses, as closing blocks says.
                let mut why = "Its terminals will close.".to_string();
                for b in &busy {
                    why = format!("{why} {b}");
                }
                let open_id = id.clone();
                menu.item(
                    item(ws, "Open", None, move |ws, window, cx| {
                        if let Some(at) = ws.order().iter().position(|s| s.id == open_id) {
                            ws.switch_to(at, window, cx);
                        }
                    })
                    .disabled(active),
                )
                .item(item(
                    ws,
                    "Edit…",
                    Some(IconName::Pencil),
                    move |ws, window, cx| crate::space_dialog::open(ws, &id, window, cx),
                ))
                .separator()
                .item(
                    item(ws, "Stop", Some(IconName::Power), move |ws, window, cx| {
                        ws.stop_space(&stop_id, window, cx)
                    })
                    .disabled(!stoppable),
                )
                .item(
                    item(
                        ws,
                        "Delete…",
                        Some(IconName::Delete),
                        move |_, window, cx| {
                            let ws = cx.entity();
                            let id = delete_id.clone();
                            if unsaved.is_empty() {
                                prompt::confirm(
                                    format!("Delete {delete_name}?"),
                                    why.clone(),
                                    "Delete",
                                    move |window, cx| {
                                        ws.update(cx, |ws, cx| ws.delete_space(&id, window, cx))
                                    },
                                    window,
                                    cx,
                                );
                                return;
                            }
                            // Its editors have changed files: save them, or not, first.
                            let (saver, save_id) = (ws.clone(), id.clone());
                            prompt::save_or_discard(
                                format!("Delete {delete_name}?"),
                                format!("{why} Unsaved: {}", unsaved.join(", ")),
                                move |window, cx| {
                                    saver.update(cx, |ws, cx| {
                                        let saved = ws.save_spaces(Some(&save_id), window, cx);
                                        if saved {
                                            ws.delete_space(&save_id, window, cx);
                                        }
                                        saved
                                    })
                                },
                                move |window, cx| {
                                    ws.update(cx, |ws, cx| ws.delete_space(&id, window, cx))
                                },
                                window,
                                cx,
                            );
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
        ws.edit_boards(|b| b.rename_board(board, name));
        cx.notify();
    });
    true
}
