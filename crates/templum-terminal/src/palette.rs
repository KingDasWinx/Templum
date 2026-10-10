//! Colors: the palette a terminal is drawn with (foreground, background, 16 ANSI colors) and the
//! xterm 256-color cube around it.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor};

use crate::Rgb;

/// A theme's terminal colors. Indexes 16–255 are always the xterm cube and grays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub foreground: Rgb,
    pub background: Rgb,
    /// Normal colors (0–7), then bright ones (8–15).
    pub ansi: [Rgb; 16],
}

// Adapted from Alacritty (Apache-2.0): alacritty/src/config/color.rs default colors.
const DEFAULT_ANSI: [u32; 16] = [
    0x181818, 0xac4242, 0x90a959, 0xf4bf75, 0x6a9fb5, 0xaa759f, 0x75b5aa, 0xd8d8d8, // normal
    0x6b6b6b, 0xc55555, 0xaac474, 0xfeca88, 0x82b8c8, 0xc28cb8, 0x93d3c3, 0xf8f8f8, // bright
];

impl Default for Palette {
    fn default() -> Self {
        Self {
            foreground: hex(0xd8d8d8),
            background: hex(0x181818),
            ansi: DEFAULT_ANSI.map(hex),
        }
    }
}

fn hex(v: u32) -> Rgb {
    Rgb {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    }
}

/// xterm 256-color palette: the theme's 16 colors, a 6x6x6 cube, 24 grays.
fn indexed(i: u8, p: &Palette) -> Rgb {
    match i {
        0..16 => p.ansi[i as usize],
        16..232 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { v * 40 + 55 };
            Rgb {
                r: level(i / 36),
                g: level(i / 6 % 6),
                b: level(i % 6),
            }
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            Rgb { r: v, g: v, b: v }
        }
    }
}

fn dim(c: Rgb) -> Rgb {
    let f = |v: u8| (v as f32 * 0.66) as u8;
    Rgb {
        r: f(c.r),
        g: f(c.g),
        b: f(c.b),
    }
}

/// A cell color: the terminal's OSC-set palette first, then the theme's.
pub(crate) fn resolve(color: Color, colors: &Colors, p: &Palette) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,
        Color::Indexed(i) => colors[i as usize].unwrap_or_else(|| indexed(i, p)),
        Color::Named(n) => colors[n].unwrap_or_else(|| match n as usize {
            i @ 0..16 => indexed(i as u8, p),
            i @ 259..267 => dim(indexed((i - 259) as u8, p)),
            _ if n == NamedColor::Background => p.background,
            _ if n == NamedColor::DimForeground => dim(p.foreground),
            _ => p.foreground,
        }),
    }
}

/// Answer to an OSC 4/10/11 color query: 0..=255 palette, 256 foreground, 257 background.
pub(crate) fn query(index: usize, p: &Palette) -> Rgb {
    match index {
        0..=255 => indexed(index as u8, p),
        257 => p.background,
        _ => p.foreground,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_matches_xterm() {
        let c = Colors::default();
        let p = Palette::default();
        assert_eq!(
            resolve(Color::Indexed(16), &c, &p),
            Rgb { r: 0, g: 0, b: 0 }
        );
        assert_eq!(
            resolve(Color::Indexed(196), &c, &p),
            Rgb { r: 255, g: 0, b: 0 }
        );
        assert_eq!(
            resolve(Color::Indexed(255), &c, &p),
            Rgb {
                r: 238,
                g: 238,
                b: 238
            }
        );
        assert_eq!(
            resolve(Color::Named(NamedColor::Red), &c, &p),
            hex(0xac4242)
        );
        assert_eq!(
            resolve(Color::Named(NamedColor::Background), &c, &p),
            p.background
        );
    }

    #[test]
    fn resolve_and_query_use_the_given_palette() {
        let gray = Rgb { r: 9, g: 9, b: 9 };
        let p = Palette {
            foreground: Rgb { r: 1, g: 2, b: 3 },
            background: Rgb { r: 4, g: 5, b: 6 },
            ansi: [gray; 16],
        };
        let c = Colors::default();
        assert_eq!(resolve(Color::Named(NamedColor::Red), &c, &p), gray);
        assert_eq!(
            resolve(Color::Named(NamedColor::Foreground), &c, &p),
            p.foreground
        );
        assert_eq!(
            resolve(Color::Named(NamedColor::Background), &c, &p),
            p.background
        );
        assert_eq!(query(1, &p), gray);
        assert_eq!(query(256, &p), p.foreground);
        assert_eq!(query(257, &p), p.background);
    }
}
