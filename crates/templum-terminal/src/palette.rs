//! Colors: Alacritty's defaults and the xterm 256-color palette.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor};

use crate::Rgb;

// Adapted from Alacritty (Apache-2.0): alacritty/src/config/color.rs default colors.
pub const DEFAULT_FG: Rgb = Rgb {
    r: 0xd8,
    g: 0xd8,
    b: 0xd8,
};
pub const DEFAULT_BG: Rgb = Rgb {
    r: 0x18,
    g: 0x18,
    b: 0x18,
};
const ANSI: [u32; 16] = [
    0x181818, 0xac4242, 0x90a959, 0xf4bf75, 0x6a9fb5, 0xaa759f, 0x75b5aa, 0xd8d8d8, // normal
    0x6b6b6b, 0xc55555, 0xaac474, 0xfeca88, 0x82b8c8, 0xc28cb8, 0x93d3c3, 0xf8f8f8, // bright
];

fn hex(v: u32) -> Rgb {
    Rgb {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    }
}

/// xterm 256-color palette: 16 ANSI, a 6x6x6 cube, 24 grays.
fn indexed(i: u8) -> Rgb {
    match i {
        0..16 => hex(ANSI[i as usize]),
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

/// A cell color: the terminal's OSC-set palette first, then our defaults.
pub(crate) fn resolve(color: Color, colors: &Colors) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,
        Color::Indexed(i) => colors[i as usize].unwrap_or_else(|| indexed(i)),
        Color::Named(n) => colors[n].unwrap_or_else(|| match n as usize {
            i @ 0..16 => indexed(i as u8),
            i @ 259..267 => dim(indexed((i - 259) as u8)),
            _ if n == NamedColor::Background => DEFAULT_BG,
            _ if n == NamedColor::DimForeground => dim(DEFAULT_FG),
            _ => DEFAULT_FG,
        }),
    }
}

/// Answer to an OSC 4/10/11 color query: 0..=255 palette, 256 foreground, 257 background.
pub(crate) fn query(index: usize) -> Rgb {
    match index {
        0..=255 => indexed(index as u8),
        257 => DEFAULT_BG,
        _ => DEFAULT_FG,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_matches_xterm() {
        let c = Colors::default();
        assert_eq!(resolve(Color::Indexed(16), &c), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(resolve(Color::Indexed(196), &c), Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(
            resolve(Color::Indexed(255), &c),
            Rgb {
                r: 238,
                g: 238,
                b: 238
            }
        );
        assert_eq!(resolve(Color::Named(NamedColor::Red), &c), hex(0xac4242));
        assert_eq!(
            resolve(Color::Named(NamedColor::Background), &c),
            DEFAULT_BG
        );
        assert_eq!(query(257), DEFAULT_BG);
        assert_eq!(query(256), DEFAULT_FG);
    }
}
