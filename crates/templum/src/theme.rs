//! Themes: Alacritty color files, the interface colors derived from them, the built-ins and the
//! user's themes folder.

use std::path::Path;
use std::rc::Rc;

use gpui_kit::{App, Hsla};
use templum_terminal::{Palette, Rgb};
use toml_edit::DocumentMut;

use crate::grid::hsla;

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

pub(crate) const DEFAULT_LIGHT: &str = "Catppuccin Latte";
pub(crate) const DEFAULT_DARK: &str = "Catppuccin Mocha";
/// The picker's group for themes from the user's folder.
pub(crate) const MY_THEMES: &str = "My themes";

pub(crate) struct Builtin {
    pub group: &'static str,
    pub name: &'static str,
    pub toml: &'static str,
}

/// Embedded in the binary; sources and licenses are in themes/CREDITS.md.
pub(crate) const BUILTIN: [Builtin; 8] = [
    Builtin {
        group: "Catppuccin",
        name: "Catppuccin Latte",
        toml: include_str!("../themes/catppuccin-latte.toml"),
    },
    Builtin {
        group: "Catppuccin",
        name: "Catppuccin Frappé",
        toml: include_str!("../themes/catppuccin-frappe.toml"),
    },
    Builtin {
        group: "Catppuccin",
        name: "Catppuccin Macchiato",
        toml: include_str!("../themes/catppuccin-macchiato.toml"),
    },
    Builtin {
        group: "Catppuccin",
        name: "Catppuccin Mocha",
        toml: include_str!("../themes/catppuccin-mocha.toml"),
    },
    Builtin {
        group: "One",
        name: "One Light",
        toml: include_str!("../themes/one-light.toml"),
    },
    Builtin {
        group: "One",
        name: "One Dark",
        toml: include_str!("../themes/one-dark.toml"),
    },
    Builtin {
        group: "Apple",
        name: "Apple Light",
        toml: include_str!("../themes/apple-light.toml"),
    },
    Builtin {
        group: "Apple",
        name: "Apple Dark",
        toml: include_str!("../themes/apple-dark.toml"),
    },
];

pub(crate) fn default_name(dark: bool) -> &'static str {
    if dark { DEFAULT_DARK } else { DEFAULT_LIGHT }
}

/// One line of the picker.
pub(crate) struct Entry {
    pub group: &'static str,
    pub theme: Rc<Theme>,
}

/// The built-ins, then the valid themes of `<config_dir>/themes` sorted by name. A user theme
/// with a built-in's name replaces it.
pub(crate) fn load(config_dir: Option<&Path>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = BUILTIN
        .iter()
        .map(|b| Entry {
            group: b.group,
            theme: Rc::new(parse(b.name, b.toml).expect("built-in themes are valid")),
        })
        .collect();
    let mut mine = config_dir
        .map(|dir| read_folder(&dir.join("themes")))
        .unwrap_or_default();
    mine.sort_by(|a, b| a.name.cmp(&b.name));
    for theme in mine {
        entries.retain(|e| e.theme.name != theme.name);
        entries.push(Entry {
            group: MY_THEMES,
            theme: Rc::new(theme),
        });
    }
    entries
}

/// Every valid `*.toml` directly inside `folder`. Broken files are reported and skipped.
fn read_folder(folder: &Path) -> Vec<Theme> {
    let files = match std::fs::read_dir(folder) {
        Ok(files) => files,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            eprintln!("templum: couldn't read {}: {e}", folder.display());
            return Vec::new();
        }
    };
    let mut themes = Vec::new();
    for path in files.flatten().map(|f| f.path()) {
        if path.extension().is_none_or(|e| e != "toml") || !path.is_file() {
            continue;
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let theme = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| parse(&name, &text).map_err(|e| e.to_string()));
        match theme {
            Ok(theme) => themes.push(theme),
            Err(err) => eprintln!("templum: skipping theme {}: {err}", path.display()),
        }
    }
    themes
}

pub(crate) fn find(themes: &[Entry], name: &str) -> Option<Rc<Theme>> {
    themes
        .iter()
        .find(|e| e.theme.name == name)
        .map(|e| e.theme.clone())
}

/// The theme called `name`, or `default` (with a warning) when there is none.
pub(crate) fn pick(themes: &[Entry], name: &str, default: &str) -> Rc<Theme> {
    find(themes, name)
        .or_else(|| {
            eprintln!("templum: no theme named {name:?}, using {default:?}");
            find(themes, default)
        })
        .expect("the default themes are built in")
}

/// Gives gpui-kit's own widgets (title bar, notifications) the theme's colors.
pub(crate) fn apply_to_kit(theme: &Theme, cx: &mut App) {
    use gpui_kit::component::{Theme as Kit, ThemeMode};
    let mode = if theme.is_dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    // A mode change reloads gpui-kit's built-in colors: switch first, then set ours.
    Kit::update(cx, |kit| kit.mode = mode);
    let ui = theme.ui;
    let background = hsla(theme.palette.background, 1.0);
    let foreground = hsla(theme.palette.foreground, 1.0);
    Kit::update(cx, |kit| {
        // gpui-kit's root paints `background` under the whole window: keep it clear for the glass.
        kit.background = gpui_kit::transparent_black();
        kit.title_bar = gpui_kit::transparent_black();
        kit.popover = background;
        kit.foreground = foreground;
        kit.popover_foreground = foreground;
        kit.border = ui.hairline;
        kit.title_bar_border = ui.hairline;
        kit.window_border = ui.hairline;
        kit.muted_foreground = ui.title_dim;
        kit.accent = ui.accent;
        kit.ring = ui.accent;
        kit.selection = ui.selection;
    });
}

/// How opaque the canvas is when the window is translucent.
pub(crate) const CANVAS_ALPHA: f32 = 0.72;

/// Whether the window is drawn translucent with the desktop blurred behind it: where the
/// system blurs (macOS, Windows, KDE and Hyprland under Wayland), never elsewhere.
pub(crate) fn translucent(os: &str, desktop: Option<&str>, wayland: bool) -> bool {
    match os {
        "macos" | "windows" => true,
        // XDG_CURRENT_DESKTOP is a colon-separated list.
        "linux" => {
            wayland && desktop.is_some_and(|d| d.split(':').any(|d| d == "KDE" || d == "Hyprland"))
        }
        _ => false,
    }
}

pub(crate) fn system_translucent() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    translucent(
        std::env::consts::OS,
        desktop.as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
    )
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
    #[test]
    fn all_builtins_load_in_order() {
        let all = load(None);
        assert_eq!(all.len(), BUILTIN.len());
        for (entry, builtin) in all.iter().zip(&BUILTIN) {
            assert_eq!(entry.theme.name, builtin.name);
            assert_eq!(entry.group, builtin.group);
        }
        // Frappé, Macchiato, Mocha, One Dark, Apple Dark.
        assert_eq!(all.iter().filter(|e| e.theme.is_dark).count(), 5);
    }

    #[test]
    fn pick_falls_back_to_the_default() {
        let all = load(None);
        assert_eq!(pick(&all, "One Dark", DEFAULT_DARK).name, "One Dark");
        assert_eq!(
            pick(&all, "No Such Theme", DEFAULT_DARK).name,
            "Catppuccin Mocha"
        );
        assert_eq!(default_name(false), "Catppuccin Latte");
        assert_eq!(default_name(true), "Catppuccin Mocha");
    }

    #[test]
    fn user_themes_load_sorted_and_win_over_builtins() {
        let dir = tempfile::tempdir().unwrap();
        let themes = dir.path().join("themes");
        std::fs::create_dir(&themes).unwrap();
        std::fs::write(themes.join("zeta.toml"), MINIMAL).unwrap();
        std::fs::write(themes.join("One Dark.toml"), MINIMAL).unwrap();
        std::fs::write(themes.join("broken.toml"), "[colors").unwrap();
        std::fs::write(themes.join("notes.txt"), MINIMAL).unwrap();
        let all = load(Some(dir.path()));
        let mine: Vec<&str> = all
            .iter()
            .filter(|e| e.group == MY_THEMES)
            .map(|e| e.theme.name.as_str())
            .collect();
        assert_eq!(mine, ["One Dark", "zeta"]);
        assert_eq!(
            all.len(),
            BUILTIN.len() + 1,
            "One Dark replaced, zeta added"
        );
        assert_eq!(
            find(&all, "One Dark").unwrap().palette.background,
            rgb(0x101010)
        );
    }

    #[test]
    fn a_missing_themes_folder_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(Some(dir.path())).len(), BUILTIN.len());
    }

    #[test]
    fn translucent_only_where_the_system_blurs() {
        assert!(translucent("macos", None, false));
        assert!(translucent("windows", None, false));
        assert!(translucent("linux", Some("Hyprland"), true));
        assert!(translucent("linux", Some("KDE"), true));
        assert!(translucent("linux", Some("ubuntu:KDE"), true));
        assert!(!translucent("linux", Some("KDE"), false), "X11");
        assert!(!translucent("linux", Some("GNOME"), true));
        assert!(!translucent("linux", None, true));
        assert!(!translucent("freebsd", Some("KDE"), true));
    }
}
