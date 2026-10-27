//! Level of detail: how much of a block is drawn, from how big its text is on screen (its size at
//! zoom 1 times the zoom), like InDesign's "Greek Type Below", and from how big the block is
//! there. Pure: unit-tested here. See docs/specs/level-of-detail.md, section 1.

use crate::Size;

/// Text smaller than this on screen (pixels) is drawn as bars: about the largest default size at
/// 33% (4.62 px) and the smallest at 50% (6.0 px) apart, so every default block changes level at
/// the same step and none sits on the edge.
pub const GREEK_BELOW: f32 = 5.5;
/// Under this a row is about 3 px and its bars 1 px lines: the block is its card.
pub const CARD_BELOW: f32 = 2.5;
/// The smallest block on screen (pixels, wide and high) that holds its title's label: the 12 px
/// title 4 px in from the card's corner, legible if cut. Every block of its kind's smallest size
/// holds it at 25% (a note, 50 × 35); a smaller block is its shape.
pub const LABEL_FITS: Size = Size { w: 40.0, h: 24.0 };

/// From the most drawn to the least.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Drawn as it is: the text is readable.
    Full,
    /// The text is bars in its own colors, on the letters' rows and columns.
    Silhouette,
    /// The block's card with its title and status, nothing inside.
    Card,
    /// The card alone, with a terminal's status dot: too small on screen for its title.
    Shape,
}

/// The level of text `text` units high at zoom 1, drawn at `zoom`.
pub fn level(text: f32, zoom: f32) -> Level {
    let on_screen = text * zoom;
    if on_screen >= GREEK_BELOW {
        Level::Full
    } else if on_screen >= CARD_BELOW {
        Level::Silhouette
    } else {
        Level::Card
    }
}

/// The level of a block `size` world units big whose text is `text` units high, drawn at `zoom`:
/// its shape when it can't hold its label there, else its text's [`level`].
pub fn block_level(text: f32, size: Size, zoom: f32) -> Level {
    if size.w * zoom < LABEL_FITS.w || size.h * zoom < LABEL_FITS.h {
        Level::Shape
    } else {
        level(text, zoom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ZOOM_STEPS;

    /// A terminal (14), an editor, a note or a kanban block (13), a Git block or a title (12).
    const DEFAULTS: [f32; 3] = [12.0, 13.0, 14.0];

    #[test]
    fn default_sizes_are_bars_at_25_and_33_and_letters_from_50() {
        for text in DEFAULTS {
            for zoom in [0.25, 0.33] {
                assert_eq!(level(text, zoom), Level::Silhouette, "{text} at {zoom}");
            }
            for zoom in ZOOM_STEPS.into_iter().filter(|&z| z >= 0.5) {
                assert_eq!(level(text, zoom), Level::Full, "{text} at {zoom}");
            }
        }
    }

    /// Float error can't flip a level: no step puts a default size within 0.25 px of a threshold.
    #[test]
    fn no_step_puts_a_default_size_near_a_threshold() {
        for text in DEFAULTS {
            for zoom in ZOOM_STEPS {
                for threshold in [GREEK_BELOW, CARD_BELOW] {
                    let off = (text * zoom - threshold).abs();
                    assert!(off >= 0.25, "{text} at {zoom}: {off} px from {threshold}");
                }
            }
        }
    }

    /// Under 25% no default text is drawn: every block is its card, or its shape.
    #[test]
    fn default_sizes_are_cards_under_25() {
        for text in DEFAULTS {
            for zoom in ZOOM_STEPS.into_iter().filter(|&z| z < 0.25) {
                assert_eq!(level(text, zoom), Level::Card, "{text} at {zoom}");
            }
        }
    }

    #[test]
    fn a_block_too_small_for_its_label_is_its_shape() {
        // The smallest note (200 × 140) holds its label at 25%: 50 × 35 px.
        let note = Size::new(200.0, 140.0);
        assert_eq!(block_level(13.0, note, 0.25), Level::Silhouette);
        assert_eq!(block_level(13.0, note, 0.16), Level::Shape, "32 × 22 px");
        // A default note (360 × 280): its label down to 12%, its shape from 8%.
        let note = Size::new(360.0, 280.0);
        assert_eq!(block_level(13.0, note, 0.12), Level::Card, "43 × 34 px");
        assert_eq!(block_level(13.0, note, 0.08), Level::Shape, "29 × 22 px");
        // Too narrow, or too low.
        assert_eq!(
            block_level(13.0, Size::new(390.0, 900.0), 0.1),
            Level::Shape
        );
        assert_eq!(
            block_level(13.0, Size::new(900.0, 230.0), 0.1),
            Level::Shape
        );
        // Big letters on a small block: still too small for the label.
        assert_eq!(
            block_level(72.0, Size::new(300.0, 200.0), 0.1),
            Level::Shape
        );
        assert_eq!(block_level(72.0, Size::new(300.0, 200.0), 1.0), Level::Full);
        assert!(Level::Full < Level::Silhouette && Level::Card < Level::Shape);
    }

    #[test]
    fn small_fonts_turn_to_cards_and_large_ones_keep_their_letters() {
        assert_eq!(level(9.0, 0.25), Level::Card, "2.25 px");
        assert_eq!(level(10.0, 0.25), Level::Silhouette, "2.5 px");
        assert_eq!(level(17.0, 0.33), Level::Full, "5.61 px");
    }
}
