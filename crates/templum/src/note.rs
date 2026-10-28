//! A note block: tabs of plain-text notes (`notes.rs` keeps them, app-wide), each edited in
//! gpui-kit's editor; with none open, a way to open one. And the Notes dialog, where every note is
//! found, opened and deleted. See docs/specs/cockpit.md, section 5.

use std::cell::RefCell;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::input::{Editor, EditorState, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, ElementId, Entity, KeyDownEvent, ScrollWheelEvent,
    SharedString, TestSupportExt, Window, div, px, relative,
};
use gpui_kit::{EntityInputHandler as _, Keystroke};
use templum_canvas::{BlockId, Level, Size, chrome};

use crate::block::BORDER;
use crate::grid::{BAR_OPACITY, hsla};
use crate::list_dialog;
use crate::notes;
use crate::silhouette::{self, Far};
use crate::tab_strip::{LABELS_IN, TABS_H, tab_strip};
use crate::workspace::Workspace;
use crate::workspace::remote::Deed;

/// A new note's size, and the smallest one (world units).
pub(crate) const SIZE: Size = Size { w: 360.0, h: 280.0 };
pub(crate) const MIN: Size = Size { w: 200.0, h: 140.0 };
/// The text's size at zoom 1, world units.
pub(crate) const FONT: f32 = 13.0;
/// What gpui-kit's code editor keeps at fixed pixel sizes at any zoom: its padding
/// (gpui-component-0.7.3 src/input/input.rs:591-600 and sizing.rs:147-166, size Medium: 6 left
/// in a code editor, 10 right, 8 top and bottom) and, inside it, left of the text, the line
/// numbers' margin (6) and the fold icons (18), and right of it a margin its wrap width leaves
/// out (10) (gpui-base-0.7.3 src/input/base/element.rs:50-53, 1102-1124, 2562).
const FIXED_LEFT: f32 = 6.0 + 6.0 + 18.0;
const FIXED_RIGHT: f32 = 10.0 + 10.0;
pub(crate) const FIXED_TOP: f32 = 8.0;
const FIXED_BOTTOM: f32 = 8.0;
/// Where the text starts from the left of the area under the tabs, world units: in line with the
/// tabs' labels.
pub(crate) const TEXT_LEFT: f32 = LABELS_IN;
/// An empty block shows its icon when the area under the tabs is `ROOMY` tall (world units) and
/// its words from `TIGHT`; the smallest block's keeps to the two buttons.
const ROOMY: f32 = 170.0;
const TIGHT: f32 = 110.0;

/// A note block: the notes it shows as tabs, and the one on screen.
#[derive(Default)]
pub(crate) struct Note {
    pub tabs: Vec<NoteTab>,
    pub active: usize,
    /// The tab whose name is being typed over, in place.
    pub renaming: Option<Renaming>,
    /// Of a mirrored workspace: its ids are home's, its notes the copy in `RemoteSpace::notes`,
    /// its editors read-only and unsubscribed. No local lookup by note id meets it.
    pub remote: bool,
}

/// A tab's name being edited in its place: kept on Enter or when the keyboard leaves, dropped on
/// Esc.
pub(crate) struct Renaming {
    pub id: u64,
    pub input: Entity<InputState>,
}

/// A note open in a block: its id in `Workspace::notes`, and its editor, whose every change is
/// written there.
pub(crate) struct NoteTab {
    pub id: u64,
    pub editor: Entity<EditorState>,
    /// Its word map, drawn far away (`silhouette.rs`).
    pub far: RefCell<Far>,
}

impl Note {
    pub fn tab(&self) -> Option<&NoteTab> {
        self.tabs.get(self.active)
    }

    /// Takes tab `index` out; the tab on screen stays the same one where it can. A rename of it
    /// ends.
    pub fn remove_tab(&mut self, index: usize) -> NoteTab {
        let tab = self.tabs.remove(index);
        if self.active > index || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        if self.renaming.as_ref().is_some_and(|r| r.id == tab.id) {
            self.renaming = None;
        }
        tab
    }

    /// The notes it shows, in tab order.
    pub fn ids(&self) -> Vec<u64> {
        self.tabs.iter().map(|t| t.id).collect()
    }

    /// The note on screen's text; "" without one.
    pub fn text(&self, cx: &App) -> String {
        self.tab()
            .map(|t| t.editor.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// What sending it to a terminal types: the selection, or the whole note.
    pub fn to_send(&self, cx: &App) -> String {
        let Some(tab) = self.tab() else {
            return String::new();
        };
        let selected = tab.editor.read(cx).selected_value();
        if selected.trim().is_empty() {
            self.text(cx)
        } else {
            selected.to_string()
        }
    }

    /// The note on screen's name; "Notes" without one.
    pub fn title(&self, ws: &Workspace) -> SharedString {
        match (self.tab(), ws.notes_of(self)) {
            (Some(tab), Some(notes)) => notes.name(tab.id).to_string().into(),
            _ => "Notes".into(),
        }
    }

    /// Typing goes to the note on screen; false without one.
    pub fn focus(&self, window: &mut Window, cx: &mut App) -> bool {
        let Some(tab) = self.tab() else {
            return false;
        };
        tab.editor.update(cx, |editor, cx| editor.focus(window, cx));
        true
    }
}

/// Whether `keystroke` types into a text field, or pastes or cuts: a key with a character and no
/// Ctrl, Alt or platform key, Enter, Backspace, Delete, Tab, and the platform's paste and cut.
pub(crate) fn would_type(keystroke: &Keystroke) -> bool {
    let m = &keystroke.modifiers;
    let secondary = if cfg!(target_os = "macos") {
        m.platform
    } else {
        m.control
    };
    if secondary && !m.alt {
        return matches!(keystroke.key.as_str(), "v" | "x");
    }
    if m.control || m.alt || m.platform {
        return false;
    }
    keystroke.key_char.is_some()
        || matches!(
            keystroke.key.as_str(),
            "enter" | "backspace" | "delete" | "tab"
        )
}

/// A note's editor: plain text, wrapped at the block's width.
pub(crate) fn editor(text: &str, window: &mut Window, cx: &mut App) -> Entity<EditorState> {
    let text = text.to_string();
    cx.new(|cx| {
        EditorState::new(window, cx)
            .soft_wrap(true)
            .line_number(false)
            .default_value(text)
    })
}

/// Writes `text` into a note's `editor` as one ranged change: one step of its Ctrl+Z, the
/// selection (moved by the change) and the scroll kept. The same when the editor is read-only
/// (gpui-base lifts that for a program's edits). Nothing happens when the text is the same. A
/// composition pending (a dead key, an input method) is committed first as plain text: a ranged
/// replace would otherwise land over its marked range.
pub(crate) fn write(editor: &Entity<EditorState>, text: &str, window: &mut Window, cx: &mut App) {
    editor.update(cx, |editor, cx| {
        if editor.marked_text_range(window, cx).is_some() {
            editor.unmark_text(window, cx);
        }
        let Some(change) = notes::splice(&editor.value(), text) else {
            return;
        };
        let (kept, scroll) = (editor.selected_range(), editor.scroll_offset());
        editor.set_selected_range(change.range.clone(), cx);
        editor.replace(change.text.clone(), window, cx);
        editor.set_selected_range(
            notes::shift(kept.start, &change)..notes::shift(kept.end, &change),
            cx,
        );
        editor.set_scroll_offset(scroll, cx);
    });
}

/// Whether the editor of `note` (home's id on a mirror) takes no keys: it is a mirror's and the
/// rule, a cut note or a copy being made says so.
fn readonly(ws: &Workspace, note: &Note, id: u64) -> bool {
    note.remote
        && ws
            .space
            .remote
            .as_deref()
            .is_none_or(|remote| remote.note_locked(id).is_some())
}

/// The body below the header at the block's zoom `z`: the tab strip, then the note on screen
/// (below Full, its text's bars), or a way to open one.
pub(crate) fn body(
    ws: &Workspace,
    id: BlockId,
    note: &Note,
    z: f32,
    level: Level,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let n = id.0 as usize;
    let content = ws
        .space
        .canvas
        .get(id)
        .map_or(Size::default(), |b| chrome::content_rect(b.rect).size());
    // The text's area, below the tabs.
    let area = Size::new(content.w, (content.h - TABS_H).max(0.0));
    // A mirrored note's tabs can't be added, closed or renamed: home's notes are read-only here.
    let locked = note
        .remote
        .then(|| ws.why_not(Deed::EditNote))
        .flatten()
        .map(SharedString::from);
    // Far away the tabs' place stays, empty: the bars stay where the text was.
    let strip = if level == Level::Full {
        let now = std::time::Instant::now();
        let titles = note
            .tabs
            .iter()
            .map(|t| {
                let name = ws
                    .notes_of(note)
                    .map_or(notes::UNTITLED, |notes| notes.name(t.id));
                // A mirrored note whose text isn't at home yet.
                let unsaved = note.remote
                    && ws
                        .space
                        .remote
                        .as_deref()
                        .is_some_and(|r| r.note_unsaved(t.id, now));
                if unsaved {
                    format!("{name} •").into()
                } else {
                    name.to_string().into()
                }
            })
            .collect();
        let renaming = note.renaming.as_ref().and_then(|r| {
            let at = note.tabs.iter().position(|t| t.id == r.id)?;
            Some((at, &r.input))
        });
        tab_strip(
            ws,
            id,
            titles,
            note.active,
            renaming,
            z,
            [
                Workspace::show_note_tab,
                Workspace::close_note_tab,
                Workspace::rename_note_tab,
            ],
            Workspace::new_note_tab,
            locked,
            cx,
        )
    } else {
        div().h(px(TABS_H * z)).into_any_element()
    };
    let inner = match note.tab() {
        None if level == Level::Full => empty(ws, id, note.remote, area, z, cx),
        None => div().into_any_element(),
        Some(tab) => {
            // What its word map is of.
            let taken = silhouette::Taken {
                len: tab.editor.read(cx).text().len(),
                size: (area.w, area.h),
                font: ws.cell.font.family.clone(),
                wrap: true,
                tree: false,
            };
            if level == Level::Full {
                tab.far.borrow_mut().full = Some((z, taken));
                editor_box(ws, n, &tab.editor, area, z, readonly(ws, note, tab.id), cx)
            } else {
                // Its words as bars; the wheel moves the canvas.
                silhouette::body(
                    ("silhouette", n).into(),
                    &tab.editor,
                    &tab.far,
                    taken,
                    z,
                    FONT,
                    silhouette::note_origin,
                    hsla(ws.theme.palette.foreground, BAR_OPACITY),
                    |cx| editor_box(ws, n, &tab.editor, area, z, readonly(ws, note, tab.id), cx),
                    cx,
                )
            }
        }
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .text_size(px(FONT * z))
        // The wheel scrolls the note and stops there; with Ctrl it zooms the canvas.
        .when(level == Level::Full, |el| {
            el.on_scroll_wheel(|e: &ScrollWheelEvent, _, cx| {
                if !e.modifiers.control {
                    cx.stop_propagation();
                }
            })
        })
        .child(strip)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h(px(0.0))
                // A key that would type in home's note is told once, here before the editor
                // (which is read-only) takes it.
                .when(note.remote, |el| {
                    el.capture_key_down(cx.listener(move |ws, e: &KeyDownEvent, window, cx| {
                        if would_type(&e.keystroke) {
                            ws.tell_note_locked(id, window, cx);
                        }
                    }))
                })
                .child(inner),
        )
        .into_any_element()
}

/// A button of the empty block and of the Notes dialog: an icon and a label.
pub(crate) fn button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    ws: &Workspace,
    z: f32,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    styled(id, icon, label, false, ws, z)
}

/// `button`, in the accent's tint: the one thing a block asks for first.
fn lit_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    ws: &Workspace,
    z: f32,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    styled(id, icon, label, true, ws, z)
}

/// Both buttons: `lit` tints it with the accent, else it is outlined by a hairline.
fn styled(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    lit: bool,
    ws: &Workspace,
    z: f32,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let ui = ws.ui;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.0 * z))
        .px(px(12.0 * z))
        .py(px(8.0 * z))
        .rounded(px(8.0 * z))
        .border_1()
        .text_size(px(13.0 * z))
        .cursor_pointer()
        .map(|el| {
            if lit {
                el.border_color(gpui_kit::transparent_black())
                    .bg(ui.accent.opacity(0.16))
                    .text_color(ui.accent)
                    .hover(move |s| s.bg(ui.accent.opacity(0.24)))
                    .active(move |s| s.bg(ui.accent.opacity(0.30)))
            } else {
                el.border_color(ui.hairline)
                    .text_color(ui.title)
                    .hover(move |s| s.bg(ui.hover))
                    .active(move |s| s.bg(ui.pressed))
            }
        })
        .child(Icon::new(icon).size(px(14.0 * z)))
        .child(label)
}

/// A block with no note open: a new one, or one of those there are. Roomy enough, an icon tops
/// it; the new note is the tinted button.
fn empty(
    ws: &Workspace,
    block: BlockId,
    remote: bool,
    area: Size,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let n = block.0 as usize;
    let ui = ws.ui;
    // A mirrored block's buttons are home's, dimmed with the rule's words when the mirror can't.
    let dim = |deed: Deed| remote.then(|| ws.why_not(deed)).flatten();
    let (new_why, open_why) = (dim(Deed::EditNote), dim(Deed::OpenNote));
    let gated = |button: gpui_kit::Stateful<gpui_kit::Div>, why: Option<String>| {
        button
            .when(why.is_some(), |el| el.opacity(0.4))
            .when_some(why, |el, words| {
                el.tooltip(move |window, cx| Tooltip::new(words.clone()).build(window, cx))
            })
    };
    let (roomy, tight) = (area.h >= ROOMY, area.h < TIGHT);
    let wide = |button: gpui_kit::Stateful<gpui_kit::Div>| {
        button
            .w(px(148.0 * z))
            .justify_center()
            .when(tight, |el| el.py(px(5.0 * z)))
    };
    let new = gated(
        wide(lit_button(
            ("note-new-here", n),
            IconName::Plus,
            "New note",
            ws,
            z,
        )),
        new_why,
    )
    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
        if !remote || ws.why_not(Deed::EditNote).is_none() {
            ws.new_note_tab(block, window, cx)
        }
    }))
    .test_support();
    let open = gated(
        wide(button(
            ("note-open-here", n),
            IconName::NotebookText,
            "Open a note…",
            ws,
            z,
        )),
        open_why,
    )
    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
        if !remote || ws.why_not(Deed::OpenNote).is_none() {
            open_notes(ws.space.id.clone(), Some(block), window, cx)
        }
    }))
    .test_support();
    div()
        .id(("note-empty", n))
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.0 * z))
        .when(roomy, |el| {
            el.child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(36.0 * z))
                    .mb(px(4.0 * z))
                    .rounded(px(10.0 * z))
                    .bg(ui.accent.opacity(0.14))
                    .child(
                        Icon::new(IconName::NotebookText)
                            .size(px(18.0 * z))
                            .text_color(ui.accent),
                    ),
            )
        })
        .when(!tight, |el| {
            el.child(
                div()
                    .text_size(px(13.0 * z))
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .text_color(ui.title)
                    .child("No note open here"),
            )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.0 * z))
                .when(!tight, |el| el.mt(px(8.0 * z)))
                .child(new)
                .child(open),
        )
        .test_support()
        .into_any_element()
}

/// The note's editor as Full draws it, at zoom `z`, in the text's `area`. Its fixed parts would
/// make the text start further in, wrap at other words and show fewer lines the further the
/// canvas is zoomed out: the editor is as much larger (or smaller) and moved by its fixed left
/// and top parts' share, so the text starts (`TEXT_LEFT` in), wraps and fits as if they zoomed
/// too. Its lines change only when the block is resized. At zoom 1, the editor ends where the
/// text's area does.
fn editor_box(
    ws: &Workspace,
    n: usize,
    editor: &Entity<EditorState>,
    area: Size,
    z: f32,
    readonly: bool,
    cx: &mut App,
) -> AnyElement {
    let (across, down) = (FIXED_LEFT + FIXED_RIGHT, FIXED_TOP + FIXED_BOTTOM);
    let wrap = area.w - 2.0 * BORDER - TEXT_LEFT - FIXED_RIGHT;
    crate::editor::keep_rows(editor, FONT, z, cx);
    div()
        .id(("note-editor", n))
        .size_full()
        .font_family(ws.cell.font.family.clone())
        .child(
            div()
                .id(("note-text", n))
                .relative()
                .left(px(TEXT_LEFT * z - FIXED_LEFT))
                .top(px(-FIXED_TOP * (1.0 - z)))
                .w(px((wrap * z + across).max(0.0)))
                .h(px(((area.h - down) * z + down).max(0.0)))
                .child(
                    // On the editor itself: it sets the theme's code size on itself, over a
                    // parent's.
                    Editor::new(editor)
                        .readonly(readonly)
                        .appearance(false)
                        .bordered(false)
                        .text_size(px(FONT * z))
                        .h(relative(1.0)),
                )
                .test_support(),
        )
        .test_support()
        .into_any_element()
}

/// The Notes dialog of workspace `space`: every note, the most recently changed first, and a field
/// that finds them by their text. A click opens one: in the block `into` it was asked from, where
/// it is open already, or in a new block. Its trash deletes it on a second click, from every block
/// too. On a mirror it lists home's notes the workspace's blocks show ("Open") and those last
/// closed in it ("Closed"); a click on a closed one, New note and the trash are home's.
pub(crate) fn open_notes(
    space: String,
    into: Option<BlockId>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let kind = list_dialog::Kind {
        title: "Notes",
        prefix: "note",
        find: "Find a note",
        none: "No notes yet.",
        no_match: "No note holds that.",
        new: "New note",
    };
    let (listed, opens, deletes, makes) = (space.clone(), space.clone(), space.clone(), space);
    list_dialog::open(
        kind,
        move |ws, query| note_rows(ws, &listed, query),
        move |ws, id, window, cx| {
            if ws.still_on(&opens, window, cx) {
                ws.open_note_here(id, into, window, cx)
            }
        },
        move |ws, id, window, cx| {
            if ws.still_on(&deletes, window, cx) {
                ws.delete_note_here(id, window, cx)
            }
        },
        move |ws, window, cx| {
            if ws.still_on(&makes, window, cx) {
                match into {
                    Some(block) => ws.new_note_tab(block, window, cx),
                    None => ws.new_note(window, cx),
                }
            }
        },
        window,
        cx,
    );
}

/// The Notes dialog's rows for `query`, of workspace `space`: each note's title, then when it
/// changed, where it is open and its next line ("5 min ago · Open in Workspace 2 · milk"), its
/// color's dot. A mirror's are home's: the ones its blocks show are "Open", the ones last closed
/// in it "Closed", and no other.
pub(crate) fn note_rows(ws: &Workspace, space: &str, query: &str) -> Vec<list_dialog::Row> {
    let now = notes::now();
    let Some(shown_in) = ws.space_named(space) else {
        return Vec::new();
    };
    let Some(all) = ws.notes_in(space) else {
        return Vec::new();
    };
    let remote = shown_in.remote.as_deref();
    let home = remote.and_then(|r| r.home_workspace());
    all.find(query)
        .into_iter()
        .filter_map(|note| {
            let at = match remote {
                Some(remote) => {
                    let open = remote.note_editors.contains_key(&note.id);
                    let closed = home.is_some() && note.closed_in == home;
                    if !open && !closed {
                        return None;
                    }
                    Some(if open { "Open" } else { "Closed" }.to_string())
                }
                None => list_dialog::open_at(ws, ws.note_open_at(note.id).map(|(space, _)| space)),
            };
            // The first line the name doesn't already say.
            let line = note
                .text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .find(|l| notes::title(l) != note.name)
                .map(str::to_string);
            let ago = (remote.is_none() || note.changed > 0).then(|| notes::ago(note.changed, now));
            let detail: Vec<String> = [ago, at, line].into_iter().flatten().collect();
            Some(list_dialog::Row {
                id: note.id,
                title: note.name.clone(),
                detail: detail.join(" · "),
                dot: note
                    .color
                    .map(|color| hsla(ws.theme.palette.ansi[color.ansi()], 1.0)),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keystroke(text: &str) -> Keystroke {
        Keystroke::parse(text).unwrap().with_simulated_ime()
    }

    #[test]
    fn would_type_is_a_key_that_types_or_pastes() {
        for typing in [
            "a",
            "shift-A",
            "enter",
            "backspace",
            "tab",
            "secondary-v",
            "secondary-x",
        ] {
            assert!(would_type(&keystroke(typing)), "{typing}");
        }
        for not in [
            "ctrl-t",
            "alt-1",
            "f5",
            "escape",
            "secondary-c",
            "pageup",
            "secondary-f",
        ] {
            assert!(!would_type(&keystroke(not)), "{not}");
        }
    }
}
