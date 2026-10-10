//! Themes: Alacritty color files and the interface colors derived from them.

use gpui_kit::Hsla;
use templum_terminal::{Palette, Rgb};
use toml_edit::DocumentMut;

use crate::grid::hsla;

/// How opaque the canvas is when the window is translucent.
pub(crate) const CANVAS_ALPHA: f32 = 0.72;
const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
const ANSI_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

/// A terminal palette plus the interface colors derived from it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Theme {
    pub name: String,
    pub palette: Palette,
    pub is_dark: bool,
    pub ui: Ui,
}

/// Interface colors, computed from the terminal colors with one formula for every theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ui {
    /// Under the blocks; drawn at [`CANVAS_ALPHA`] when the window is translucent.
    pub canvas: Rgb,
    pub dot: Hsla,
    pub hairline: Hsla,
    /// Block titles: the focused one, the others.
    pub title: Hsla,
    pub title_dim: Hsla,
    pub hint: Hsla,
    pub hover: Hsla,
    /// Focus ring, picker selection.
    pub accent: Hsla,
    /// Drawn over selected cells; the text keeps its color.
    pub selection: Hsla,
    pub cursor: Hsla,
    /// IME pre-edit text.
    pub marked: Hsla,
    pub shadow: Hsla,
}

impl Theme {
    pub fn new(name: &str, palette: Palette, cursor: Option<Rgb>, selection: Option<Rgb>) -> Self {
        // About CIE L* 50.
        let is_dark = luminance(palette.background) < 0.18;
        let fg = palette.foreground;
        let blue = palette.ansi[4];
        let ui = Ui {
            canvas: mix(palette.background, BLACK, if is_dark { 0.30 } else { 0.04 }),
            dot: hsla(fg, 0.12),
            hairline: hsla(fg, 0.10),
            title: hsla(fg, 0.60),
            title_dim: hsla(fg, 0.40),
            hint: hsla(fg, 0.40),
            hover: hsla(fg, 0.08),
            accent: hsla(blue, 1.0),
            selection: hsla(selection.unwrap_or(blue), 0.35),
            cursor: hsla(cursor.unwrap_or(fg), 1.0),
            marked: hsla(palette.ansi[3], 1.0),
            shadow: hsla(BLACK, if is_dark { 0.45 } else { 0.15 }),
        };
        Self {
            name: name.to_string(),
            palette,
            is_dark,
            ui,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum ThemeError {
    Toml(String),
    Missing(String),
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Toml(message) => write!(f, "not valid TOML: {message}"),
            Self::Missing(key) => write!(f, "missing {key}"),
        }
    }
}

/// Reads a theme in Alacritty's format. Only the colors Templum uses are read; everything else
/// in the file is ignored.
pub(crate) fn parse(name: &str, text: &str) -> Result<Theme, ThemeError> {
    // Windows editors may save a byte-order mark, which TOML doesn't allow.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| ThemeError::Toml(e.message().to_string()))?;
    let color = |table: &str, key: &str| {
        doc.get("colors")?
            .get(table)?
            .get(key)?
            .as_str()
            .and_then(parse_hex)
    };
    let need = |table: &str, key: &str| {
        color(table, key).ok_or_else(|| ThemeError::Missing(format!("colors.{table}.{key}")))
    };
    let background = need("primary", "background")?;
    let foreground = need("primary", "foreground")?;
    let mut ansi = [BLACK; 16];
    for (i, key) in ANSI_NAMES.iter().enumerate() {
        ansi[i] = need("normal", key)?;
        ansi[i + 8] = color("bright", key).unwrap_or(ansi[i]);
    }
    let palette = Palette {
        foreground,
        background,
        ansi,
    };
    Ok(Theme::new(
        name,
        palette,
        color("cursor", "cursor"),
        color("selection", "background"),
    ))
}

/// "#rrggbb" or "0xrrggbb". Anything else (Alacritty's "CellForeground"...) is not a color.
fn parse_hex(s: &str) -> Option<Rgb> {
    let hex = s
        .strip_prefix('#')
        .or_else(|| s.strip_prefix("0x"))
        .or_else(|| s.strip_prefix("0X"))?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(Rgb {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    })
}

/// WCAG relative luminance.
fn luminance(c: Rgb) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// Linear blend in sRGB: `t = 0` is `a`, `t = 1` is `b`.
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb {
        r: f(a.r, b.r),
        g: f(a.g, b.g),
        b: f(a.b, b.b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCHA: &str = include_str!("../themes/catppuccin-mocha.toml");
    const LATTE: &str = include_str!("../themes/catppuccin-latte.toml");
    const MINIMAL: &str = r##"
[colors.primary]
background = "0x101010"
foreground = "0XE0E0E0"

[colors.normal]
black = "#000000"
red = "#aa0000"
green = "#00aa00"
yellow = "#aaaa00"
blue = "#0000aa"
magenta = "#aa00aa"
cyan = "#00aaaa"
white = "#aaaaaa"
"##;

    fn rgb(v: u32) -> Rgb {
        Rgb {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        }
    }

    #[test]
    fn parses_catppuccin_mocha() {
        let t = parse("Catppuccin Mocha", MOCHA).unwrap();
        assert_eq!(t.name, "Catppuccin Mocha");
        assert_eq!(t.palette.background, rgb(0x1e1e2e));
        assert_eq!(t.palette.foreground, rgb(0xcdd6f4));
        let normal = [
            0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de,
        ];
        let bright = [
            0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
        ];
        let expected: Vec<Rgb> = normal.iter().chain(&bright).map(|&v| rgb(v)).collect();
        assert_eq!(t.palette.ansi.to_vec(), expected);
        assert!(t.is_dark);
    }

    #[test]
    fn accepts_0x_colors_and_copies_missing_bright() {
        let t = parse("Minimal", MINIMAL).unwrap();
        assert_eq!(t.palette.background, rgb(0x101010));
        assert_eq!(t.palette.foreground, rgb(0xe0e0e0));
        assert_eq!(t.palette.ansi[8..], t.palette.ansi[..8]);
    }

    #[test]
    fn missing_required_key_names_it() {
        let text = MINIMAL.replace("red = \"#aa0000\"\n", "");
        assert_eq!(
            parse("x", &text),
            Err(ThemeError::Missing("colors.normal.red".into()))
        );
        assert_eq!(
            parse("x", &text).unwrap_err().to_string(),
            "missing colors.normal.red"
        );
    }

    #[test]
    fn alacritty_cell_colors_count_as_missing() {
        let with_cursor = format!("{MINIMAL}\n[colors.cursor]\ncursor = \"CellForeground\"\n");
        let t = parse("x", &with_cursor).unwrap();
        assert_eq!(t.ui.cursor, hsla(t.palette.foreground, 1.0));
        let bad_bg = MINIMAL.replace("\"0x101010\"", "\"CellBackground\"");
        assert_eq!(
            parse("x", &bad_bg),
            Err(ThemeError::Missing("colors.primary.background".into()))
        );
    }

    #[test]
    fn unknown_tables_are_ignored() {
        let text = format!(
            "{MINIMAL}\n[colors.search.matches]\nforeground = \"#ffffff\"\n\n[window]\nopacity = 0.9\n"
        );
        assert!(parse("x", &text).is_ok());
    }

    #[test]
    fn invalid_toml_is_an_error() {
        assert!(matches!(parse("x", "[colors"), Err(ThemeError::Toml(_))));
    }

    #[test]
    fn parses_bom_and_crlf() {
        let windows = format!("\u{feff}{}", MINIMAL.replace('\n', "\r\n"));
        assert_eq!(parse("x", &windows).unwrap().palette.ansi[1], rgb(0xaa0000));
    }

    #[test]
    fn derived_colors() {
        let mocha = parse("Catppuccin Mocha", MOCHA).unwrap();
        assert_eq!(mocha.ui.accent, hsla(rgb(0x89b4fa), 1.0));
        assert_eq!(mocha.ui.canvas, rgb(0x151520));
        assert_eq!(mocha.ui.dot, hsla(rgb(0xcdd6f4), 0.12));
        assert_eq!(mocha.ui.selection, hsla(rgb(0xf5e0dc), 0.35));
        assert_eq!(mocha.ui.cursor, hsla(rgb(0xf5e0dc), 1.0));
        assert_eq!(mocha.ui.shadow, hsla(rgb(0), 0.45));
        let latte = parse("Catppuccin Latte", LATTE).unwrap();
        assert!(!latte.is_dark);
        assert_eq!(latte.ui.canvas, rgb(0xe5e7eb));
        assert_eq!(latte.ui.shadow, hsla(rgb(0), 0.15));
        let minimal = parse("Minimal", MINIMAL).unwrap();
        assert_eq!(
            minimal.ui.selection,
            hsla(rgb(0x0000aa), 0.35),
            "no selection: blue"
        );
        assert_eq!(
            minimal.ui.cursor,
            hsla(rgb(0xe0e0e0), 1.0),
            "no cursor: foreground"
        );
        assert_eq!(minimal.ui.marked, hsla(rgb(0xaaaa00), 1.0));
    }
}
