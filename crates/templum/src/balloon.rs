//! What the requests inbox (`inbox.rs`) draws of a request an agent makes of the person
//! (docs/specs/ask-inbox.md): its questions (docs/specs/ask-user.md, The dialog), a plan to
//! approve, or a request with a yes and a no, each as a body that scrolls and a foot that stays,
//! in the language of the kanban card's dialog and the Git popup. Drawn at zoom 1: the inbox is
//! window chrome.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Div, ElementId, FontWeight, Hsla, ScrollWheelEvent,
    SharedString, Stateful, TestSupportExt, Window, canvas, div, point, px, relative,
};
use templum_mcp::ask::{Ask, Choice, Form, Row, clean_preview};

use crate::git_block::{self, FONT};
use crate::grid::hsla;
use crate::theme::Ui;
use crate::workspace::{Wait, Waiting, Workspace};

/// What a question whose client stopped waiting says under the header.
const STOPPED: &str = "The agent stopped waiting: your answer will be typed into its terminal.";
/// Its text's size, world units: the questions.
const TEXT: f32 = 14.5;
/// An option's label.
const LABEL: f32 = 13.5;
/// The size of what is said about them, world units: descriptions, hints, the preview.
const SMALL: f32 = 12.0;
/// The body's sides, and the foot's.
pub(crate) const SIDE: f32 = 18.0;
/// A preview's line height, world units, and the most lines it shows before Show more.
const PREVIEW_LINE: f32 = 16.0;
const PREVIEW_CLAMP: usize = 12;
/// A dialog narrower than this, world units, lays a preview under the options instead of beside
/// them.
const BESIDE: f32 = 520.0;
/// The most of the body's height a plan's frame takes; it scrolls past that.
const PLAN_SHARE: f32 = 0.6;
/// The most characters of a plan's title shown.
const PLAN_TITLE: usize = 60;
/// The inbox is window chrome: never scaled by the canvas's zoom.
const Z: f32 = 1.0;

/// What a kind of request puts in the inbox's right column: the body (which scrolls) and the
/// foot (which stays).
pub(crate) struct Parts {
    pub body: Div,
    pub foot: Div,
}

/// The request `wait` of the block `n`, drawn in a column `w` wide whose body is `h` high at most
/// (world units); None when it is no request of the person's.
pub(crate) fn request(
    ws: &Workspace,
    wait: &Wait,
    n: usize,
    w: f32,
    h: f32,
    cx: &mut Context<Workspace>,
) -> Option<Parts> {
    Some(match &wait.what {
        Waiting::Question {
            ask, form, scroll, ..
        } if ask.plan.is_some() => plan(ws, wait, ask, form, scroll, n, w, h, Z, cx),
        Waiting::Question { ask, form, .. } => question(ws, wait, ask, form, n, w, Z, cx),
        Waiting::Allow { then } => allow(ws, wait, then.buttons(), n, Z, cx),
        Waiting::Turn { .. } | Waiting::Event { .. } => return None,
    })
}

/// Said under the header of a question whose client gave up on the call, where it stays in
/// sight.
pub(crate) fn stopped_banner(ws: &Workspace, n: usize) -> AnyElement {
    // The yellow of the dot of a block waiting for its person.
    let waiting = hsla(ws.theme.palette.ansi[3], 1.0);
    div()
        .id(part(n, "stopped"))
        .flex()
        .items_start()
        .flex_shrink_0()
        .gap(px(8.0))
        .mx(px(SIDE))
        .mt(px(12.0))
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .bg(waiting.opacity(0.12))
        .text_size(px(SMALL))
        .child(
            div().mt(px(1.0)).child(
                Icon::new(IconName::CircleAlert)
                    .size(px(14.0))
                    .text_color(waiting),
            ),
        )
        .child(div().flex_1().min_w(px(0.0)).child(STOPPED))
        .test_support()
        .into_any_element()
}

/// The id of the request's element `what` on the asking block `n`: `q<q>-option-<i>`,
/// `q<q>-other`, `note`, `send`, `dismiss`, `progress`, `allow`, `deny`, `stopped`, `body`,
/// `tab-<q>` and, on an answered question's tab, `tab-<q>-answered`.
pub(crate) fn part(n: usize, what: &str) -> ElementId {
    ElementId::Name(format!("balloon-{n}-{what}").into())
}

/// A question's dialog, `w` world units wide: with several questions, a tab each, then the shown
/// one with its header, text, options and Other field; the note; Dismiss and Send in the foot.
/// Today's shape (`ask.json` false) draws today's: a button per option, or the field. A click on
/// an option only picks it; Send, lit once every question has an answer, answers. Only the rows
/// `Form::rows` lists are drawn.
#[allow(clippy::too_many_arguments)]
fn question(
    ws: &Workspace,
    wait: &Wait,
    ask: &Ask,
    form: &Form,
    n: usize,
    w: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> Parts {
    let (key, ui) = (wait.key, ws.ui);
    let Waiting::Question { others, note, .. } = &wait.what else {
        return Parts {
            body: div(),
            foot: div(),
        };
    };
    let texts = ws.texts(wait, cx).map(|(texts, _)| texts);
    let ready = texts
        .as_ref()
        .is_some_and(|texts| form.answers(ask, texts).is_some());
    let answered = texts.as_ref().map_or(0, |texts| form.answered(ask, texts));
    let rows = form.rows(ask);
    let mut body = div().flex().flex_col().gap(px(18.0 * z));
    if ask.tabs() {
        let texts = texts.as_deref().unwrap_or_default();
        body = body.child(tabs(ws, key, ask, form, texts, n, z, cx));
    }
    let q = form.tab;
    let question = &ask.questions[q];
    let title = div()
        .flex()
        .flex_col()
        .gap(px(8.0 * z))
        .when_some(
            question.header_shown().filter(|_| !ask.tabs()),
            |el, header| el.child(div().flex().child(chip(header, ui, z))),
        )
        .child(
            div()
                .min_w(px(0.0))
                .text_size(px(TEXT * z))
                .font_weight(FontWeight::MEDIUM)
                .line_height(relative(1.4))
                .child(question.text.clone()),
        )
        .when(question.multi, |el| {
            el.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.0 * z))
                    .text_size(px(SMALL * z))
                    .text_color(ui.title_dim)
                    .child(
                        Icon::new(IconName::ListChecks)
                            .size(px(13.0 * z))
                            .text_color(ui.title_dim),
                    )
                    .child("Pick any"),
            )
        });
    let mut block = div().flex().flex_col().gap(px(10.0 * z)).child(title);
    // Its options and Other, in a faint panel; with a preview, a column beside it (or under
    // it, in a narrow dialog).
    let mut choices = div().flex().flex_col().gap(px(2.0 * z));
    for (i, choice) in question.options.iter().enumerate() {
        let id = part(n, &format!("q{q}-option-{i}"));
        let (picked, lit) = (form.picked(q, i), form.highlight == Some(Row::Option(q, i)));
        // Today's shape: a button, no radio.
        let mark = ask.json.then_some(question.multi);
        choices = choices.child(
            option(id, mark, picked, lit, choice, ui, z)
                .children(reveal(Row::Option(q, i), wait))
                .on_click(cx.listener(once(move |ws, window, cx| {
                    ws.pick_option(key, q, i, window, cx)
                })))
                .on_hover(hover(key, Row::Option(q, i), cx))
                .test_support(),
        );
    }
    if rows.contains(&Row::Other(q)) {
        let input = &others[q].0;
        let lit = form.highlight == Some(Row::Other(q));
        let mut other = div()
            .id(part(n, &format!("q{q}-other")))
            .flex()
            .items_center()
            .gap(px(8.0 * z))
            .w_full()
            .rounded(px(8.0 * z))
            .when(!question.options.is_empty(), |el| {
                let (fill, _) = tint(form.other_picked(q), lit, ui);
                el.px(px(10.0 * z))
                    .py(px(6.0 * z))
                    .when_some(fill, |el, fill| el.bg(fill))
            })
            .when(question.options.is_empty() && lit, |el| el.bg(ui.hover));
        if !question.options.is_empty() {
            // A press on its mark or its name puts the keyboard in its field.
            let focus = input.clone();
            other = other.child(
                div()
                    .id(part(n, &format!("q{q}-other-mark")))
                    .flex()
                    .items_center()
                    .gap(px(10.0 * z))
                    .cursor_pointer()
                    .child(mark(question.multi, form.other_picked(q), ui, z))
                    .child(div().text_size(px(LABEL * z)).child("Other"))
                    .on_click(move |_, window, cx| {
                        focus.update(cx, |input, cx| input.focus(window, cx))
                    }),
            );
        }
        other = other
            .child(git_block::field(input, z).flex_1().min_w(px(0.0)))
            .children(reveal(Row::Other(q), wait));
        choices = choices.child(other.on_hover(hover(key, Row::Other(q), cx)).test_support());
    }
    let panel = |choices: Div| {
        if question.options.is_empty() {
            choices
        } else {
            choices.p(px(4.0 * z)).rounded(px(10.0 * z)).bg(ui.card)
        }
    };
    block = block.child(match ask.preview_of(q, form) {
        None => panel(choices),
        Some(shown) => {
            // The body's sides are not the preview's; beside the options it has what their 45% and
            // the gap leave.
            let preview_w = if w >= BESIDE {
                (w - 2.0 * SIDE) * 0.55 - 12.0
            } else {
                w - 2.0 * SIDE
            };
            let preview = preview(ws, wait, shown, (n, q), preview_w, z, cx);
            if w >= BESIDE {
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.0 * z))
                    .child(panel(choices).w(relative(0.45)).flex_shrink_0())
                    .child(preview)
            } else {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0 * z))
                    .child(panel(choices))
                    .child(preview)
            }
        }
    });
    body = body.child(block);
    if let Some((input, _)) = note {
        body = body.child(div().h(px(1.0 * z)).bg(ui.hairline)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0 * z))
                .child(section(ui, IconName::StickyNote, "Note", "optional", z))
                .child(
                    div()
                        .id(part(n, "note"))
                        .w_full()
                        .rounded(px(8.0 * z))
                        .when(form.highlight == Some(Row::Note), |el| el.bg(ui.hover))
                        .child(git_block::field(input, z).w_full())
                        .children(reveal(Row::Note, wait))
                        .on_hover(hover(key, Row::Note, cx))
                        .test_support(),
                ),
        );
    }
    let many = ask.json && ask.questions.len() > 1;
    let total = ask.questions.len();
    let foot = foot_row(z)
        .child(
            ghost(part(n, "dismiss"), "Dismiss", ui, z)
                .on_click(cx.listener(once(move |ws, window, cx| {
                    ws.dismiss_question(key, window, cx)
                })))
                .test_support(),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .justify_end()
                .when(many, |el| {
                    el.child(
                        div()
                            .id(part(n, "progress"))
                            .truncate()
                            .text_size(px(SMALL * z))
                            .text_color(ui.title_dim)
                            .child(format!("{answered} of {total} answered"))
                            .test_support(),
                    )
                }),
        )
        .child(
            send_button(
                part(n, "send"),
                ready,
                form.highlight == Some(Row::Send),
                ui,
                z,
            )
            .ml_auto()
            .on_hover(hover(key, Row::Send, cx))
            .on_click(cx.listener(once(move |ws, window, cx| {
                ws.send_question(key, window, cx)
            })))
            .test_support(),
        );
    Parts { body, foot }
}

/// The tabs of the question `key`, which asks several (docs/specs/ask-user.md, Tabs), in the Git
/// popup's look: each its header, or "Question N", after a ring that fills with a check once its
/// question has an answer (`others`: the Other texts); the shown one is underlined in the accent.
/// A click shows its question. They wrap on more lines in a dialog too narrow for one row.
#[allow(clippy::too_many_arguments)]
fn tabs(
    ws: &Workspace,
    key: u64,
    ask: &Ask,
    form: &Form,
    others: &[String],
    n: usize,
    z: f32,
    cx: &mut Context<Workspace>,
) -> Div {
    let ui = ws.ui;
    let fg = hsla(ws.theme.palette.foreground, 1.0);
    let tabs = (0..ask.questions.len()).map(|q| {
        let on = q == form.tab;
        let ring = div()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .size(px(14.0 * z))
            .rounded_full();
        let ring = if form.has_answer(ask, q, others) {
            ring.id(part(n, &format!("tab-{q}-answered")))
                .bg(ui.accent)
                .child(
                    Icon::new(IconName::Check)
                        .size(px(10.0 * z))
                        .text_color(ui.surface),
                )
                .test_support()
                .into_any_element()
        } else {
            ring.border_1()
                .border_color(ui.title_dim)
                .into_any_element()
        };
        div()
            .id(part(n, &format!("tab-{q}")))
            .flex()
            .items_center()
            .gap(px(6.0 * z))
            .min_w(px(0.0))
            .min_h(px(30.0 * z))
            .border_b_2()
            .border_color(if on {
                ui.accent
            } else {
                gpui_kit::transparent_black()
            })
            .text_size(px(FONT * z))
            .font_weight(FontWeight::MEDIUM)
            .text_color(if on { fg } else { ui.title_dim })
            .cursor_pointer()
            .hover(move |s| s.text_color(fg))
            .child(ring)
            .child(ask.tab(q))
            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                ws.show_question(key, q, window, cx)
            }))
            .test_support()
    });
    div()
        .flex()
        .flex_wrap()
        .gap_x(px(16.0 * z))
        .border_b_1()
        .border_color(ui.hairline)
        .children(tabs)
}

/// A plan's dialog (docs/specs/agent-tools.md, propose_plan): the chip "Plan" and its title, its
/// Markdown in a frame on the canvas color that scrolls on its own, at most PLAN_SHARE of the
/// body's height `h` (world units), the comment, then Reject on the left and Approve on the right.
#[allow(clippy::too_many_arguments)]
fn plan(
    ws: &Workspace,
    wait: &Wait,
    ask: &Ask,
    form: &Form,
    scroll: &crate::markdown::PreviewScroll,
    n: usize,
    w: f32,
    h: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> Parts {
    let (key, ui) = (wait.key, ws.ui);
    let (Some(shown), Waiting::Question { note, .. }) = (&ask.plan, &wait.what) else {
        return Parts {
            body: div(),
            foot: div(),
        };
    };
    let title: String = ask.summary().chars().take(PLAN_TITLE).collect();
    // The plan's width, world units: the column's less the body's padding.
    let w = w - 2.0 * SIDE - 2.0;
    let head = div()
        .id(part(n, "plan-title"))
        .flex()
        .items_center()
        .gap(px(8.0 * z))
        .child(chip("Plan", ui, z))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .test_support();
    let overflows = scroll.handle.clone();
    let frame = div()
        .flex()
        .flex_col()
        .max_h(px(h * PLAN_SHARE * z))
        .overflow_hidden()
        // The wheel scrolls a plan that overflows, not the dialog's body around it.
        .on_scroll_wheel(move |e: &ScrollWheelEvent, _, cx| {
            if !e.modifiers.control && overflows.max_offset().y > px(0.0) {
                cx.stop_propagation();
            }
        })
        .rounded(px(8.0 * z))
        .border_1()
        .border_color(ui.hairline)
        .bg(ui.canvas)
        .child(crate::markdown::preview(
            part(n, "plan-body"),
            ("plan-markdown", key).into(),
            shown.text.clone().into(),
            scroll,
            w,
            LABEL,
            z,
        ));
    let body = div()
        .flex()
        .flex_col()
        .gap(px(12.0 * z))
        .child(head)
        .child(frame)
        .when_some(note.as_ref(), |el, (input, _)| {
            el.child(
                div()
                    .id(part(n, "note"))
                    .w_full()
                    .rounded(px(8.0 * z))
                    .when(form.highlight == Some(Row::Note), |el| el.bg(ui.hover))
                    .child(git_block::field(input, z).w_full())
                    .children(reveal(Row::Note, wait))
                    .on_hover(hover(key, Row::Note, cx))
                    .test_support(),
            )
        });
    let row = |i: usize| Row::Option(0, i);
    let reject = ghost(part(n, "plan-reject"), "Reject", ui, z)
        .when(form.highlight == Some(row(1)), |el| el.bg(ui.hover))
        .on_hover(hover(key, row(1), cx))
        .on_click(cx.listener(once(move |ws, window, cx| {
            ws.pick_option(key, 0, 1, window, cx)
        })))
        .test_support();
    let approve = solid(
        part(n, "plan-approve"),
        "Approve",
        Some(IconName::Check),
        ui,
        z,
    )
    .when(form.highlight == Some(row(0)), |el| {
        el.bg(ui.accent.opacity(0.85))
    })
    .on_hover(hover(key, row(0), cx))
    .on_click(cx.listener(once(move |ws, window, cx| {
        ws.pick_option(key, 0, 0, window, cx)
    })))
    .test_support();
    Parts {
        body,
        foot: foot_row(z).child(reject).child(approve.ml_auto()),
    }
}

/// Scrolls the body to show `row` once, when the keys just moved the highlight to it and it is
/// cut at an edge (or out of view): an invisible layer over the row, which sees where it was
/// laid out. A wheel that scrolls it away later isn't fought.
fn reveal(row: Row, wait: &Wait) -> Option<AnyElement> {
    if wait.reveal.get() != Some(row) {
        return None;
    }
    let (wanted, body) = (wait.reveal.clone(), wait.body.handle.clone());
    Some(
        canvas(
            move |at, window, _| {
                if wanted.take().is_none() {
                    return;
                }
                let (view, offset, max) = (body.bounds(), body.offset(), body.max_offset());
                let by = if at.top() < view.top() {
                    view.top() - at.top()
                } else if at.bottom() > view.bottom() {
                    view.bottom() - at.bottom()
                } else {
                    return;
                };
                body.set_offset(point(offset.x, (offset.y + by).clamp(-max.y, px(0.0))));
                window.request_animation_frame();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
        .into_any_element(),
    )
}

/// Hovering a row of the question `key` highlights it.
fn hover(
    key: u64,
    row: Row,
    cx: &mut Context<Workspace>,
) -> impl Fn(&bool, &mut Window, &mut gpui_kit::App) + 'static {
    cx.listener(move |ws, hovered: &bool, _, cx| {
        if *hovered {
            ws.highlight_row(key, row, cx);
        }
    })
}

/// Question `q`'s preview of the block `n`, `w` world units wide: a caption over `shown` in the
/// terminal's font, wrapped on the frame's width and cut after PREVIEW_CLAMP lines with a Show
/// more under it that shows the rest (and Show less); None says "No preview".
fn preview(
    ws: &Workspace,
    wait: &Wait,
    shown: Option<&str>,
    (n, q): (usize, usize),
    w: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> Div {
    let ui = ws.ui;
    let pad = 8.0 * z;
    let (key, more) = (wait.key, wait.more.get());
    // The lines it takes wrapped, by the width of a letter of its font.
    let letters = ((w - 2.0 * (8.0 + 1.0)) / (ws.cell.size.w * SMALL / ws.cell.font_size))
        .floor()
        .max(1.0) as usize;
    let long = shown.is_some_and(|text| {
        text.lines()
            .map(|line| line.chars().count().div_ceil(letters).max(1))
            .sum::<usize>()
            > PREVIEW_CLAMP
    });
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap(px(6.0 * z))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0 * z))
                .text_size(px(11.0 * z))
                .text_color(ui.title_dim)
                .child(
                    Icon::new(IconName::Eye)
                        .size(px(12.0 * z))
                        .text_color(ui.title_dim),
                )
                .child("Preview"),
        )
        .child(
            div()
                .id(part(n, &format!("q{q}-preview")))
                .w_full()
                // Cut only where Show more is offered: an estimate that is short shows it whole.
                .when(long && !more, |el| {
                    el.max_h(px(PREVIEW_CLAMP as f32 * PREVIEW_LINE * z + 2.0 * pad))
                })
                .overflow_hidden()
                .p(px(pad))
                .rounded(px(8.0 * z))
                .border_1()
                .border_color(ui.hairline)
                .bg(ui.canvas)
                .font_family(ws.cell.font.family.clone())
                .text_size(px(SMALL * z))
                .line_height(px(PREVIEW_LINE * z))
                .whitespace_normal()
                .child(match shown {
                    Some(text) => div().child(clean_preview(text)),
                    None => div().text_color(ui.title_dim).child("No preview"),
                })
                .test_support(),
        )
        .when(long || more, |el| {
            el.child(
                div()
                    .id(part(n, &format!("q{q}-preview-more")))
                    .text_size(px(11.0 * z))
                    .text_color(ui.accent)
                    .cursor_pointer()
                    .child(if more { "Show less" } else { "Show more" })
                    .on_click(
                        cx.listener(move |ws, _: &ClickEvent, _, cx| ws.toggle_previews(key, cx)),
                    )
                    .test_support(),
            )
        })
}

/// One of a question's options: its radio (`mark` false) or checkbox (`mark` true) and its label,
/// and its description under it; with no mark, a button.
fn option(
    id: ElementId,
    mark: Option<bool>,
    picked: bool,
    lit: bool,
    choice: &Choice,
    ui: Ui,
    z: f32,
) -> Stateful<Div> {
    let (fill, over) = tint(picked, lit, ui);
    let multi = mark;
    div()
        .id(id)
        .flex()
        .items_start()
        .gap(px(10.0 * z))
        .w_full()
        .px(px(10.0 * z))
        .py(px(8.0 * z))
        .rounded(px(8.0 * z))
        .cursor_pointer()
        .hover(move |s| s.bg(over))
        .when_some(fill, |el, fill| el.bg(fill))
        // On the label's first line.
        .when_some(multi, |el, multi| {
            el.child(
                div()
                    .mt(px(1.5 * z))
                    .child(self::mark(multi, picked, ui, z)),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0 * z))
                .child(
                    div()
                        .text_size(px(LABEL * z))
                        .line_height(relative(1.4))
                        .when(picked && mark.is_none(), |el| el.text_color(ui.accent))
                        .child(choice.label.clone()),
                )
                .when_some(choice.description.clone(), |el, description| {
                    el.child(
                        div()
                            .text_size(px(SMALL * z))
                            .line_height(relative(1.45))
                            .text_color(ui.title_dim)
                            .child(description),
                    )
                }),
        )
}

/// A row's fill, and its fill under the pointer: a picked one is tinted with the accent (more
/// under the pointer or the keys' highlight), one only highlighted takes the hover color.
fn tint(picked: bool, lit: bool, ui: Ui) -> (Option<Hsla>, Hsla) {
    if picked {
        let over = ui.accent.opacity(0.22);
        (Some(if lit { over } else { ui.accent.opacity(0.14) }), over)
    } else {
        (lit.then_some(ui.hover), ui.hover)
    }
}

/// A header of the dialog's chips: a faint accent badge, as the kanban card's number.
fn chip(label: impl Into<SharedString>, ui: Ui, z: f32) -> Div {
    div()
        .flex_shrink_0()
        .px(px(8.0 * z))
        .py(px(2.0 * z))
        .rounded(px(6.0 * z))
        .bg(ui.accent.opacity(0.16))
        .text_size(px(11.5 * z))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(ui.accent)
        .child(label.into())
}

/// A section's header, as the kanban card's: its icon, its name and, at the right, `aside`.
fn section(ui: Ui, icon: IconName, label: &'static str, aside: &'static str, z: f32) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.0 * z))
        .child(Icon::new(icon).size(px(13.0 * z)).text_color(ui.title_dim))
        .child(
            div()
                .text_size(px(SMALL * z))
                .font_weight(FontWeight::MEDIUM)
                .text_color(ui.title)
                .child(label),
        )
        .child(
            div()
                .ml_auto()
                .text_size(px(11.0 * z))
                .text_color(ui.title_dim)
                .child(aside),
        )
}

/// A foot's quiet button (Dismiss, Reject, Deny): no fill, the hover color under the pointer.
pub(crate) fn ghost(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    ui: Ui,
    z: f32,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.0 * z))
        .px(px(12.0 * z))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.0 * z))
        .text_size(px(FONT * z + 0.5 * z))
        .font_weight(FontWeight::MEDIUM)
        .text_color(ui.title)
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(label.into())
}

/// A foot's solid button (Send, Approve, Allow): the accent, the surface's color on it.
fn solid(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: Option<IconName>,
    ui: Ui,
    z: f32,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.0 * z))
        .px(px(14.0 * z))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0 * z))
        .rounded(px(8.0 * z))
        .bg(ui.accent)
        .text_size(px(FONT * z + 0.5 * z))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(ui.surface)
        .cursor_pointer()
        .hover(move |s| s.bg(ui.accent.opacity(0.85)))
        .active(move |s| s.bg(ui.accent.opacity(0.7)))
        .when_some(icon, |el, icon| {
            el.child(Icon::new(icon).size(px(14.0 * z)).text_color(ui.surface))
        })
        .child(label.into())
}

/// Send: solid accent once every question has an answer, a faint fill and inert before, as the
/// kanban's Comment; `lit` while the keys' highlight is on it.
fn send_button(id: ElementId, ready: bool, lit: bool, ui: Ui, z: f32) -> Stateful<Div> {
    if ready {
        solid(id, "Send", Some(IconName::ArrowUp), ui, z)
            .when(lit, |el| el.bg(ui.accent.opacity(0.85)))
    } else {
        div()
            .id(id)
            .h(px(30.0 * z))
            .px(px(14.0 * z))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.0 * z))
            .rounded(px(8.0 * z))
            .bg(ui.hover)
            .text_size(px(FONT * z + 0.5 * z))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(ui.title_dim)
            .child(
                Icon::new(IconName::ArrowUp)
                    .size(px(14.0 * z))
                    .text_color(ui.title_dim),
            )
            .child("Send")
    }
}

/// A radio (a circle with a dot) or a checkbox (a rounded square with a check), in the accent
/// when picked.
fn mark(multi: bool, picked: bool, ui: Ui, z: f32) -> Div {
    let size = 16.0 * z;
    let base = div()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .size(px(size))
        .rounded(px(if multi { 5.0 * z } else { size / 2.0 }))
        .border_1()
        .border_color(if picked { ui.accent } else { ui.title_dim });
    match (multi, picked) {
        (true, true) => base.bg(ui.accent).child(
            Icon::new(IconName::Check)
                .size(px(11.0 * z))
                .text_color(ui.surface),
        ),
        (false, true) => base.child(div().size(px(8.0 * z)).rounded_full().bg(ui.accent)),
        (_, false) => base,
    }
}

/// A request with two buttons: what it asks, its yes and its no (Allow and Deny for a request to
/// connect).
fn allow(
    ws: &Workspace,
    wait: &Wait,
    (yes, no): (&'static str, &'static str),
    n: usize,
    z: f32,
    cx: &mut Context<Workspace>,
) -> Parts {
    let (key, ui) = (wait.key, ws.ui);
    Parts {
        body: div().min_w(px(0.0)).child(ws.allow_line(wait, cx)),
        foot: foot_row(z)
            .justify_end()
            .child(
                ghost(part(n, "deny"), no, ui, z)
                    .on_click(cx.listener(once(move |ws, _, cx| ws.answer_allow(key, false, cx))))
                    .test_support(),
            )
            .child(
                solid(part(n, "allow"), yes, None, ui, z)
                    .on_click(cx.listener(once(move |ws, _, cx| ws.answer_allow(key, true, cx))))
                    .test_support(),
            ),
    }
}

/// A foot's row of buttons: they wrap on a second line in a dialog too narrow for one, so none is
/// cut at the side; a button with `ml_auto` keeps the right.
fn foot_row(z: f32) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0 * z))
        .w_full()
}

/// A button's click, but not a double click's second: once a request is answered, the next one's
/// buttons are drawn under the pointer, and that click would choose for the person.
fn once(
    answer: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) -> impl Fn(&mut Workspace, &ClickEvent, &mut Window, &mut Context<Workspace>) + 'static {
    move |ws, event, window, cx| {
        if event.click_count() <= 1 {
            answer(ws, window, cx);
        }
    }
}
