//! One row of the diff panel as an element: a section heading, a file row with its actions, a
//! hunk row, a fold, a note, or a line painted on a canvas in the terminal's font.

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, ContentMask, Context, Div, ElementId, Font, Hsla,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, SharedString, Stateful, TestSupportExt,
    TextAlign, TextRun, Window, canvas, div, fill, point, px, size,
};
use templum_git::conflict::Take;
use templum_git::{Change, FileDiff, Kind};

use super::select::display;
use super::{Note, Row, Shaped, Shown, Source, files_word};
use crate::editor::human;
use crate::git_block::widgets::{badge, icon_button, small};
use crate::git_block::{FONT, LISTS_W, words};
use crate::grid::hsla;
use crate::menu::Target;
use crate::theme::Ui;
use crate::workspace::Workspace;
use crate::workspace::remote::Deed;

/// Row `ix` of the panel.
#[allow(clippy::too_many_arguments)]
pub(super) fn row(
    ws: &Workspace,
    files: &Rc<Vec<Shown>>,
    rows: &[Row],
    ix: usize,
    cursor: Option<usize>,
    row_h: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    match &rows[ix] {
        Row::Section { staged, files: n } => section(*staged, *n, ws.ui, row_h, z),
        Row::File { file } => file_row(ws, &files[*file], ix, row_h, z, cx),
        Row::Hunk { file, hunk } => hunk_row(
            ws,
            files,
            *file,
            *hunk,
            ix,
            cursor == Some(ix),
            row_h,
            z,
            cx,
        ),
        Row::Fold { file, lines } => fold_row(ws, &files[*file], ix, *lines, row_h, z, cx),
        Row::Note { file, note } => note_row(ws, &files[*file], ix, *note, row_h, z, cx),
        Row::Line { file, hunk, line } => {
            line_row(ws, &files[*file], *file, *hunk, *line, ix, row_h, z, cx)
        }
        Row::Region { file, region } => region_row(ws, &files[*file], *region, ix, row_h, z, cx),
        Row::Pair {
            file,
            hunk,
            old,
            new,
        } => pair_row(
            ws,
            &files[*file],
            *file,
            *hunk,
            *old,
            *new,
            ix,
            row_h,
            z,
            cx,
        ),
    }
}

/// A conflict region's two parts' lines, as drawn: what a choice checks the file still has.
fn sides(s: &Shown, region: usize) -> (Vec<String>, Vec<String>) {
    let texts = |h: usize| -> Vec<String> {
        s.diff.hunks.get(h).map_or_else(Vec::new, |h| {
            h.lines.iter().map(|l| l.text.clone()).collect()
        })
    };
    (texts(2 * region), texts(2 * region + 1))
}

/// A conflict region's heading: "Conflict 1 of 2", and on hover Use both and Edit in editor
/// (at its `<<<<<<<` line).
#[allow(clippy::too_many_arguments)]
fn region_row(
    ws: &Workspace,
    s: &Shown,
    region: usize,
    ix: usize,
    row_h: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let busy = ws.space.git().is_some_and(|g| g.acting > 0);
    let path = s.diff.path().to_string();
    let ids = |what: &str| ElementId::Name(format!("git-region-{what}-{ix}").into());
    // The first part starts on the line after `<<<<<<<`.
    let marker = s
        .diff
        .hunks
        .get(2 * region)
        .map_or(1, |h| h.new_start.saturating_sub(1).max(1));
    let mut buttons: Vec<AnyElement> = Vec::new();
    if !busy {
        let (both, shown) = (path.clone(), sides(s, region));
        buttons.push(
            small(ids("both"), "Use both", ui, z)
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.resolve_region(both.clone(), region, Take::Both, shown.clone(), window, cx)
                }))
                .test_support()
                .into_any_element(),
        );
        buttons.push(
            small(ids("edit"), "Edit in editor", ui, z)
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.open_from_git_at(&path, Some(marker), window, cx)
                }))
                .test_support()
                .into_any_element(),
        );
    }
    div()
        .id(("git-region", ix))
        .group("git-region")
        .w_full()
        .h(px(row_h))
        .flex()
        .items_center()
        .gap(px(6.0 * z))
        .px(px(8.0 * z))
        .bg(ui.card)
        .text_size(px(FONT * z))
        .text_color(ui.title)
        .child(div().flex_1().min_w(px(0.0)).truncate().child(format!(
            "Conflict {} of {}",
            region + 1,
            s.diff.hunks.len() / 2
        )))
        .child(
            div()
                .flex()
                .gap(px(4.0 * z))
                .children(buttons)
                .invisible()
                .group_hover("git-region", |s| s.visible()),
        )
        .test_support()
        .into_any_element()
}

/// "+12 −3", dimmed.
pub(super) fn counts(d: &FileDiff, ui: Ui) -> Div {
    div()
        .flex_shrink_0()
        .text_color(ui.title_dim)
        .child(format!("+{} −{}", d.added(), d.removed()))
}

fn section(staged: bool, n: usize, ui: Ui, row_h: f32, z: f32) -> AnyElement {
    div()
        .w_full()
        .h(px(row_h))
        .flex()
        .items_center()
        .px(px(8.0 * z))
        .text_size(px(11.0 * z))
        .text_color(ui.title_dim)
        .child(format!(
            "{} · {}",
            if staged { "Staged" } else { "Changes" },
            files_word(n)
        ))
        .into_any_element()
}

/// The file's row: its fold arrow, badge, name (a rename as "old → new"), folder dimmed,
/// counts, and on hover its actions. A click folds it.
fn file_row(
    ws: &Workspace,
    s: &Shown,
    ix: usize,
    row_h: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let d = &s.diff;
    let (key, path) = (s.key(), d.path().to_string());
    let git = ws.space.git();
    let p = git.map(|g| g.diff());
    let folded = p.is_some_and(|p| p.folded.contains(&key));
    let working = p.is_some_and(|p| p.working());
    let busy = git.is_some_and(|g| g.acting > 0);
    // The badge from the status (an untracked file is U there, A in its diff); the diff's own
    // when the status hasn't caught up with the file, and for a range (not the working tree).
    let entry = git
        .filter(|_| working)
        .and_then(|g| g.status.entries.iter().find(|e| e.path == path));
    let change = entry
        .and_then(|e| if s.staged { e.staged } else { e.unstaged })
        .unwrap_or(d.status);
    let ids = |what: &str| ElementId::Name(format!("git-panel-{what}-{ix}").into());
    // Shown while the row is hovered; pressing one never folds the file.
    let hover_button = |what: &str, icon: IconName, tip: SharedString| {
        icon_button(ids(what), icon, ui, z)
            .invisible()
            .group_hover("git-file", |s| s.visible())
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    };
    let mut buttons: Vec<AnyElement> = Vec::new();
    // A range is read-only; a conflicted file's choices are its regions' and its menu's.
    if !busy && working && d.status != Change::Conflict {
        if s.staged {
            let p = path.clone();
            buttons.push(
                hover_button("unstage", IconName::Minus, words::named("Unstage").into())
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        ws.unstage(vec![p.clone()], window, cx)
                    }))
                    .test_support()
                    .into_any_element(),
            );
        } else {
            let (p, q) = (path.clone(), path.clone());
            buttons.push(
                hover_button("stage", IconName::Plus, words::named("Stage").into())
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        ws.stage(vec![p.clone()], window, cx)
                    }))
                    .test_support()
                    .into_any_element(),
            );
            buttons.push(match ws.why_not(Deed::Git("discard")) {
                Some(why) => hover_button("discard", IconName::Undo2, why.into())
                    .opacity(0.5)
                    .test_support()
                    .into_any_element(),
                None => hover_button("discard", IconName::Undo2, words::named("Discard").into())
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        ws.discard(vec![q.clone()], window, cx)
                    }))
                    .test_support()
                    .into_any_element(),
            });
        }
    }
    if !busy && working && !ws.on_remote() {
        let p = path.clone();
        buttons.push(
            hover_button("open", IconName::FileCode, "Open in editor".into())
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.open_from_git(&p, window, cx)
                }))
                .test_support()
                .into_any_element(),
        );
    }
    let renamed = matches!((&d.old_path, &d.new_path), (Some(o), Some(n)) if o != n);
    let name = if renamed {
        format!(
            "{} → {}",
            d.old_path.as_deref().unwrap_or(""),
            d.new_path.as_deref().unwrap_or("")
        )
    } else {
        path.rsplit('/').next().unwrap_or(&path).to_string()
    };
    let folder = if renamed {
        String::new()
    } else {
        path.rsplit_once('/').map_or("", |(dir, _)| dir).to_string()
    };
    div()
        .id(("git-file", ix))
        .group("git-file")
        .w_full()
        .h(px(row_h))
        .flex()
        .items_center()
        .gap(px(6.0 * z))
        .px(px(8.0 * z))
        .bg(ui.card)
        .text_size(px(FONT * z))
        .text_color(ui.title)
        .cursor_pointer()
        .child(
            Icon::new(if folded {
                IconName::ChevronRight
            } else {
                IconName::ChevronDown
            })
            .size(px(12.0 * z))
            .text_color(ui.title_dim),
        )
        .child(badge(ws, change, z))
        .child(div().truncate().child(name))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_color(ui.title_dim)
                .child(folder),
        )
        .child(counts(d, ui))
        .children(buttons)
        .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.toggle_fold(key.clone(), cx)))
        .test_support()
        .into_any_element()
}

/// A hunk's row: where it is, and its buttons: Stage and Discard in Changes, Unstage in Staged
/// (only for a file taken apart by hunk, and not while whitespace is ignored: the patch would
/// miss it), and Open at its line. Shown while the row is hovered.
#[allow(clippy::too_many_arguments)]
fn hunk_row(
    ws: &Workspace,
    files: &Rc<Vec<Shown>>,
    file: usize,
    hunk: usize,
    ix: usize,
    on_cursor: bool,
    row_h: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let s = &files[file];
    let h = &s.diff.hunks[hunk];
    let git = ws.space.git();
    let busy = git.is_some_and(|g| g.acting > 0);
    let working = git.is_some_and(|g| g.diff().working());
    let by_hunk = working && s.diff.by_hunk() && git.is_some_and(|g| !g.diff().ignore_ws);
    // A blame's group: its heading names the commit, and a click opens it (none for the lines
    // not committed yet).
    let blame = git.and_then(|g| match &g.diff().source {
        Source::Blame { hashes, .. } => Some(hashes.get(hunk).cloned().unwrap_or_default()),
        _ => None,
    });
    let opens = blame
        .clone()
        .filter(|h| !h.is_empty() && !h.bytes().all(|b| b == b'0'));
    let path = s.diff.path().to_string();
    let ids = |what: &str| ElementId::Name(format!("git-hunk-{what}-{ix}").into());
    // A conflict region's part: named after its marker's label, with Use this.
    let part = s.diff.status == Change::Conflict;
    // Built at the click from the files as drawn: what the user saw is what is applied.
    let drawn = files.clone();
    let patch = move || templum_git::hunk_patch(&drawn[file].diff, hunk);
    let mut buttons: Vec<AnyElement> = Vec::new();
    if !busy && working && part {
        let (shown, path) = (sides(s, hunk / 2), path.clone());
        let take = if hunk.is_multiple_of(2) {
            Take::First
        } else {
            Take::Second
        };
        buttons.push(
            small(ids("use"), "Use this", ui, z)
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.resolve_region(path.clone(), hunk / 2, take, shown.clone(), window, cx)
                }))
                .test_support()
                .into_any_element(),
        );
    }
    if !busy && by_hunk {
        if s.staged {
            let patch = patch.clone();
            buttons.push(
                small(ids("unstage"), "Unstage", ui, z)
                    .tooltip(|window, cx| Tooltip::new(words::tip("Unstage")).build(window, cx))
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        if let Some(p) = patch() {
                            ws.apply_hunk(p, true, window, cx)
                        }
                    }))
                    .test_support()
                    .into_any_element(),
            );
        } else {
            let stage = patch.clone();
            buttons.push(
                small(ids("stage"), "Stage", ui, z)
                    .tooltip(|window, cx| Tooltip::new(words::tip("Stage")).build(window, cx))
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        if let Some(p) = stage() {
                            ws.apply_hunk(p, false, window, cx)
                        }
                    }))
                    .test_support()
                    .into_any_element(),
            );
            let (discard, path) = (patch.clone(), path.clone());
            buttons.push(match ws.why_not(Deed::Git("discard_hunk")) {
                Some(why) => small(ids("discard"), "Discard", ui, z)
                    .opacity(0.5)
                    .tooltip(move |window, cx| Tooltip::new(why.clone()).build(window, cx))
                    .test_support()
                    .into_any_element(),
                None => small(ids("discard"), "Discard", ui, z)
                    .tooltip(|window, cx| Tooltip::new(words::tip("Discard")).build(window, cx))
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        if let Some(p) = discard() {
                            ws.discard_hunk(path.clone(), p, window, cx)
                        }
                    }))
                    .test_support()
                    .into_any_element(),
            });
        }
    }
    if !busy && working && !part && !ws.on_remote() {
        let line = h.new_start.max(1);
        buttons.push(
            small(ids("open"), "Open", ui, z)
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.open_from_git_at(&path, Some(line), window, cx)
                }))
                .test_support()
                .into_any_element(),
        );
    }
    div()
        .id(("git-hunk", ix))
        .group("git-hunk")
        .w_full()
        .h(px(row_h))
        .flex()
        .items_center()
        .gap(px(6.0 * z))
        .px(px(8.0 * z))
        .bg(ui.hover)
        .when(on_cursor, |el| el.border_l_2().border_color(ui.accent))
        .text_size(px(FONT * z))
        .text_color(ui.title_dim)
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .font_family(ws.cell.font.family.clone())
                .child(if part || blame.is_some() {
                    h.context.clone()
                } else {
                    format!("@@ {}", h.title())
                }),
        )
        .when_some(opens, |el, hash| {
            el.cursor_pointer()
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.show_commit(hash.clone(), window, cx)
                }))
        })
        .child({
            let tools = div().flex().gap(px(4.0 * z)).children(buttons);
            if on_cursor {
                tools
            } else {
                tools.invisible().group_hover("git-hunk", |s| s.visible())
            }
        })
        .test_support()
        .into_any_element()
}

/// "⋯ 40 unchanged lines": a click shows the file whole.
#[allow(clippy::too_many_arguments)]
fn fold_row(
    ws: &Workspace,
    s: &Shown,
    ix: usize,
    lines: u32,
    row_h: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let key = s.key();
    // ponytail: a comparison's fold isn't expandable; read the range again with -U if people
    // ask.
    let working = ws.space.git().is_some_and(|g| g.diff().working());
    div()
        .id(("git-fold", ix))
        .w_full()
        .h(px(row_h))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.0 * z))
        .text_color(ui.title_dim)
        .child(format!(
            "⋯ {lines} unchanged line{}",
            if lines == 1 { "" } else { "s" }
        ))
        .when(working, |el| {
            el.cursor_pointer()
                .hover(move |st| st.bg(ui.hover))
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.show_whole(key.clone(), window, cx)
                }))
        })
        .test_support()
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn note_row(
    ws: &Workspace,
    s: &Shown,
    ix: usize,
    note: Note,
    row_h: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let d = &s.diff;
    let (key, path) = (s.key(), d.path().to_string());
    let working = ws.space.git().is_some_and(|g| g.diff().working());
    let text = match note {
        Note::Binary => format!("Binary file · {}", human(d.bytes.unwrap_or(0))),
        Note::TooLarge => "Too large to show".to_string(),
        Note::Cut => format!(
            "Too large to show from {}.",
            ws.space
                .remote
                .as_deref()
                .map(crate::mirror::computer_of)
                .unwrap_or_default()
        ),
        Note::Large if d.generated() => "Generated file".to_string(),
        Note::Large => format!("{} changed lines", d.changed()),
        Note::Mode => {
            let (old, new) = d.mode.clone().unwrap_or_default();
            format!("Mode {old} → {new}")
        }
        Note::NoNewline => "No newline at end of file".to_string(),
        Note::Unread => format!("New file · {} · not read yet", human(d.bytes.unwrap_or(0))),
        Note::Resolved => "No conflict markers left".to_string(),
    };
    let row = div()
        .id(("git-note", ix))
        .w_full()
        .h(px(row_h))
        .flex()
        .items_center()
        .gap(px(8.0 * z))
        .px(px(24.0 * z))
        .text_size(px(FONT * z))
        .text_color(ui.title_dim)
        .child(text);
    let row = match note {
        Note::Large => row.child(
            small(("git-show-diff", ix), "Show diff", ui, z)
                .on_click(
                    cx.listener(move |ws, _: &ClickEvent, _, cx| ws.show_large(key.clone(), cx)),
                )
                .test_support(),
        ),
        Note::TooLarge if working && !ws.on_remote() => row.child(
            small(("git-open-large", ix), "Open in editor", ui, z)
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.open_from_git(&path, window, cx)
                }))
                .test_support(),
        ),
        Note::Unread if working => row.child(
            small(("git-show-diff", ix), "Show diff", ui, z)
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.show_whole(key.clone(), window, cx)
                }))
                .test_support(),
        ),
        Note::Resolved if working => row.child(
            small(("git-mark-resolved", ix), "Mark resolved", ui, z)
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.stage(vec![path.clone()], window, cx)
                }))
                .test_support(),
        ),
        Note::TooLarge
        | Note::Cut
        | Note::Unread
        | Note::Resolved
        | Note::Binary
        | Note::Mode
        | Note::NoNewline => row,
    };
    row.test_support().into_any_element()
}

/// What a line row paints.
pub(super) struct LinePaint {
    pub text: SharedString,
    pub digits: usize,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub sign: char,
    /// The soft green or red behind an added or removed line.
    pub bg: Option<Hsla>,
    /// The language's colors, byte ranges of `text` (Task 6).
    pub colors: Rc<Vec<(Range<usize>, Hsla)>>,
    /// The changed words, a stronger green or red (Task 6).
    pub marks: Rc<Vec<Range<usize>>>,
    pub mark: Hsla,
    /// The selected bytes (Task 7).
    pub selection: Option<Range<usize>>,
    pub font: Font,
    /// One glyph's advance, screen pixels.
    pub cw: f32,
    pub row_h: f32,
    /// The text's size, screen pixels.
    pub size: f32,
    pub ui: Ui,
}

/// What a line of the panel paints: its text, numbers, colors and selection. In Split, the
/// selection shows on the side it was made on.
#[allow(clippy::too_many_arguments)]
fn line_paint(
    ws: &Workspace,
    s: &Shown,
    file: usize,
    hunk: usize,
    line: usize,
    ix: usize,
    right: bool,
    row_h: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> LinePaint {
    let ui = ws.ui;
    let l = &s.diff.hunks[hunk].lines[line];
    let ansi = &ws.theme.palette.ansi;
    let (bg, mark) = match l.kind {
        Kind::Added => (Some(hsla(ansi[2], 0.15)), hsla(ansi[2], 0.35)),
        Kind::Removed => (Some(hsla(ansi[1], 0.15)), hsla(ansi[1], 0.35)),
        Kind::Context => (None, ui.selection),
    };
    let theme = cx.theme().highlight_theme.clone();
    let hunk_colors = ws
        .space
        .git()
        .map(|g| g.diff().colors.borrow_mut().of(file, hunk, s, &theme));
    let (colors, marks) = match &hunk_colors {
        Some(c) => (
            Rc::new(c.lines[line].clone()),
            Rc::new(c.marks[line].clone()),
        ),
        None => (Rc::new(Vec::new()), Rc::new(Vec::new())),
    };
    let text: SharedString = display(&l.text).into();
    let selection = ws
        .space
        .git()
        .map(|g| g.diff())
        .filter(|p| !p.split || p.split_right == right)
        .and_then(|p| p.selection)
        .and_then(|sel| sel.range_in(ix, text.len()));
    LinePaint {
        text,
        digits: s.digits(),
        old_no: l.old_no,
        new_no: l.new_no,
        sign: l.kind.sign(),
        bg,
        colors,
        marks,
        mark,
        selection,
        font: ws.cell.font.clone(),
        // The cell was measured at the terminal's size: scale its advance to the panel's.
        cw: ws.cell.size.w * FONT / ws.cell.font_size * z,
        row_h,
        size: FONT * z,
        ui,
    }
}

/// A line row's mouse: press and drag select, right-click opens its menu.
fn line_mouse(el: Stateful<Div>, ix: usize, cx: &mut Context<Workspace>) -> Stateful<Div> {
    el.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |ws, e: &MouseDownEvent, _, cx| {
            ws.diff_press(ix, e.position.x, e.click_count, cx)
        }),
    )
    .on_mouse_move(cx.listener(move |ws, e: &MouseMoveEvent, _, cx| {
        if e.pressed_button == Some(MouseButton::Left) {
            ws.diff_drag(ix, e.position.x, cx)
        }
    }))
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
            ws.open_menu(Target::GitLine { row: ix }, e.position, window, cx);
            cx.stop_propagation();
        }),
    )
}

#[allow(clippy::too_many_arguments)]
fn line_row(
    ws: &Workspace,
    s: &Shown,
    file: usize,
    hunk: usize,
    line: usize,
    ix: usize,
    row_h: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let paint = line_paint(ws, s, file, hunk, line, ix, false, row_h, z, cx);
    // The whole text, so the list (as wide as its widest row) scrolls to the end of it; the
    // trailing cell keeps the last glyph off the edge.
    let width =
        paint.cw * (super::text_columns(s.digits()) + paint.text.chars().count() + 1) as f32;
    let shaped = ws.space.git().map(|g| g.diff().shaped.clone());
    let row = div()
        .id(("git-line", ix))
        .w_full()
        .min_w(px(width))
        .h(px(row_h))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, cx| {
                    let (line, x) = paint_line(&paint, bounds, window, cx);
                    if let Some(shaped) = &shaped {
                        let left = bounds.origin.x;
                        shaped
                            .borrow_mut()
                            .insert((ix, false), Shaped { line, x, left });
                    }
                },
            )
            .size_full(),
        );
    line_mouse(row, ix, cx).test_support().into_any_element()
}

/// Split: the old line in the left half (its old number), the new one in the right half (its
/// new number), an empty cell dimmed; each half `half` columns wide.
#[allow(clippy::too_many_arguments)]
fn pair_row(
    ws: &Workspace,
    s: &Shown,
    file: usize,
    hunk: usize,
    old: Option<usize>,
    new: Option<usize>,
    ix: usize,
    row_h: f32,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let git = ws.space.git();
    let half = git.map_or(0, |g| g.diff().half);
    let cw = ws.cell.size.w * FONT / ws.cell.font_size * z;
    // The panel's width: the popup's as drawn, less the left column while it shows.
    let lists = if git.is_some_and(|g| g.lists_folded) {
        0.0
    } else {
        LISTS_W
    };
    let panel_w = (ws.git_popup_size().w - lists) * z;
    // ponytail: a side is at most half the panel, so the added side is always in view; a line
    // longer than that is cut at its side (Unified shows it whole). Scroll each side alone if
    // people need the end of long lines side by side.
    let half_w = px((cw * half as f32).min((panel_w / 2.0).floor()));
    let mut side = |line: Option<usize>, right: bool| {
        line.map(|l| {
            let mut p = line_paint(ws, s, file, hunk, l, ix, right, row_h, z, cx);
            if right {
                p.old_no = None
            } else {
                p.new_no = None
            }
            p
        })
    };
    let (left, right) = (side(old, false), side(new, true));
    let empty = ws.ui.hover;
    let shaped = ws.space.git().map(|g| g.diff().shaped.clone());
    let row = div()
        .id(("git-pair", ix))
        .w_full()
        .min_w(half_w * 2.0)
        .h(px(row_h))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, cx| {
                    for (paint, right, x0) in [
                        (left.as_ref(), false, bounds.origin.x),
                        (right.as_ref(), true, bounds.origin.x + half_w),
                    ] {
                        let cell = Bounds::new(
                            point(x0, bounds.origin.y),
                            size(half_w, bounds.size.height),
                        );
                        match paint {
                            Some(p) => {
                                let mask = ContentMask { bounds: cell };
                                let (line, x) = window.with_content_mask(Some(mask), |window| {
                                    paint_line(p, cell, window, cx)
                                });
                                if let Some(shaped) = &shaped {
                                    shaped
                                        .borrow_mut()
                                        .insert((ix, right), Shaped { line, x, left: x0 });
                                }
                            }
                            None => window.paint_quad(fill(cell, empty)),
                        }
                    }
                },
            )
            .size_full(),
        );
    line_mouse(row, ix, cx).test_support().into_any_element()
}

/// Paints one line: its background, the changed words, the selection, the numbers, the sign
/// and the text. Returns the shaped text and where it starts.
pub(super) fn paint_line(
    p: &LinePaint,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> (gpui_kit::ShapedLine, Pixels) {
    let (x0, y) = (bounds.origin.x, bounds.origin.y);
    // Columns: [1][old][1][new][1][sign][1][text].
    let col = |c: usize| x0 + px(p.cw * c as f32);
    let text_x = col(2 * p.digits + 5);
    if let Some(bg) = p.bg {
        window.paint_quad(fill(bounds, bg));
    }
    let runs = runs(&p.text, &p.colors, &p.font, p.ui.title);
    let line = window
        .text_system()
        .shape_line(p.text.clone(), px(p.size), &runs, None);
    let span = |r: &Range<usize>| {
        let (a, b) = (line.x_for_index(r.start), line.x_for_index(r.end));
        Bounds::new(point(text_x + a, y), size(b - a, bounds.size.height))
    };
    for r in p.marks.iter() {
        window.paint_quad(fill(span(r), p.mark));
    }
    if let Some(r) = p.selection.as_ref().filter(|r| r.start < r.end) {
        window.paint_quad(fill(span(r), p.ui.selection));
    }
    let mut dim = |s: String, at: usize| {
        let s: SharedString = s.into();
        let run = TextRun {
            len: s.len(),
            font: p.font.clone(),
            color: p.ui.title_dim,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let l = window.text_system().shape_line(s, px(p.size), &[run], None);
        let _ = l.paint(
            point(col(at), y),
            px(p.row_h),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    };
    if let Some(n) = p.old_no {
        dim(format!("{n:>w$}", w = p.digits), 1);
    }
    if let Some(n) = p.new_no {
        dim(format!("{n:>w$}", w = p.digits), p.digits + 2);
    }
    if p.sign != ' ' {
        dim(p.sign.to_string(), 2 * p.digits + 3);
    }
    let _ = line.paint(
        point(text_x, y),
        px(p.row_h),
        TextAlign::Left,
        None,
        window,
        cx,
    );
    (line, text_x)
}

/// Text runs covering `text`: `colors` where they fall, `default` between them.
fn runs(text: &str, colors: &[(Range<usize>, Hsla)], font: &Font, default: Hsla) -> Vec<TextRun> {
    let run = |len: usize, color: Hsla| TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut out = Vec::new();
    let mut at = 0;
    for (r, color) in colors {
        let (start, end) = (r.start.max(at), r.end.min(text.len()));
        if start >= end {
            continue;
        }
        if start > at {
            out.push(run(start - at, default));
        }
        out.push(run(end - start, *color));
        at = end;
    }
    if at < text.len() {
        out.push(run(text.len() - at, default));
    }
    out
}
