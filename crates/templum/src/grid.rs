//! Font metrics and painting of a terminal's grid.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, BorderStyle, Bounds, Entity, Font, FontStyle, FontWeight, Hsla, Pixels, Point, Rgba,
    TextAlign, TextRun, UnderlineStyle, Window, canvas, fill, font, outline, point, px, size,
};
use templum_canvas::Size;
use templum_terminal::{CursorShape, Rgb, SharedTerm, Snapshot};

use crate::theme::Theme;
use crate::workspace::Workspace;

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
    /// Points at zoom 1.
    pub font_size: f32,
}

impl CellMetrics {
    /// Measures a cell of `family` (unless $TEMPLUM_FONT is set) at `font_size`. The second
    /// value is a warning when `family` isn't installed.
    pub fn measure(
        window: &Window,
        family: Option<&str>,
        font_size: f32,
    ) -> (Self, Option<String>) {
        let (font, warning) = pick_font(window, family);
        let ts = window.text_system();
        let id = ts.resolve_font(&font);
        let w = ts
            .advance(id, px(font_size), 'm')
            .map(|s| f32::from(s.width))
            .unwrap_or(0.0);
        let ascent = f32::from(ts.ascent(id, px(font_size)));
        let descent = f32::from(ts.descent(id, px(font_size))).abs();
        // Never zero: a bad font must not turn into a 65535-column grid.
        let w = if w > 0.0 { w } else { font_size * 0.6 };
        let h = (ascent + descent).ceil().max(font_size);
        (
            Self {
                font,
                size: Size::new(w, h),
                font_size,
            },
            warning,
        )
    }

    /// Cell size in whole pixels, as reported to the shell.
    pub fn px(&self) -> (u16, u16) {
        (self.size.w.round() as u16, self.size.h.round() as u16)
    }
}

/// $TEMPLUM_FONT, then `family`, then a list of common monospace fonts, then any installed
/// font. Never panics: the test platform has no fonts at all.
fn pick_font(window: &Window, family: Option<&str>) -> (Font, Option<String>) {
    let names = window.text_system().all_font_names();
    let installed = |f: &str| names.iter().any(|n| n == f);
    let env = std::env::var("TEMPLUM_FONT").ok();
    let chosen = env
        .as_deref()
        .into_iter()
        .chain(family)
        .chain(MONOSPACE)
        .find(|f| installed(f))
        .map(str::to_string)
        .or_else(|| names.first().cloned())
        .unwrap_or_else(|| "monospace".to_string());
    let warning = match family {
        Some(f) if env.is_none() && !installed(f) => {
            Some(format!("Font \"{f}\" isn't installed; using \"{chosen}\""))
        }
        _ => None,
    };
    (font(chosen), warning)
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

/// The cursor's quad inside its cell: the whole cell, or a thin bar or underline (2 px at zoom
/// 1, never under 1 px).
pub(crate) fn cursor_quad(cell: Bounds<Pixels>, shape: CursorShape, z: f32) -> Bounds<Pixels> {
    let thin = px((2.0 * z).max(1.0));
    match shape {
        CursorShape::Beam => Bounds::new(cell.origin, size(thin, cell.size.height)),
        CursorShape::Underline => Bounds::new(
            point(cell.origin.x, cell.origin.y + cell.size.height - thin),
            size(cell.size.width, thin),
        ),
        _ => cell,
    }
}

/// The grid of one terminal: refreshes its snapshot (never blocking) and paints it. The
/// focused one also reports its cursor to the workspace, for the IME popup.
#[allow(clippy::too_many_arguments)]
pub(crate) fn body(
    term: SharedTerm,
    snapshot: Rc<RefCell<Snapshot>>,
    metrics: CellMetrics,
    zoom: f32,
    focused: bool,
    marked: Option<String>,
    theme: Rc<Theme>,
    blink_on: bool,
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
                blink_on,
                &theme,
                window,
                cx,
            );
            if focused {
                let at = cursor.map(|(at, _)| at);
                if let (Some(at), Some(text)) = (at, &marked) {
                    paint_marked(text, at, zoom, &metrics, &theme, window, cx);
                }
                ws.update(cx, |ws, _| {
                    ws.ime_cursor = at;
                    ws.cursor_blinks = cursor.is_some_and(|(_, blinking)| blinking);
                });
            }
        },
    )
    .size_full()
}

/// Paints the cells; returns the cursor cell's bounds and whether the cursor blinks.
#[allow(clippy::too_many_arguments)]
fn paint(
    term: &SharedTerm,
    snapshot: &RefCell<Snapshot>,
    origin: Point<Pixels>,
    z: f32,
    metrics: &CellMetrics,
    focused: bool,
    blink_on: bool,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
) -> Option<(Bounds<Pixels>, bool)> {
    let mut snap = snapshot.borrow_mut();
    if !snap.refresh(term, &theme.palette) {
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
            window.paint_quad(fill(cell(i, span.start, span.len), theme.ui.selection));
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
            px(metrics.font_size * z),
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
    let color = theme.ui.cursor.opacity(0.6);
    if !focused || c.shape == CursorShape::HollowBlock {
        window.paint_quad(outline(r, color, BorderStyle::Solid));
    } else if !(c.blinking && !blink_on) {
        window.paint_quad(fill(cursor_quad(r, c.shape, z), color));
    }
    Some((r, c.blinking))
}

/// IME pre-edit text (dead keys, compose), drawn at the cursor until committed.
fn paint_marked(
    text: &str,
    at: Bounds<Pixels>,
    z: f32,
    metrics: &CellMetrics,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
) {
    let run = TextRun {
        len: text.len(),
        font: metrics.font.clone(),
        color: theme.ui.marked,
        background_color: Some(hsla(theme.palette.background, 1.0)),
        underline: Some(UnderlineStyle {
            thickness: px(1.0),
            color: None,
            wavy: false,
        }),
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        text.to_string().into(),
        px(metrics.font_size * z),
        &[run],
        None,
    );
    let _ = line.paint_background(at.origin, at.size.height, TextAlign::Left, None, window, cx);
    let _ = line.paint(at.origin, at.size.height, TextAlign::Left, None, window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_quads_per_shape() {
        let cell = Bounds::new(point(px(10.), px(20.)), size(px(8.), px(16.)));
        assert_eq!(cursor_quad(cell, CursorShape::Block, 1.0), cell);
        let bar = cursor_quad(cell, CursorShape::Beam, 1.0);
        assert_eq!(
            (bar.origin, bar.size.width, bar.size.height),
            (cell.origin, px(2.), px(16.))
        );
        let under = cursor_quad(cell, CursorShape::Underline, 2.0);
        assert_eq!(under.size, size(px(8.), px(4.)));
        assert_eq!(under.origin.y, px(32.), "at the cell's bottom");
        // Thin shapes never vanish when zoomed out.
        assert_eq!(
            cursor_quad(cell, CursorShape::Beam, 0.25).size.width,
            px(1.)
        );
    }
}
