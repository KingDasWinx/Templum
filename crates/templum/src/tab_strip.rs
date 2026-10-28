//! The tab strip of note blocks: a tab per open note, the one on screen lit; a click shows one, a
//! double click renames it in place, its × closes it (it shows on the tab on screen and under the
//! pointer), + opens a new one in a new tab.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::input::{Escape, Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, ElementId, Entity, FontWeight, MouseButton, SharedString,
    TestSupportExt, Window, div, px, relative,
};
use templum_canvas::BlockId;

use crate::grid::hsla;
use crate::workspace::Workspace;

/// The strip's height, world units.
pub(crate) const TABS_H: f32 = 28.0;
/// The strip's left inset, and a tab's: the first label is `LABELS_IN` from the strip's edge, and
/// the note's text starts there too (`note::TEXT_LEFT`). A tab's height, and the + button's side.
const STRIP_IN: f32 = 4.0;
const TAB_IN: f32 = 10.0;
pub(crate) const LABELS_IN: f32 = STRIP_IN + TAB_IN;
const TAB_H: f32 = 22.0;

/// What a tab's click, its double click and its × do.
pub(crate) type TabAction =
    fn(&mut Workspace, BlockId, usize, &mut Window, &mut Context<Workspace>);

/// The tabs `titles` of `block`, `active` on screen, at zoom `z`; `renaming` is the tab whose name
/// is being typed and its field, in the title's place. `actions` are what a click, a ×, and a
/// double click do, in that order; `new` is the strip's +. `locked` is why the + and the ×s, and a
/// double click, do nothing (a mirrored note's tabs are home's): they are dimmed, the words their
/// tooltip. Their element ids are
/// `note-tab-{n}-{i}`, `note-tab-close-{n}-{i}` and `(note-tab-new, n)`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tab_strip(
    ws: &Workspace,
    block: BlockId,
    titles: Vec<SharedString>,
    active: usize,
    renaming: Option<(usize, &Entity<InputState>)>,
    z: f32,
    [show, close, rename]: [TabAction; 3],
    new: fn(&mut Workspace, BlockId, &mut Window, &mut Context<Workspace>),
    locked: Option<SharedString>,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let fg = hsla(ws.theme.palette.foreground, 1.0);
    let n = block.0 as usize;
    let name = |what: &str, i: usize| ElementId::Name(format!("note-{what}-{n}-{i}").into());
    let tabs: Vec<AnyElement> = titles
        .into_iter()
        .enumerate()
        .map(|(i, title)| {
            let on = i == active;
            let group = SharedString::from(format!("note-tab-group-{n}-{i}"));
            let words = locked.clone();
            let close = div()
                .id(name("tab-close", i))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .size(px(16.0 * z))
                .rounded(px(4.0 * z))
                // The others' × shows while the pointer is on the tab.
                .when(!on, |el| {
                    el.opacity(0.0)
                        .group_hover(group.clone(), |s| s.opacity(1.0))
                })
                .cursor_pointer()
                .hover(move |s| s.bg(ui.pressed))
                .child(
                    Icon::new(IconName::Close)
                        .size(px(10.0 * z))
                        .text_color(ui.title),
                )
                .when(locked.is_some(), |el| el.opacity(0.4))
                .tooltip(move |window, cx| {
                    let tip = words.clone().unwrap_or_else(|| "Close tab".into());
                    Tooltip::new(tip).build(window, cx)
                })
                .on_click({
                    let locked = locked.is_some();
                    cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        if !locked {
                            close(ws, block, i, window, cx)
                        }
                    })
                })
                .test_support();
            div()
                .id(name("tab", i))
                .group(group)
                .flex_shrink_0()
                .max_w(px(170.0 * z))
                .h(px(TAB_H * z))
                .flex()
                .items_center()
                .gap(px(2.0 * z))
                .pl(px(TAB_IN * z))
                .pr(px(3.0 * z))
                .rounded(px(6.0 * z))
                .text_size(px(12.0 * z))
                .cursor_pointer()
                .map(|el| {
                    if on {
                        el.bg(ui.hover)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                    } else {
                        el.text_color(ui.title_dim)
                            .hover(move |s| s.bg(ui.card).text_color(fg))
                    }
                })
                .child(match renaming.filter(|(at, _)| *at == i) {
                    Some((_, input)) => rename_field(input, block, ui.title, z, cx),
                    None => div().truncate().child(title).into_any_element(),
                })
                .child(close)
                .on_click({
                    let locked = locked.is_some();
                    cx.listener(move |ws, e: &ClickEvent, window, cx| {
                        if e.click_count() >= 2 {
                            if !locked {
                                rename(ws, block, i, window, cx)
                            }
                        } else {
                            show(ws, block, i, window, cx)
                        }
                    })
                })
                .test_support()
                .into_any_element()
        })
        .collect();
    let plus = div()
        .id(("note-tab-new", n))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .size(px(TAB_H * z))
        .rounded(px(6.0 * z))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(
            Icon::new(IconName::Plus)
                .size(px(13.0 * z))
                .text_color(ui.title),
        )
        .when(locked.is_some(), |el| el.opacity(0.4))
        .tooltip({
            let words = locked.clone();
            move |window, cx| {
                let tip = words.clone().unwrap_or_else(|| "New note".into());
                Tooltip::new(tip).build(window, cx)
            }
        })
        .on_click({
            let locked = locked.is_some();
            cx.listener(move |ws, _: &ClickEvent, window, cx| {
                if !locked {
                    new(ws, block, window, cx)
                }
            })
        })
        .test_support();
    div()
        .id(("note-tabs", n))
        .flex_shrink_0()
        .h(px(TABS_H * z))
        .flex()
        .items_center()
        .gap(px(2.0 * z))
        .px(px(STRIP_IN * z))
        .border_b_1()
        .border_color(ui.hairline)
        .overflow_x_scroll()
        .children(tabs)
        .child(plus)
        .test_support()
        .into_any_element()
}

/// The field that renames a tab, in its title's place: one line of the tab's text at zoom `z`.
/// Esc drops the new name; a press in it is its own, not the tab's click.
fn rename_field(
    input: &Entity<InputState>,
    block: BlockId,
    color: gpui_kit::Hsla,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    div()
        .w(px(120.0 * z))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_action(
            cx.listener(move |ws, _: &Escape, window, cx| ws.cancel_note_rename(block, window, cx)),
        )
        .child(
            Styled::h(Input::new(input), px(TAB_H * z))
                .appearance(false)
                .px(px(0.0))
                .py(px(0.0))
                .text_size(px(12.0 * z))
                .line_height(relative(1.5))
                .text_color(color),
        )
        .into_any_element()
}
