//! One block, a continuous card: header (title, close button), body (a terminal's grid, or a
//! kanban board) and resize grip, in screen space. Every size is multiplied by the zoom, so text
//! is laid out at its real size. The close button and the grip show only while the pointer is
//! over the block.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::animation::Lerp as _;
use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _, Size as UiSize};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, BoxShadow, ClickEvent, Context, ElementId,
    FontWeight, Hsla, MouseButton, MouseDownEvent, ScrollWheelEvent, SharedString, TestSupportExt,
    Window, div, point, px,
};
use templum_canvas::{Block, BlockId, Edges, Level, Point, Rect, chrome};

use crate::alert::{self, Alert};
use crate::checklist;
use crate::editor;
use crate::git_block;
use crate::grid::{self, hsla};
use crate::kanban;
use crate::motion::{self, Look};
use crate::note;
use crate::workspace::agents_view::Doing;
use crate::workspace::remote::Deed;
use crate::workspace::{AgentState, Body, Copied, FindBar, Pane, Workspace};

/// World units, like everything in a block.
const TITLE_FONT: f32 = 12.0;
/// A kanban or checklist block's text: its cards', columns' and items'.
const BOARD_FONT: f32 = 13.0;
const RADIUS: f32 = 10.0;
/// The focused block's ring, outside its border.
const RING: f32 = 2.0;
/// The card's border (`border_1`), screen pixels: not zoomed.
pub(crate) const BORDER: f32 = 1.0;
/// How thick the strips along a block's edges that resize it are, screen pixels, at most: thinner
/// when zoomed out, so the title stays easy to grab.
const EDGE: f32 = 6.0;
/// Below Full a title is a label this far in from the card's border, and a terminal's status
/// dot in it is this wide: screen pixels, never zoomed.
const LABEL_IN: f32 = 4.0;
/// How far below a block's top its far-zoom label reaches, screen pixels: its inset, border,
/// padding and a line of text, with room to spare.
const LABEL_BAND: f32 = 32.0;
const DOT: f32 = 8.0;
/// The Templum Agent's mark before a title, world units.
const MARK: f32 = 10.0;

/// Why a mirror's block headers can't close: the same for every block, so asked once a frame,
/// not once a block.
pub(crate) struct Gates {
    close: Option<String>,
    /// The header buttons to every board, note and checklist: they list this computer's own.
    boards: Option<String>,
    notes: Option<String>,
    checklists: Option<String>,
}

impl Gates {
    pub(crate) fn of(ws: &Workspace) -> Gates {
        Gates {
            close: ws.why_not(Deed::Close),
            boards: ws.why_not(Deed::ShowBoard),
            notes: ws.why_not(Deed::OpenNote),
            checklists: ws.why_not(Deed::OpenChecklist),
        }
    }
}

/// `look` scales the block around its center (it is drawn at zoom × scale) and fades it.
/// A block that isn't `live` is a ghost fading out: no controls, and clicks go through it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    ws: &Workspace,
    block: Block,
    body: &Body,
    look: Look,
    live: bool,
    gates: &Gates,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let id = block.id;
    let n = id.0 as usize;
    // A note or checklist of a home before batch 2e is a card, as batch 1 drew it.
    let older = ws.older_content();
    let z = ws.space.view.zoom() * look.scale;
    // Bodies with gpui-kit editors glide too, but on the zooms where their rows are exact (see
    // `exact_rows`): their rows and their letters grow together. At rest, the zoom itself.
    let font = match body {
        Body::Editor(_) => editor::FONT,
        Body::Note(_) => note::FONT,
        Body::Terminal(_) | Body::Kanban(_) | Body::Checklist(_) | Body::Mirrored(_) => 0.0,
    };
    let settled = if font == 0.0 || (ws.space.flight.is_none() && look.scale == 1.0) {
        z
    } else {
        exact_rows(z, font)
    };
    let full = ws.space.view.rect_to_screen(block.rect);
    let r = Rect::new(
        full.x + full.w * (1.0 - look.scale) / 2.0,
        full.y + full.h * (1.0 - look.scale) / 2.0,
        full.w * look.scale,
        full.h * look.scale,
    );
    // Mid-glide, a text-heavy body is pulled toward the glide's pivot so the text there stays
    // under it, as a photo would: by how far its origin is from the pivot, times how much the
    // two zooms differ.
    let pull = match ws.space.flight {
        Some((flight, _)) if settled != z => {
            let origin = Point::new(
                r.x + BORDER + chrome::PAD * z,
                r.y + BORDER + (chrome::TITLE_H + chrome::PAD) * z,
            );
            let k = 1.0 - settled / z;
            Point::new(
                (flight.pivot.x - origin.x) * k,
                (flight.pivot.y - origin.y) * k,
            )
        }
        _ => Point::default(),
    };
    let focused = live && ws.space.focused == Some(id);
    // Selected blocks wear the focused one's ring and border, and so does the tile a held one
    // would trade places with.
    let marked = focused || (live && (ws.space.selected.contains(&id) || ws.dropping_on(id)));
    // How much of the body is drawn: by its text's size on screen at the zoom the camera rests at,
    // and by the block's own size there (docs/specs/level-of-detail.md, section 1).
    let size = block.rect.size();
    let level = match body {
        // Drawn in full down to the Card level: a board's cards have no bars.
        Body::Kanban(_) | Body::Checklist(_) | Body::Mirrored(_) => {
            match templum_canvas::block_level(BOARD_FONT, size, ws.space.rest) {
                Level::Silhouette => Level::Full,
                level => level,
            }
        }
        Body::Terminal(_) => templum_canvas::block_level(ws.cell.font_size, size, ws.space.rest),
        _ => templum_canvas::block_level(font, size, ws.space.rest),
    };
    // The block worked in (focused, or holding the keyboard: GPUI drops the focus of what isn't
    // drawn) has its letters where they would be bars; a card's would be specks under 2.5 px.
    let level = if level == Level::Silhouette && (focused || (live && body.has_focus(window, cx))) {
        Level::Full
    } else {
        level
    };
    // Too small on screen for its label, the block is its shape: the title covers it, with a
    // terminal's status dot, and nothing else is drawn on it.
    let shape = level == Level::Shape;
    let hovered = live && ws.space.hovered == Some(id);
    let ui = ws.ui;
    // 0 at rest, 1 hovered; eased both ways.
    let lift = transition(
        ElementId::from(("lift", n)),
        if hovered { 1.0_f32 } else { 0.0 },
        Transition::new(motion::HOVER),
        window,
        cx,
    );
    let mut shadows = vec![shadow(ui.shadow, focused, lift, z)];
    if marked {
        shadows.push(BoxShadow {
            color: ui.ring,
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(RING * z),
            inset: false,
        });
    }

    // The card's background, a note's tint included; a label sits on it too.
    let bg = match body {
        Body::Note(note) => note
            .tab()
            .and_then(|t| ws.notes_of(note)?.get(t.id)?.color)
            .map_or(ws.ui.surface, |color| ws.theme.note_bg(color)),
        _ => ws.ui.surface,
    };
    // Below Full the title is a label: never scaled, at the card's top-left corner, over the
    // header and the top of the body (docs/specs/level-of-detail.md, section 2).
    let labeled = shape || templum_canvas::level(TITLE_FONT, ws.space.rest) != Level::Full;
    // The Templum Agent's blocks wear its mark before their title, at the Full level.
    let uid = ws.space.uids.get(&id).map_or("", String::as_str);
    let mark = match body {
        Body::Terminal(pane)
            if !labeled && pane.opened_by.as_deref() == Some(templum_agent::ASKER) =>
        {
            Some(
                div()
                    .id(SharedString::from(format!("agent-mark-{uid}")))
                    .flex_none()
                    .child(
                        Icon::new(IconName::Sparkles)
                            .size(px(MARK * z))
                            .text_color(ui.accent),
                    )
                    .test_support(),
            )
        }
        _ => None,
    };
    // A block it just touched: its border goes to the accent twice.
    let now = cx.background_executor().now();
    let glow = ws
        .templum_agent
        .glow
        .get(uid)
        .and_then(|at| crate::chat::glow(*at, now));
    // An automatic copy's "Copied", fading in and out.
    let copied = ws
        .space
        .copied
        .as_ref()
        .filter(|c| live && c.block == id)
        .and_then(|c| Some((c, motion::copied(c.since, now)?)));
    if glow.is_some() || copied.is_some() {
        window.request_animation_frame();
    }
    let words = title(ws, body, cx);
    let title_text = div()
        .id(("title-text", n))
        .truncate()
        .when(labeled, |el| el.min_w(px(0.0)))
        .text_size(px(TITLE_FONT * if labeled { 1.0 } else { z }))
        .text_color(if focused { ui.title } else { ui.title_dim })
        .child(words.clone())
        .test_support();
    // An agent's block name, which agents call it by (`z0yfd`), after its title.
    let name = match body {
        Body::Terminal(pane) if pane.seen.agent.is_some() && !uid.is_empty() => Some(
            div()
                .id(("block-name", n))
                .flex_none()
                .text_size(px(TITLE_FONT * if labeled { 1.0 } else { z }))
                .text_color(ui.title_dim)
                .child(uid.to_string())
                .test_support(),
        ),
        _ => None,
    };
    // A terminal's state: on the left of its header, or in its label.
    let dot = match body {
        Body::Terminal(pane) if live => status_dot(ws, pane).map(|(color, text)| {
            div()
                .id(("status", n))
                .flex_shrink_0()
                .size(px(if shape {
                    // As big as the label's, or as the card is within its border.
                    DOT.min(r.w.min(r.h) - 2.0 * BORDER).max(0.0)
                } else if labeled {
                    DOT
                } else {
                    7.0 * z
                }))
                .rounded_full()
                .bg(color)
                .when(!labeled, |el| {
                    el.absolute()
                        .top(px((chrome::TITLE_H - 7.0) / 2.0 * z))
                        .left(px(12.0 * z))
                })
                .tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
                .test_support()
        }),
        _ => None,
    };
    let (label_dot, header_dot) = if labeled { (dot, None) } else { (None, dot) };
    // After the dot, over the title, which moves the block as before: who pressed keys in it for
    // a few seconds (docs/specs/connections.md), else its agent's session (docs/specs/usage.md).
    let chip = |id: &'static str, text: String, tip: String| {
        div()
            .id((id, n))
            .absolute()
            .top(px(4.0 * z))
            .left(px(24.0 * z))
            .h(px(20.0 * z))
            .flex()
            .items_center()
            .text_size(px(11.0 * z))
            .text_color(ui.title_dim)
            .child(text)
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .test_support()
    };
    let chip = match body {
        Body::Terminal(pane) if live && !labeled => match pane.answered_by() {
            Some(by) => Some(chip(
                "answered-by",
                format!("answered by {by}"),
                format!("{by} pressed keys in this terminal"),
            )),
            None => pane
                .usage
                .as_ref()
                .and_then(crate::usage::chip)
                .map(|(text, tip)| {
                    // A mirrored agent's effort is home's: its block has no place for it but here.
                    let effort = pane
                        .remote
                        .as_ref()
                        .and_then(|r| r.agent.as_ref())
                        .and_then(|a| a.effort.as_deref());
                    chip("usage-chip", text, crate::usage::with_effort(tip, effort))
                }),
        },
        _ => None,
    };
    // After the title, at Full: a clock and how many schedules its agent set, which its tooltip
    // lists (docs/specs/agent-tools.md, What the person sees).
    let clock = match body {
        Body::Terminal(pane) if live && !labeled && !ws.schedules.is_empty() => {
            let uid = ws.space.uids.get(&id).cloned().unwrap_or_default();
            let lines = ws.schedule_lines(pane.term.id(), &uid);
            (!lines.is_empty()).then(|| {
                let (count, tip) = (lines.len(), lines.join("\n"));
                div()
                    .id(ElementId::Name(format!("schedules-{n}").into()))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(3.0 * z))
                    .text_size(px(11.0 * z))
                    .text_color(ui.title_dim)
                    .child(
                        Icon::new(IconName::Clock)
                            .size(px(11.0 * z))
                            .text_color(ui.title_dim),
                    )
                    .child(count.to_string())
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .test_support()
            })
        }
        _ => None,
    };
    let title = if shape {
        // The whole card, its dot in the middle; hovering it names the block.
        div()
            .id(("title", n))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .children(label_dot)
            .tooltip(move |window, cx| Tooltip::new(words.clone()).build(window, cx))
    } else if labeled {
        div()
            .id(("title", n))
            .absolute()
            .top(px(LABEL_IN))
            .left(px(LABEL_IN))
            .max_w(px((r.w - 2.0 * BORDER - 2.0 * LABEL_IN).max(0.0)))
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(6.0))
            .py(px(2.0))
            .overflow_hidden()
            .rounded(px(4.0))
            .border_1()
            .border_color(ui.hairline)
            .bg(bg)
            .children(label_dot)
            .child(title_text)
            .children(name)
    } else {
        div()
            .id(("title", n))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            // Clear of the ⋯, fullscreen and close buttons on the right (and of a terminal's chip
            // on the left), and centered.
            .px(px(if chip.is_some() { 120.0 } else { 76.0 } * z))
            .gap(px(6.0 * z))
            .overflow_hidden()
            .gap(px(4.0 * z))
            .children(mark)
            .child(title_text)
            .children(name)
            .children(clock)
    }
    // The title is where the block is held to move it: an open hand over it, a closed one
    // while it moves (CSS's grab and grabbing).
    .when(live, |el| {
        if ws.moving(id) {
            el.cursor_grabbing()
        } else {
            el.cursor_grab()
        }
    })
    .when(live, |el| {
        el.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                // Shift+click selects the block, or deselects it; it neither moves nor focuses.
                if e.modifiers.shift {
                    ws.toggle_selected(id, window, cx);
                    cx.stop_propagation();
                    return;
                }
                // A double-click frames the block; the camera flies, so nothing moves.
                if e.click_count == 2 {
                    ws.frame_block(id, window, cx);
                } else if ws.space.selected.contains(&id) {
                    // A selected block's title moves every selected block, still selected.
                    ws.begin_group_move(e.position, None);
                    cx.stop_propagation();
                    return;
                } else {
                    ws.begin_move(id, e.position);
                }
                // A label lies over the body: it takes the press itself, so a terminal under it
                // starts no selection, and it focuses the block as the card would.
                if labeled {
                    ws.focus_block(id, window, cx);
                    cx.stop_propagation();
                }
            }),
        )
    })
    .test_support();

    // On a mirror, what home can't do from here is dimmed, its tooltip the rule's words.
    let no_close = gates.close.clone();
    // Painted over the title: pressing it never starts a move.
    let close = div()
        .id(("close", n))
        .absolute()
        .top(px(4.0 * z))
        .right(px(4.0 * z))
        .flex()
        .items_center()
        .justify_center()
        .size(px(20.0 * z))
        .rounded(px(5.0 * z))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(
            Icon::new(IconName::Close)
                .size(px(12.0 * z))
                .text_color(ui.title),
        )
        .when_some(no_close, |el, why| {
            el.opacity(0.4)
                .tooltip(move |window, cx| Tooltip::new(why.clone()).build(window, cx))
        })
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| ws.close(id, window, cx)))
        .test_support();

    // Beside it: the block full screen (grid mode), or back to the canvas.
    let tiled = ws.space.grid.is_some();
    let fullscreen = div()
        .id(("fullscreen", n))
        .absolute()
        .top(px(4.0 * z))
        .right(px(28.0 * z))
        .flex()
        .items_center()
        .justify_center()
        .size(px(20.0 * z))
        .rounded(px(5.0 * z))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(
            Icon::new(if tiled {
                IconName::Minimize2
            } else {
                IconName::Maximize2
            })
            .size(px(12.0 * z))
            .text_color(ui.title),
        )
        .tooltip(move |window, cx| {
            Tooltip::new(if tiled {
                "Back to the canvas"
            } else {
                "Fullscreen"
            })
            .build(window, cx)
        })
        // The block isn't focused by this press: that would drop the selection going full screen.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| ws.fullscreen(id, window, cx)))
        .test_support();

    // Beside those: the block's right-click menu, for whoever doesn't know it is there.
    let more = div()
        .id(("more", n))
        .absolute()
        .top(px(4.0 * z))
        .right(px(52.0 * z))
        .flex()
        .items_center()
        .justify_center()
        .size(px(20.0 * z))
        .rounded(px(5.0 * z))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(
            Icon::new(IconName::Ellipsis)
                .size(px(12.0 * z))
                .text_color(ui.title),
        )
        .tooltip(|window, cx| Tooltip::new("More").build(window, cx))
        // Pressing it doesn't focus the block: that would drop the selection its menu carries.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |ws, e: &ClickEvent, window, cx| {
            ws.open_block_menu(id, e.position(), window, cx)
        }))
        .test_support();

    // A kanban block's button to every board, on the left of its header (over the title: no move).
    let boards = match body {
        Body::Kanban(_) if live && !labeled => Some(header_button(
            ws,
            ("boards", n).into(),
            IconName::LayoutDashboard,
            "All boards",
            gates.boards.clone(),
            z,
            move |ws, window, cx| {
                let space = ws.space.id.clone();
                kanban::open_boards(space, id, window, cx)
            },
            cx,
        )),
        _ => None,
    };
    // A note block's button to every note, on the left of its header (over the title: no move).
    let preview = match body {
        Body::Note(_) if live && !labeled => Some(header_button(
            ws,
            ("notes", n).into(),
            IconName::NotebookText,
            "All notes",
            gates.notes.clone(),
            z,
            move |ws, window, cx| note::open_notes(ws.space.id.clone(), Some(id), window, cx),
            cx,
        )),
        _ => None,
    };
    // A checklist block's button to every checklist, there too.
    let all_checklists = match body {
        Body::Checklist(_) if live && !labeled => Some(header_button(
            ws,
            ("checklists", n).into(),
            IconName::ListChecks,
            "All checklists",
            gates.checklists.clone(),
            z,
            move |ws, window, cx| {
                checklist::open_checklists(ws.space.id.clone(), Some(id), window, cx)
            },
            cx,
        )),
        _ => None,
    };
    // An editor's file-tree toggle, on the left of its header (over the title: no move).
    let tree_toggle = match body {
        Body::Editor(e) if live && !labeled => {
            let shown = e.show_tree;
            Some(
                div()
                    .id(("tree-toggle", n))
                    .absolute()
                    .top(px(4.0 * z))
                    .left(px(4.0 * z))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(20.0 * z))
                    .rounded(px(5.0 * z))
                    .cursor_pointer()
                    .hover(move |s| s.bg(ui.hover))
                    .child(
                        Icon::new(if shown {
                            IconName::PanelLeftClose
                        } else {
                            IconName::PanelLeftOpen
                        })
                        .size(px(12.0 * z))
                        .text_color(ui.title),
                    )
                    .tooltip(move |window, cx| {
                        Tooltip::new(if shown {
                            "Hide the file tree"
                        } else {
                            "Show the file tree"
                        })
                        .build(window, cx)
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.toggle_tree(id, cx)))
                    .test_support(),
            )
        }
        _ => None,
    };
    // The resize strips, on the block under the pointer only (the pointer reaches an edge from
    // inside the block): the top ones under the header's buttons, the others over the body.
    let edge = (EDGE * z).clamp(2.0, EDGE);
    // A tile's size is the grid's: it has no resize strips nor grip; a shape is moved only.
    let (top_zones, other_zones) = if hovered && !tiled && !shape {
        let mut zone = |edges| resize_zone(id, edges, edge, cx);
        (
            vec![
                zone(Edges::TOP),
                zone(Edges::TOP_LEFT),
                zone(Edges::TOP_RIGHT),
            ],
            vec![
                zone(Edges::LEFT),
                zone(Edges::RIGHT),
                zone(Edges::BOTTOM),
                zone(Edges::BOTTOM_LEFT),
                zone(Edges::BOTTOM_RIGHT),
            ],
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let header = if shape {
        title.into_any_element()
    } else {
        div()
            .relative()
            .w_full()
            .h(px(chrome::TITLE_H * z))
            .child(title)
            .children(top_zones)
            .children(header_dot)
            .children(chip)
            .children(boards)
            .children(preview)
            .children(all_checklists)
            .children(tree_toggle)
            .when(hovered, |el| {
                el.child(fade_in(more, ("more-fade", n)))
                    .child(fade_in(fullscreen, ("fullscreen-fade", n)))
                    .child(fade_in(close, ("close-fade", n)))
            })
            .into_any_element()
    };

    // Drawn at the Full level only.
    let covered = match body {
        Body::Terminal(pane) => level == Level::Full && pane.find.is_some(),
        _ => false,
    };
    let terminal = matches!(body, Body::Terminal(_));
    let frame = div()
        .id(("body", n))
        .absolute()
        .left(px(0.0))
        .top(px(chrome::TITLE_H * z))
        .w_full()
        .h(px((r.h - chrome::TITLE_H * z).max(0.0)))
        .p(px(chrome::PAD * z))
        .overflow_hidden()
        // A selected block's body holds the selection like its title does: a press (before the
        // content hears it) moves every selected block, and a click picks this block; Shift+click
        // takes it out. A far label over the body's top is the title: there it is the title's press
        // (a click keeps the selection), taken here because the body's content, a kanban's or a
        // checklist's, hears presses ahead of the label. Left to the block's own handlers: the
        // resize zones over the body and a terminal's find field (drawn over it, and not occluding
        // it; the notice of a request does occlude it), and a terminal's Ctrl+click (a link opens).
        .when(
            live && !tiled && !covered && ws.space.selected.contains(&id),
            |el| {
                el.capture_any_mouse_down(cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                    let p = ws.local(e.position);
                    if e.button != MouseButton::Left
                        || (terminal && e.modifiers.secondary())
                        || !chrome::on_body(r, edge, z, p)
                    {
                        return;
                    }
                    if e.modifiers.shift {
                        ws.toggle_selected(id, window, cx);
                    } else {
                        let label = labeled && p.y < r.y + LABEL_BAND;
                        if label && e.click_count == 2 {
                            // As the label does: the camera flies to the block, which is picked.
                            ws.frame_block(id, window, cx);
                            ws.focus_block(id, window, cx);
                        } else {
                            ws.begin_group_move(e.position, (!label).then_some(id));
                        }
                    }
                    cx.stop_propagation();
                }))
            },
        );
    let content = match body {
        // Nothing inside: a terminal's grid isn't painted, so the terminal isn't re-armed and its
        // output asks for no frames, as off screen; no body's text is laid out.
        _ if level >= Level::Card => frame,
        Body::Terminal(pane) => {
            frame
                .when(live, |el| {
                    el.cursor_text().on_any_mouse_down(cx.listener(
                        move |ws, e: &MouseDownEvent, window, cx| {
                            let left = e.button == MouseButton::Left;
                            // Ctrl+click (Cmd on macOS) opens a path or a link; the block's own
                            // press would take the keyboard back from the editor it opened.
                            if left && e.modifiers.secondary() && ws.open_link(id, e, window, cx) {
                                cx.stop_propagation();
                            } else if ws.press_program(id, e, window, cx) {
                                // The card focuses on a left press; the middle and right ones
                                // are the program's, not the canvas's pan nor the block's menu.
                                if !left {
                                    ws.focus_block(id, window, cx);
                                    cx.stop_propagation();
                                }
                            } else if left {
                                ws.begin_select(id, e);
                            }
                        },
                    ))
                })
                // Over bars the wheel moves the canvas: a far terminal's history stays still.
                .when(live && level == Level::Full, |el| {
                    el.on_scroll_wheel(cx.listener(move |ws, e: &ScrollWheelEvent, window, cx| {
                        ws.scroll_terminal(id, e, window, cx)
                    }))
                })
                .map(|el| {
                    // Home's whole grid fits the block: at the zoom step that holds it.
                    let grid_zoom = if pane.remote.is_some() {
                        crate::workspace::remote::fit_zoom(
                            z,
                            chrome::content_rect(block.rect).size(),
                            pane.grid_size(),
                            ws.cell.size,
                        )
                    } else {
                        z
                    };
                    let grid = grid::body(
                        pane.term.shared().clone(),
                        pane.snapshot.clone(),
                        ws.cell.clone(),
                        grid_zoom,
                        level == Level::Silhouette,
                        focused,
                        ws.marked.clone().filter(|_| focused),
                        ws.theme.clone(),
                        ws.blink_on,
                        cx.entity(),
                    );
                    if level == Level::Full {
                        el.child(grid)
                    } else {
                        el.child(
                            div()
                                .id(("silhouette", n))
                                .size_full()
                                .child(grid)
                                .test_support(),
                        )
                    }
                })
                // Up close only, like the wheel that scrolls it: far away the wheel moves the
                // canvas and a terminal's history stays still.
                .when(live && level == Level::Full, |el| {
                    let view = pane
                        .term
                        .shared()
                        .scrollback()
                        .unwrap_or_else(|| pane.snapshot.borrow().scrollback);
                    // From the text's top to the grip's square: the grip keeps its presses.
                    let h =
                        (r.h - 2.0 * BORDER - (chrome::TITLE_H + chrome::PAD + chrome::HANDLE) * z)
                            .max(0.0);
                    el.when(view.bar(), |el| {
                        el.child(history_bar(
                            ws,
                            id,
                            pane.term.shared(),
                            view,
                            h,
                            z,
                            window,
                            cx,
                        ))
                    })
                    .when(view.below > 0, |el| {
                        el.child(to_bottom(ws, id, view.below, z, cx))
                    })
                })
                // Its find field is kept while the block is far, and comes back with its letters.
                .when_some(
                    pane.find.as_ref().filter(|_| live && level == Level::Full),
                    |el, bar| el.child(find_bar(ws, n, bar, z)),
                )
                .when(
                    live && level == Level::Full
                        && pane.remote.as_ref().is_some_and(|r| r.read_only),
                    |el| el.child(read_only_footer(ws, z)),
                )
                .when_some(pane.starting.as_ref().filter(|_| live), |el, s| {
                    el.child(starting_cover(ws, n, s.title.clone(), z))
                })
                .when_some(copied, |el, (c, opacity)| {
                    el.child(copied_pill(ws, n, c, opacity, r, z))
                })
        }
        // A fading kanban block is drawn empty: its board is still live elsewhere.
        Body::Kanban(k) => frame.when(live, |el| el.child(kanban::body(ws, id, *k, z, window, cx))),
        // A fading checklist block is drawn empty too: its checklists are live elsewhere.
        Body::Checklist(c) => frame.when(live, |el| match older.as_ref().filter(|_| c.remote) {
            Some(older) => el.child(mirrored_card(ws, n, "checklist", &c.title(ws), older, z)),
            None => el.child(checklist::body(ws, id, c, z, cx)),
        }),
        // A fading note is drawn empty, like a kanban block.
        // Below Full a note shown rendered is its tinted card, empty: gpui-kit's Markdown view
        // has no layout to draw bars from.
        Body::Note(note) => frame.when(live, |el| match older.as_ref().filter(|_| note.remote) {
            Some(older) => el.child(mirrored_card(ws, n, "note", &note.title(ws), older, z)),
            None => el.child(settled_box(
                block,
                settled,
                pull,
                note::body(ws, id, note, settled, level, cx),
            )),
        }),
        // A block of home's the mirror doesn't draw yet: a card in its place.
        Body::Mirrored(card) => frame.when(live, |el| {
            let computer = ws
                .space
                .remote
                .as_deref()
                .map_or("the other computer".to_string(), crate::mirror::computer_of);
            let sentence = format!("Not mirrored yet: open it on {computer}.");
            el.child(mirrored_card(ws, n, &card.kind, &card.title, &sentence, z))
        }),
        // A fading editor is drawn empty, like a note.
        Body::Editor(e) => frame.when(live, |el| {
            el.child(settled_box(
                block,
                settled,
                pull,
                editor::body(ws, id, e, settled, level, cx),
            ))
        }),
    }
    .test_support();

    let handle = div()
        .id(("handle", n))
        .absolute()
        .right(px(0.0))
        .bottom(px(0.0))
        .size(px(chrome::HANDLE * z))
        .flex()
        .items_center()
        .justify_center()
        .cursor_nwse_resize()
        .child(
            Icon::new(IconName::ResizeCorner)
                .size(px(10.0 * z))
                .text_color(ui.title_dim),
        )
        .on_mouse_down(
            MouseButton::Left,
            // The handle overlaps the body: stop here so the body doesn't start a selection.
            cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                ws.focus_block(id, window, cx);
                ws.begin_resize(id, Edges::BOTTOM_RIGHT, e.position);
                cx.stop_propagation();
            }),
        )
        .test_support();

    let root: ElementId = if live {
        ("block", n).into()
    } else {
        ("ghost", n).into()
    };
    div()
        .id(root)
        .absolute()
        .left(px(r.x))
        .top(px(r.y))
        .w(px(r.w))
        .h(px(r.h))
        .opacity(look.opacity)
        .rounded(px(RADIUS * z))
        // Always 1 px, so focusing never moves the grid.
        .border_1()
        .border_color(if marked {
            ui.accent
        } else {
            let border = ui.hairline.lerp(&ui.hover_border, lift);
            glow.map_or(border, |k| border.lerp(&ui.accent, k))
        })
        .bg(bg)
        .shadow(shadows)
        .overflow_hidden()
        .when(live, |el| {
            // Nothing behind a block reacts to its clicks; wheel events still bubble
            // (title bar pans).
            el.block_mouse_except_scroll()
                .on_hover(
                    cx.listener(move |ws, hovered: &bool, _, cx| ws.hover_block(id, *hovered, cx)),
                )
                // Blocking hides the workspace from pinch (it hit-tests like a click): zoom
                // here too.
                .on_pinch(cx.listener(|ws, e, window, cx| ws.on_pinch(e, window, cx)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                        ws.focus_block(id, window, cx);
                        cx.stop_propagation();
                    }),
                )
                // The middle button moves the canvas, over blocks too.
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(|ws, e: &MouseDownEvent, _, cx| {
                        ws.begin_pan(e.position);
                        cx.stop_propagation();
                    }),
                )
                // Its menu, which the header's ⋯ opens too.
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                        ws.open_block_menu(id, e.position, window, cx);
                        cx.stop_propagation();
                    }),
                )
        })
        .map(|el| {
            // A label lies over the top of the body: the header is drawn after it then.
            if labeled {
                el.child(content).child(header)
            } else {
                el.child(header).child(content)
            }
        })
        .children(other_zones)
        .when(hovered && !tiled && !shape, |el| {
            el.child(fade_in(handle, ("handle-fade", n)))
        })
        // That its agent waits for an answer, up close only; the inbox has the request. And that
        // answers wait for the person's message to be sent.
        .children(
            match body {
                Body::Terminal(pane) if live && level == Level::Full => [
                    crate::inbox::notice(ws, id, pane.term.id(), z, cx),
                    answers_chip(ws, id, pane.term.id(), z),
                ],
                _ => [None, None],
            }
            .into_iter()
            .flatten(),
        )
        .test_support()
        .into_any_element()
}

/// The footer of a remote terminal that takes no input (a subagent's transcript).
fn read_only_footer(ws: &Workspace, z: f32) -> AnyElement {
    div()
        .absolute()
        .left(px(0.0))
        .bottom(px(0.0))
        .w_full()
        .overflow_hidden()
        .truncate()
        .px(px(chrome::PAD * z))
        .text_size(px(11.0 * z))
        .text_color(ws.ui.title_dim)
        .child("Read-only: a subagent's transcript")
        .into_any_element()
}

/// What a block of home's that is not mirrored yet shows in its place: its kind's icon, its title
/// and where to open it.
fn mirrored_card(
    ws: &Workspace,
    n: usize,
    kind: &str,
    title: &str,
    sentence: &str,
    z: f32,
) -> AnyElement {
    let ui = ws.ui;
    let icon = match kind {
        "kanban" => IconName::LayoutDashboard,
        "note" => IconName::NotebookText,
        "checklist" => IconName::ListChecks,
        "editor" => IconName::FileCode,
        _ => IconName::Box,
    };
    let kind = if matches!(kind, "kanban" | "note" | "checklist" | "editor") {
        String::new()
    } else {
        format!("{kind} · ")
    };
    div()
        .id(("mirrored", n))
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.0 * z))
        .overflow_hidden()
        .child(Icon::new(icon).size(px(24.0 * z)).text_color(ui.title_dim))
        .child(
            div()
                .w_full()
                .overflow_hidden()
                .truncate()
                .text_center()
                .text_size(px(13.0 * z))
                .text_color(ui.title)
                .child(format!("{kind}{title}")),
        )
        .child(
            div()
                .w_full()
                .overflow_hidden()
                .truncate()
                .text_center()
                .text_size(px(11.0 * z))
                .text_color(ui.title_dim)
                .child(sentence.to_string()),
        )
        .test_support()
        .into_any_element()
}

/// What the chip of a terminal whose agent has answers waiting for its person's message reads.
pub(crate) fn answers_text(count: usize) -> String {
    match count {
        1 => "1 answer waiting".to_string(),
        count => format!("{count} answers waiting"),
    }
}

/// The chip over the terminal block `id`, whose terminal is `terminal`, at zoom `z`: answers from
/// other agents wait for its person's unsent message (`Pane::has_draft`). A click does nothing but
/// show why. It sits over the inbox's notice when that shows. None when no answer waits.
fn answers_chip(ws: &Workspace, id: BlockId, terminal: &str, z: f32) -> Option<AnyElement> {
    let count = ws.answers_waiting(terminal);
    if count == 0 {
        return None;
    }
    let lift = if ws.asking(terminal).is_some() {
        34.0
    } else {
        0.0
    };
    // The yellow of the dot of a block waiting for its person.
    let waiting = hsla(ws.theme.palette.ansi[3], 1.0);
    Some(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px((12.0 + lift) * z))
            .flex()
            .justify_center()
            .child(
                div()
                    .id(ElementId::Name(format!("answers-waiting-{}", id.0).into()))
                    .flex()
                    .items_center()
                    .gap(px(6.0 * z))
                    .px(px(10.0 * z))
                    .py(px(5.0 * z))
                    .rounded(px(8.0 * z))
                    .bg(ws.ui.surface)
                    .border_1()
                    .border_color(waiting)
                    .text_size(px(12.0 * z))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(hsla(ws.theme.palette.foreground, 1.0))
                    // A press on it reaches neither its block nor the terminal under it.
                    .occlude()
                    .child(
                        Icon::new(IconName::Clock)
                            .size(px(14.0 * z))
                            .text_color(waiting),
                    )
                    .child(answers_text(count))
                    .tooltip(|window, cx| {
                        Tooltip::new("They go in once your message is sent or cleared")
                            .build(window, cx)
                    })
                    .test_support(),
            )
            .into_any_element(),
    )
}

/// Over an agent's terminal while it starts (`starting.rs`): its name and a spinner, not its
/// shell typing the command.
fn starting_cover(ws: &Workspace, n: usize, title: SharedString, z: f32) -> impl IntoElement {
    div()
        .id(("starting", n))
        .absolute()
        .inset_0()
        .bg(ws.ui.surface)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(12.0 * z))
        .child(Spinner::new().with_size(UiSize::Size(px(22.0 * z))))
        .child(
            div()
                .text_size(px(15.0 * z))
                .text_color(ws.ui.title)
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.0 * z))
                .text_color(ws.ui.hint)
                .child("Starting…"),
        )
        .test_support()
}

/// What an automatic copy shows where it was let go, inside the terminal's body: "Copied", or
/// how many lines. It has no listener: clicks go through it to the grid.
fn copied_pill(ws: &Workspace, n: usize, c: &Copied, opacity: f32, r: Rect, z: f32) -> AnyElement {
    const H: f32 = 22.0;
    const GAP: f32 = 8.0;
    let ui = ws.ui;
    // The body on screen, the pointer in it, kept there: a drag may end outside the block.
    let (w, h) = (r.w - 2.0 * BORDER, r.h - chrome::TITLE_H * z);
    let (x, y) = ((c.at.x * z).clamp(0.0, w), (c.at.y * z).clamp(0.0, h));
    let (pad, gap) = (chrome::PAD * z, GAP * z);
    let label = if c.lines > 1 {
        format!("Copied {} lines", c.lines)
    } else {
        "Copied".to_string()
    };
    let pill = div()
        .id(("copied", n))
        .absolute()
        .h(px(H * z))
        .flex()
        .items_center()
        .gap(px(4.0 * z))
        .px(px(8.0 * z))
        .rounded(px(6.0 * z))
        .bg(ui.surface.blend(ui.card))
        .border_1()
        .border_color(ui.hairline)
        .shadow(vec![BoxShadow {
            color: ui.shadow,
            offset: point(px(0.0), px(2.0 * z)),
            blur_radius: px(8.0 * z),
            spread_radius: px(0.0),
            inset: false,
        }])
        .opacity(opacity)
        .whitespace_nowrap()
        .text_size(px(TITLE_FONT * z))
        .text_color(ui.title)
        .child(
            Icon::new(IconName::Check)
                .size(px(12.0 * z))
                .text_color(ui.accent),
        )
        .child(label);
    // It grows away from the nearer side, so it stays in the body; above the pointer, or below
    // it near the top.
    let pill = if x <= w / 2.0 {
        pill.left(px(x.max(pad)))
    } else {
        pill.right(px((w - x).max(pad)))
    };
    let pill = if y >= pad + H * z + gap {
        pill.bottom(px(h - y + gap))
    } else {
        pill.top(px((y + gap).min(h - pad - H * z)))
    };
    pill.test_support().into_any_element()
}

/// A button on the left of a block's header (over the title: no move), `tip` its tooltip. With a
/// `why` (a mirrored workspace's rule) it is dimmed, the words its tooltip, and its click does
/// nothing.
#[allow(clippy::too_many_arguments)]
pub(crate) fn header_button(
    ws: &Workspace,
    id: ElementId,
    icon: IconName,
    tip: &'static str,
    why: Option<String>,
    z: f32,
    click: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let dimmed = why.is_some();
    let tip = why.unwrap_or_else(|| tip.to_string());
    div()
        .id(id)
        .absolute()
        .top(px(4.0 * z))
        .left(px(4.0 * z))
        .flex()
        .items_center()
        .justify_center()
        .size(px(20.0 * z))
        .rounded(px(5.0 * z))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(Icon::new(icon).size(px(12.0 * z)).text_color(ui.title))
        .when(dimmed, |el| el.opacity(0.4))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, cx| {
            cx.stop_propagation()
        })
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
            if !dimmed {
                click(ws, window, cx)
            }
        }))
        .test_support()
        .into_any_element()
}

/// A block's title: a terminal's, a kanban block's board, a note's name, an editor's file,
/// a Git block's repository.
pub(crate) fn title(ws: &Workspace, body: &Body, _cx: &App) -> SharedString {
    match body {
        Body::Terminal(pane) if pane.died_starting() => "The shell ended".into(),
        Body::Terminal(pane) => match &pane.remote {
            Some(remote) => SharedString::from(remote.title.clone()),
            None => SharedString::from(pane.term.title()),
        },
        Body::Kanban(k) => k.title(ws),
        Body::Note(n) => n.title(ws),
        Body::Checklist(c) => c.title(ws),
        Body::Editor(e) => e.title(),
        Body::Mirrored(m) => SharedString::from(m.title.clone()),
    }
}

/// A terminal's find field, over its top right corner; "No matches" beside it when so.
fn find_bar(ws: &Workspace, n: usize, bar: &FindBar, z: f32) -> AnyElement {
    div()
        .id(("find", n))
        .absolute()
        .top(px(chrome::PAD * z))
        .right(px(chrome::PAD * z))
        .w(px(260.0 * z))
        .flex()
        .items_center()
        .gap(px(6.0 * z))
        .p(px(4.0 * z))
        .rounded(px(6.0 * z))
        .bg(ws.ui.surface)
        .border_1()
        .border_color(ws.ui.hairline)
        // Clicks here are the field's, not a selection's in the grid below.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(git_block::field(&bar.input, z).flex_1())
        .when(bar.missing, |el| {
            el.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(git_block::FONT * z))
                    .text_color(hsla(ws.theme.palette.ansi[1], 1.0))
                    .child("No matches"),
            )
        })
        .test_support()
        .into_any_element()
}

/// A terminal's history as the kit's scrollbar takes it: a track `height` pixels high over its
/// history and its screen, the thumb where the view is.
#[derive(Clone)]
struct History {
    term: templum_terminal::SharedTerm,
    /// The view as it was drawn, then where the thumb put it.
    view: Rc<Cell<templum_terminal::Scrollback>>,
    height: f32,
}

/// Further than any history: the offset of a view at the live screen (see `History::offset`).
const AT_THE_END: f32 = 1.0e9;

impl History {
    fn content(&self) -> f32 {
        self.height / self.view.get().thumb().1
    }
}

impl gpui_kit::component::scroll::ScrollbarHandle for History {
    /// Unused: the scrollbar takes its strip's bounds (`viewport_from_layout`).
    fn viewport_bounds(&self) -> gpui_kit::Bounds<gpui_kit::Pixels> {
        gpui_kit::Bounds::default()
    }

    /// At the live screen, an offset no history reaches: the kit keeps the thumb at the end of
    /// the track, and output that lengthens the history doesn't read as the person scrolling,
    /// which would show the bar while a program prints.
    fn offset(&self) -> gpui_kit::Point<gpui_kit::Pixels> {
        let view = self.view.get();
        let y = if view.below == 0 {
            AT_THE_END
        } else {
            view.thumb().0 * self.content()
        };
        point(px(0.0), px(-y))
    }

    fn set_offset(&self, offset: gpui_kit::Point<gpui_kit::Pixels>) {
        let view = self.view.get();
        let below = view.below_at(-f32::from(offset.y) / self.content());
        self.term.scroll_to(below);
        self.view
            .set(templum_terminal::Scrollback { below, ..view });
    }

    fn content_size(&self) -> gpui_kit::Size<gpui_kit::Pixels> {
        gpui_kit::size(px(0.0), px(self.content()))
    }
}

/// Which terminal's scrollbar shows: the one whose strip is under the pointer (its thumb held
/// too, which keeps the pointer's moves), and the one the person scrolled, for the theme's idle.
/// Not the kit's own showing, which would take a history that grows at the live screen, and a bar
/// drawn again (zoomed back in, its workspace shown again), for the person scrolling.
#[derive(Default)]
pub(crate) struct Bars {
    pub hovered: Option<BlockId>,
    scrolled: Option<(BlockId, std::time::Instant)>,
    /// A frame once the idle is over, to fade the bar.
    _timer: Option<gpui_kit::Task<()>>,
}

/// The theme's scrollbar timings (its idle, its fades), unless no theme is set.
fn scrollbar_motion(cx: &App) -> gpui_kit::component::scroll::ScrollbarMotion {
    cx.try_global::<gpui_kit::base::Theme>()
        .map(|theme| theme.scrollbar.motion())
        .unwrap_or_default()
}

impl Workspace {
    /// The person scrolled terminal `id`'s history: its scrollbar shows, then fades.
    pub(crate) fn scrolled_history(&mut self, id: BlockId, cx: &mut Context<Workspace>) {
        let idle = scrollbar_motion(cx).idle();
        self.bars.scrolled = Some((id, cx.background_executor().now()));
        self.bars._timer = Some(cx.spawn(async move |ws, cx| {
            cx.background_executor().timer(idle).await;
            let _ = ws.update(cx, |_, cx| cx.notify());
        }));
    }
}

/// A terminal's scrollbar, over the right of its text, `h` high (down to the resize grip):
/// the app's scrollbar, shown while the pointer is over its strip and while the person scrolls
/// the history, then fading. Hidden, the strip is the text's. Shown, it takes the presses that go
/// to the bar: an unfocused block is focused, and the middle button and a right-click do what
/// they do over the rest of the block.
#[allow(clippy::too_many_arguments)]
fn history_bar(
    ws: &Workspace,
    id: BlockId,
    term: &templum_terminal::SharedTerm,
    view: templum_terminal::Scrollback,
    h: f32,
    z: f32,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let n = id.0 as usize;
    let motion = scrollbar_motion(cx);
    let now = cx.background_executor().now();
    let shown = ws.bars.hovered == Some(id)
        || ws
            .bars
            .scrolled
            .is_some_and(|(b, at)| b == id && now.saturating_duration_since(at) < motion.idle());
    let opacity = transition(
        ElementId::from(("history-shown", n)),
        if shown { 1.0_f32 } else { 0.0 },
        Transition::new(if shown { motion.enter() } else { motion.exit() }),
        window,
        cx,
    );
    let history = History {
        term: term.clone(),
        view: Rc::new(Cell::new(view)),
        height: h,
    };
    div()
        .id(("history-bar", n))
        .absolute()
        .top(px(chrome::PAD * z))
        .right(px(chrome::PAD * z))
        .w(px(kanban::TRACK * z))
        .h(px(h))
        .on_hover(cx.listener(move |ws, hovered: &bool, _, cx| {
            if *hovered {
                ws.bars.hovered = Some(id);
            } else if ws.bars.hovered == Some(id) {
                ws.bars.hovered = None;
            }
            cx.notify();
        }))
        .when(opacity > 0.0, |el| {
            el.capture_any_mouse_down(cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                if ws.space.focused != Some(id) {
                    ws.focus_block(id, window, cx);
                }
                match e.button {
                    MouseButton::Middle => {
                        ws.begin_pan(e.position);
                        cx.stop_propagation();
                    }
                    MouseButton::Right => {
                        ws.open_block_menu(id, e.position, window, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .child(
                div()
                    .id(("history-scrollbar", n))
                    .absolute()
                    .inset_0()
                    .opacity(opacity)
                    .child(
                        kanban::scrollbar(&history, z)
                            .mode(gpui_kit::component::scroll::ScrollbarMode::Always),
                    )
                    .test_support(),
            )
        })
        .test_support()
        .into_any_element()
}

/// While a terminal is scrolled back, at its body's bottom right: how many lines are under the
/// view, a click going back to the live screen (typing does too). The look of the Copied pill.
fn to_bottom(
    ws: &Workspace,
    id: BlockId,
    below: usize,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    const H: f32 = 22.0;
    const GAP: f32 = 8.0;
    let ui = ws.ui;
    let bg = ui.surface.blend(ui.card);
    let label = if below == 1 {
        "1 line below".to_string()
    } else {
        format!("{below} lines below")
    };
    div()
        .id(("to-bottom", id.0 as usize))
        .absolute()
        // Clear of the resize grip, which shows in the corner while the pointer is over the block.
        .right(px((chrome::PAD + GAP) * z))
        .bottom(px((chrome::PAD + GAP) * z))
        .h(px(H * z))
        .flex()
        .items_center()
        .gap(px(4.0 * z))
        .px(px(8.0 * z))
        .rounded(px(6.0 * z))
        .bg(bg)
        .border_1()
        .border_color(ui.hairline)
        .shadow(vec![BoxShadow {
            color: ui.shadow,
            offset: point(px(0.0), px(2.0 * z)),
            blur_radius: px(8.0 * z),
            spread_radius: px(0.0),
            inset: false,
        }])
        .whitespace_nowrap()
        .text_size(px(TITLE_FONT * z))
        .text_color(ui.title)
        .cursor_pointer()
        .hover(move |s| s.bg(bg.blend(ui.hover)))
        .child(
            Icon::new(IconName::ArrowDown)
                .size(px(12.0 * z))
                .text_color(ui.accent),
        )
        .child(label)
        .tooltip(|window, cx| Tooltip::new("Jump to the bottom").build(window, cx))
        // The press is the button's, not a selection's nor a program's under it.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                ws.focus_block(id, window, cx);
                cx.stop_propagation();
            }),
        )
        .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.scroll_to_bottom(id, cx)))
        .test_support()
        .into_any_element()
}

/// An invisible strip along a block's edge (`edge` thick), or a square at its corner, that resizes
/// it by those `edges`, like a window's border.
fn resize_zone(id: BlockId, edges: Edges, edge: f32, cx: &mut Context<Workspace>) -> AnyElement {
    let corner = chrome::corner(edge);
    let name = match (edges.top, edges.bottom, edges.left, edges.right) {
        (true, _, true, _) => "top-left",
        (true, _, _, true) => "top-right",
        (_, true, true, _) => "bottom-left",
        (_, true, _, true) => "bottom-right",
        (true, ..) => "top",
        (_, true, ..) => "bottom",
        (_, _, true, _) => "left",
        _ => "right",
    };
    let el = div()
        .id(ElementId::Name(format!("resize-{name}-{}", id.0).into()))
        .absolute();
    // Across: the edge's own side; along: clear of the corners' squares.
    let el = match (edges.left || edges.right, edges.top || edges.bottom) {
        (true, true) => el.size(px(corner)),
        (true, false) => el.w(px(edge)).top(px(corner)).bottom(px(corner)),
        _ => el.h(px(edge)).left(px(corner)).right(px(corner)),
    };
    let el = if edges.left {
        el.left_0()
    } else if edges.right {
        el.right_0()
    } else {
        el
    };
    let el = if edges.top {
        el.top_0()
    } else if edges.bottom {
        el.bottom_0()
    } else {
        el
    };
    let el = match name {
        "top-left" | "bottom-right" => el.cursor_nwse_resize(),
        "top-right" | "bottom-left" => el.cursor_nesw_resize(),
        "top" | "bottom" => el.cursor_ns_resize(),
        _ => el.cursor_ew_resize(),
    };
    el.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
            ws.focus_block(id, window, cx);
            ws.begin_resize(id, edges, e.position);
            cx.stop_propagation();
        }),
    )
    .test_support()
    .into_any_element()
}

/// The zoom nearest `z` at which gpui-kit's editor rows, 1.5 × their text size of `font × zoom`,
/// are whole pixels. GPUI rounds them to whole pixels (gpui-pre-0.3.9 src/style.rs:555,
/// gpui-component-0.7.3 src/input/editor.rs:145): at any other zoom the rows grow a pixel at a
/// time while the letters grow smoothly, and the text below the first rows jumps.
fn exact_rows(z: f32, font: f32) -> f32 {
    let row = 1.5 * font;
    (row * z).round().max(1.0) / row
}

/// A text-heavy body's box: its size at the zoom `settled` (see `render`), so its text and its
/// wrapping stay a photo of the block while a zoom glides, moved by `pull` so the text under the
/// glide's pivot stays there; the frame around it clips. At rest, exactly the frame's inner
/// area: inside the card's border (the frame's width is) and the padding.
fn settled_box(block: Block, settled: f32, pull: Point, body: AnyElement) -> impl IntoElement {
    let inner = chrome::content_rect(block.rect);
    div()
        .relative()
        .left(px(pull.x))
        .top(px(pull.y))
        .w(px((inner.w * settled - 2.0 * BORDER).max(0.0)))
        .h(px((inner.h * settled).max(0.0)))
        .child(body)
}

/// Hover-only controls fade in; they disappear at once. The wrapper covers its parent, since
/// absolute children are placed relative to their direct parent.
fn fade_in(el: impl IntoElement + 'static, id: impl Into<ElementId>) -> impl IntoElement {
    div().absolute().inset_0().child(el).with_animation(
        id,
        Animation::new(motion::HOVER),
        |el, t| el.opacity(t),
    )
}

/// The drop shadow's offset and blur (world units): the focused block floats higher, and a
/// hovered one (`lift` 1) rises a little more.
fn lift_metrics(focused: bool, lift: f32) -> (f32, f32) {
    let (y, blur) = if focused { (10.0, 30.0) } else { (4.0, 12.0) };
    (y + 3.0 * lift, blur + 8.0 * lift)
}

fn shadow(color: Hsla, focused: bool, lift: f32, z: f32) -> BoxShadow {
    let (y, blur) = lift_metrics(focused, lift);
    BoxShadow {
        color,
        offset: point(px(0.0), px(y * z)),
        blur_radius: px(blur * z),
        spread_radius: px(0.0),
        inset: false,
    }
}

/// The dot before a terminal's title, and what it says. News first: yellow when it asked for
/// attention, red or green when a long command ended. Then a question its agent asks the person
/// (yellow; the notice on the block says so up close). Then an agent's turn (its hooks) before the shell's:
/// yellow while it waits for the person, the accent while it works. Then a command running, then
/// a last command that failed. Nothing when the shell doesn't say (no shell integration). Its
/// text names the block that pressed keys in it, for ANSWERED_FOR.
pub(crate) fn status_dot(ws: &Workspace, pane: &Pane) -> Option<(Hsla, SharedString)> {
    let ansi = |i: usize| hsla(ws.theme.palette.ansi[i], 1.0);
    // A remote agent's dot is home's word for it, whatever this computer's hooks hear.
    if pane.remote.is_some() {
        // Not live: home's word is stale, so its dot goes grey.
        let grey = ws.space.remote.as_deref().is_some_and(|r| r.dim());
        let dot = match ws.doing(pane) {
            Doing::NeedsYou(asks) if asks.is_empty() => Some((ansi(3), "Waiting for you".into())),
            Doing::NeedsYou(asks) => Some((ansi(3), format!("Waiting for you: {asks}").into())),
            Doing::Working(_) | Doing::AwaitsSubagents(_) => {
                Some((ws.ui.accent, pane.remote_working().into()))
            }
            Doing::Starting => Some((ws.ui.accent, "Starting".into())),
            Doing::Done => Some((ws.ui.title_dim, "Done".into())),
            Doing::Idle => None,
        };
        return dot.map(|(color, text)| (if grey { ws.ui.title_dim } else { color }, text));
    }
    let (color, text) = match (&pane.alert, ws.asking(pane.term.id())) {
        // A request waits: its own alert ("Asks you: …") or a bell's says less than this.
        (None | Some(Alert::Attention(_)), Some((wait, _))) => {
            (ansi(3), format!("Waiting for you: {}", wait.summary()))
        }
        (Some(a @ Alert::Attention(_)), None) => (ansi(3), a.text()),
        (Some(a), _) if a.failed() => (ansi(1), a.text()),
        (Some(a), _) => (ansi(2), a.text()),
        // A subagent's block (docs/specs/claude-subagents.md): its dialog in its parent's
        // screen, its work, its end.
        (None, None) if pane.follows.is_some() => {
            let follows = pane.follows.as_ref().expect("a subagent's block");
            let asks = ws
                .pane(&follows.parent)
                .filter(|p| p.asks(Some(&follows.id)))
                .and_then(|p| match &p.state {
                    Some(AgentState::NeedsYou(asks)) => Some(asks.clone()),
                    _ => None,
                });
            match (asks, follows.done) {
                (Some(asks), _) => (
                    ansi(3),
                    format!(
                        "Waiting for you: {} Answer in its Claude Code.",
                        asks.as_deref().unwrap_or("it asks you something.")
                    ),
                ),
                (None, Some(_)) => (ws.ui.title_dim, "Done".to_string()),
                (None, None) => (ws.ui.accent, "Working".to_string()),
            }
        }
        (None, None) => {
            // An agent the last snapshot no longer sees (it crashed) says nothing of its turn.
            match pane.state.as_ref().filter(|_| pane.seen.agent.is_some()) {
                Some(AgentState::NeedsYou(asks)) => (
                    ansi(3),
                    asks.as_ref().map_or_else(
                        || "Waiting for you".to_string(),
                        |a| format!("Waiting for you: {a}"),
                    ),
                ),
                Some(AgentState::Working) => (ws.ui.accent, pane.working()),
                Some(AgentState::Idle) => return None,
                None => {
                    let status = pane.term.status();
                    match (status.running, status.last) {
                        (Some(since), _) => (
                            ws.ui.accent,
                            format!("Running for {}", alert::took(since.elapsed())),
                        ),
                        (None, Some(last)) if last.code.is_some_and(|c| c != 0) => {
                            (ansi(1), alert::finished(&last))
                        }
                        _ => return None,
                    }
                }
            }
        }
    };
    Some(match pane.answered_by() {
        Some(by) => (color, format!("{text} · answered by {by}").into()),
        None => (color, text.into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hovered_block_lifts() {
        assert_eq!(lift_metrics(false, 0.0), (4.0, 12.0));
        assert_eq!(lift_metrics(false, 1.0), (7.0, 20.0));
        assert_eq!(lift_metrics(true, 0.0), (10.0, 30.0));
        assert_eq!(lift_metrics(true, 1.0), (13.0, 38.0));
    }
}
