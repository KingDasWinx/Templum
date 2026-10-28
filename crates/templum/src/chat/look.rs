//! The chat's look (docs/specs/templum-agent-look.md#the-scale): the sizes the panel, the composer
//! and the rows share, and the little maths that decides where a row sits.

use std::time::{Duration, Instant};

use gpui_kit::Hsla;
use templum_agent::Row;

/// The card's border.
pub(crate) const BORDER: f32 = 1.0;
/// Between the card's border and the column.
pub(crate) const GUTTER: f32 = 16.0;
/// The column never gets wider than this, in full screen.
pub(crate) const COLUMN_MAX: f32 = 720.0;
pub(crate) const CARD_RADIUS: f32 = 16.0;
/// A row arriving while the chat is open, or a tool row's details opening, fades in this long.
pub(crate) const FADE: Duration = Duration::from_millis(160);
/// How much of the reading color the code fill takes from the surface.
const CODE_SHARE: f32 = 0.06;

/// The code fill: the surface mixed a little toward the reading color, opaque.
pub(crate) fn code_fill(surface: Hsla, fg: Hsla) -> Hsla {
    surface.blend(fg.opacity(CODE_SHARE))
}

/// The width the rows, the composer and a pinned approval share, on a card `card_w` wide: the
/// card less its border and gutters, at most `COLUMN_MAX`.
pub(crate) fn column(card_w: f32) -> f32 {
    (card_w - 2.0 * (BORDER + GUTTER)).clamp(0.0, COLUMN_MAX)
}

/// Whether row `n` still fades in: it arrived (`arrival`: the first row of the last arrival, and
/// when) less than `FADE` ago.
pub(crate) fn fades(n: usize, arrival: Option<(usize, Instant)>, now: Instant) -> bool {
    arrival.is_some_and(|(from, at)| n >= from && now.saturating_duration_since(at) < FADE)
}

/// What a row belongs to, for the space above it.
#[derive(PartialEq)]
enum Kind {
    Person,
    Text,
    Log,
    Notice,
    Meta,
    Failure,
}

fn kind(row: &Row) -> Kind {
    match row {
        Row::Person { .. } => Kind::Person,
        Row::Text { .. } => Kind::Text,
        Row::Action { .. } | Row::Reads { .. } | Row::Approval { .. } | Row::Thinking => Kind::Log,
        Row::Notice { .. } => Kind::Notice,
        Row::End { .. } | Row::Note { .. } => Kind::Meta,
        Row::Failure { .. } => Kind::Failure,
    }
}

/// The space above `row`, which follows `prev`: a turn's tool rows stack as one log, its time and
/// cost hug its answer, and each message of the person starts a new block.
pub(crate) fn gap_above(prev: Option<&Row>, row: &Row) -> f32 {
    let Some(prev) = prev else { return 0.0 };
    match (kind(prev), kind(row)) {
        (_, Kind::Person) => 20.0,
        (Kind::Log, Kind::Log) | (Kind::Notice, Kind::Notice) => 2.0,
        (_, Kind::Meta) => 6.0,
        _ => 12.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_column_is_the_card_less_its_border_and_gutters_up_to_720() {
        assert_eq!(column(640.0), 606.0);
        assert_eq!(column(1568.0), 720.0);
        assert_eq!(column(300.0), 266.0);
        assert_eq!(column(20.0), 0.0);
    }

    #[test]
    fn the_code_fill_is_opaque_and_near_the_surface() {
        let fill = code_fill(
            gpui_kit::hsla(0.0, 0.0, 0.0, 1.0),
            gpui_kit::hsla(0.0, 0.0, 1.0, 1.0),
        );
        assert_eq!(fill.a, 1.0);
        assert!((fill.l - 0.06).abs() < 0.01, "{fill:?}");
    }

    #[test]
    fn only_rows_that_arrived_fade_and_only_for_a_moment() {
        let t0 = Instant::now();
        assert!(!fades(3, None, t0));
        assert!(!fades(2, Some((3, t0)), t0));
        assert!(fades(3, Some((3, t0)), t0));
        assert!(fades(5, Some((3, t0)), t0 + FADE / 2));
        assert!(!fades(3, Some((3, t0)), t0 + FADE));
    }

    #[test]
    fn rows_sit_by_kind() {
        use templum_agent::{ActionState, NoticeLevel};
        let person = Row::Person {
            text: "hi".into(),
            queued: false,
        };
        let text = Row::Text { text: "a".into() };
        let action = Row::Action {
            id: "1".into(),
            tool: "show".into(),
            input: serde_json::Value::Null,
            state: ActionState::Done,
            said: String::new(),
            target: None,
        };
        let reads = Row::Reads { count: 2 };
        let notice = Row::Notice {
            text: "n".into(),
            level: NoticeLevel::Done,
            target: None,
        };
        let end = Row::End {
            ms: None,
            cost_usd: None,
        };
        let note = Row::Note { text: "x".into() };
        assert_eq!(gap_above(None, &person), 0.0);
        assert_eq!(gap_above(Some(&text), &person), 20.0);
        assert_eq!(gap_above(Some(&person), &text), 12.0);
        assert_eq!(gap_above(Some(&action), &reads), 2.0);
        assert_eq!(gap_above(Some(&reads), &Row::Thinking), 2.0);
        assert_eq!(gap_above(Some(&notice), &notice), 2.0);
        assert_eq!(gap_above(Some(&text), &action), 12.0);
        assert_eq!(gap_above(Some(&action), &notice), 12.0);
        assert_eq!(gap_above(Some(&text), &end), 6.0);
        assert_eq!(gap_above(Some(&end), &note), 6.0);
    }
}
