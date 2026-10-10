//! Font metrics and painting of a terminal's grid.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, BorderStyle, Bounds, Entity, Font, FontStyle, FontWeight, Hsla, Pixels, Point, Rgba,
    TextAlign, TextRun, UnderlineStyle, Window, canvas, fill, font, outline, point, px, rgb, size,
};
use templum_canvas::Size;
use templum_terminal::palette::{DEFAULT_BG, DEFAULT_FG};
use templum_terminal::{Rgb, SharedTerm, Snapshot};

use crate::workspace::Workspace;

/// World units: the font size at zoom 1.
pub(crate) const FONT_SIZE: f32 = 14.0;
const SELECTION: u32 = 0x3d5a80;
const MONOSPACE: [&str; 9] = [
    "JetBrainsMono Nerd Font Mono",
    "JetBrainsMono Nerd Font",
    "JetBrains Mono",
    "Cascadia Mono",
    "SF Mono",
    "Menlo",
    "Consolas",
    "DejaVu Sans Mono",
    "Liberation Mono",
];

#[derive(Clone)]
pub(crate) struct CellMetrics {
    pub font: Font,
    /// One cell at zoom 1, world units.
    pub size: Size,
}

impl CellMetrics {
    pub fn measure(window: &Window) -> Self {
        let font = pick_font(window);
        let ts = window.text_system();
        let id = ts.resolve_font(&font);
        let w = ts
            .advance(id, px(FONT_SIZE), 'm')
            .map(|s| f32::from(s.width))
            .unwrap_or(0.0);
        let ascent = f32::from(ts.ascent(id, px(FONT_SIZE)));
        let descent = f32::from(ts.descent(id, px(FONT_SIZE))).abs();
        // Never zero: a bad font must not turn into a 65535-column grid.
        let w = if w > 0.0 { w } else { FONT_SIZE * 0.6 };
        let h = (ascent + descent).ceil().max(FONT_SIZE);
        Self {
            font,
            size: Size::new(w, h),
        }
    }

    /// Cell size in whole pixels, as reported to the shell.
    pub fn px(&self) -> (u16, u16) {
        (self.size.w.round() as u16, self.size.h.round() as u16)
    }
}

/// $TEMPLUM_FONT, then a list of common monospace fonts, then any installed font. Never panics:
/// the test platform has no fonts at all.
fn pick_font(window: &Window) -> Font {
    let names = window.text_system().all_font_names();
    let wanted = std::env::var("TEMPLUM_FONT").ok();
    let family = wanted
        .as_deref()
        .into_iter()
        .chain(MONOSPACE)
        .find(|f| names.iter().any(|n| n == f))
        .map(str::to_string)
        .or_else(|| names.first().cloned())
        .unwrap_or_else(|| "monospace".to_string());
    font(family)
}

pub(crate) fn hsla(c: Rgb, a: f32) -> Hsla {
    Rgba {
        r: c.r as f32 / 255.0,
        g: c.g as f32 / 255.0,
        b: c.b as f32 / 255.0,
        a,
    }
    .into()
}

/// The grid of one terminal: refreshes its snapshot (never blocking) and paints it. The
/// focused one also reports its cursor to the workspace, for the IME popup.
pub(crate) fn body(
    term: SharedTerm,
    snapshot: Rc<RefCell<Snapshot>>,
    metrics: CellMetrics,
    zoom: f32,
    focused: bool,
    marked: Option<String>,
    ws: Entity<Workspace>,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let cursor = paint(
                &term,
                &snapshot,
                bounds.origin,
                zoom,
                &metrics,
                focused,
                window,
                cx,
            );
            if focused {
                if let (Some(at), Some(text)) = (cursor, &marked) {
                    paint_marked(text, at, zoom, &metrics.font, window, cx);
                }
                ws.update(cx, |ws, _| ws.ime_cursor = cursor);
            }
        },
    )
    .size_full()
}

/// Paints the cells; returns the cursor cell's bounds.
#[allow(clippy::too_many_arguments)]
fn paint(
    term: &SharedTerm,
    snapshot: &RefCell<Snapshot>,
    origin: Point<Pixels>,
    z: f32,
    metrics: &CellMetrics,
    focused: bool,
    window: &mut Window,
    cx: &mut App,
) -> Option<Bounds<Pixels>> {
    let mut snap = snapshot.borrow_mut();
    if !snap.refresh(term) {
        // The PTY thread is busy: try again next frame.
        window.request_animation_frame();
    }
    let (cw, lh) = (metrics.size.w * z, metrics.size.h * z);
    let cell = |row: usize, col: usize, len: usize| {
        Bounds::new(
            point(
                origin.x + px(cw * col as f32),
                origin.y + px(lh * row as f32),
            ),
            size(px(cw * len as f32), px(lh)),
        )
    };
    for (i, row) in snap.rows.iter().enumerate() {
        for (span, bg) in &row.bgs {
            window.paint_quad(fill(cell(i, span.start, span.len), hsla(*bg, 1.0)));
        }
        for span in &row.selected {
            window.paint_quad(fill(cell(i, span.start, span.len), rgb(SELECTION)));
        }
        if row.text.is_empty() {
            continue;
        }
        let runs: Vec<TextRun> = row
            .runs
            .iter()
            .map(|r| TextRun {
                len: r.len,
                font: Font {
                    weight: if r.bold {
                        FontWeight::BOLD
                    } else {
                        metrics.font.weight
                    },
                    style: if r.italic {
                        FontStyle::Italic
                    } else {
                        metrics.font.style
                    },
                    ..metrics.font.clone()
                },
                color: hsla(r.fg, if r.dim { 0.66 } else { 1.0 }),
                background_color: None,
                underline: None,
                strikethrough: None,
            })
            .collect();
        // force_width pins glyph n to column n, like a terminal grid.
        let line = window.text_system().shape_line(
            row.text.clone().into(),
            px(FONT_SIZE * z),
            &runs,
            Some(px(cw)),
        );
        let _ = line.paint(
            cell(i, 0, 0).origin,
            px(lh),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
    let c = snap.cursor?;
    let r = cell(c.row, c.col, 1);
    if focused {
        window.paint_quad(fill(r, hsla(DEFAULT_FG, 0.6)));
    } else {
        window.paint_quad(outline(r, hsla(DEFAULT_FG, 0.6), BorderStyle::Solid));
    }
    Some(r)
}

/// IME pre-edit text (dead keys, compose), drawn at the cursor until committed.
fn paint_marked(
    text: &str,
    at: Bounds<Pixels>,
    z: f32,
    font: &Font,
    window: &mut Window,
    cx: &mut App,
) {
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        color: rgb(0xfeca88).into(),
        background_color: Some(hsla(DEFAULT_BG, 1.0)),
        underline: Some(UnderlineStyle {
            thickness: px(1.0),
            color: None,
            wavy: false,
        }),
        strikethrough: None,
    };
    let line =
        window
            .text_system()
            .shape_line(text.to_string().into(), px(FONT_SIZE * z), &[run], None);
    let _ = line.paint_background(at.origin, at.size.height, TextAlign::Left, None, window, cx);
    let _ = line.paint(at.origin, at.size.height, TextAlign::Left, None, window, cx);
}
