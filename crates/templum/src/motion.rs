//! Animations: blocks opening and closing, the settings panel sliding, the camera gliding; timing,
//! easing, and the scale steps blocks are drawn at.

use std::time::{Duration, Instant};

pub(crate) const OPEN: Duration = Duration::from_millis(280);
pub(crate) const CLOSE: Duration = Duration::from_millis(180);
/// The settings panel slides in or out.
pub(crate) const SLIDE: Duration = Duration::from_millis(200);
/// A hovered block lifts, and its hover-only controls fade in.
pub(crate) const HOVER: Duration = Duration::from_millis(120);
/// Switching (or previewing) a theme crossfades the canvas and the chrome.
pub(crate) const THEME: Duration = Duration::from_millis(200);
/// The camera glides to a new zoom (or a fit); eased with `slide`.
pub(crate) const FLIGHT: Duration = Duration::from_millis(150);
/// Two tiles trading places in grid mode slide to them; eased with `slide`.
pub(crate) const TILE: Duration = Duration::from_millis(220);
/// An automatic copy's "Copied" stays this long, its last CLOSE fading out (`copied`).
pub(crate) const COPIED: Duration = Duration::from_millis(1200);
/// A tile held by its title to trade places: smaller and faded, picked up.
pub(crate) const HELD: Look = Look {
    scale: 0.96,
    opacity: 0.45,
};
/// Blocks are scaled by redrawing them at zoom × scale, which rasterizes text at that size:
/// scales snap to steps so only a few extra sizes are ever cached (like the zoom steps).
const SCALE_STEP: f32 = 0.02;

/// How a block is drawn at one moment of an animation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Look {
    /// Around the block's center.
    pub scale: f32,
    pub opacity: f32,
}

impl Look {
    pub const REST: Look = Look {
        scale: 1.0,
        opacity: 1.0,
    };
}

/// 0 at `since`, 1 once `duration` has passed.
pub(crate) fn progress(since: Instant, now: Instant, duration: Duration) -> f32 {
    (now.saturating_duration_since(since).as_secs_f32() / duration.as_secs_f32()).min(1.0)
}

/// How opaque the "Copied" shown at `since` is at `now`: fading in over HOVER, out over the
/// last CLOSE of COPIED; None once it is gone.
pub(crate) fn copied(since: Instant, now: Instant) -> Option<f32> {
    let shown = now.saturating_duration_since(since);
    let left = COPIED.checked_sub(shown).filter(|left| !left.is_zero())?;
    let fade_in = shown.as_secs_f32() / HOVER.as_secs_f32();
    let fade_out = left.as_secs_f32() / CLOSE.as_secs_f32();
    Some(fade_in.min(fade_out).min(1.0))
}

/// Ease-out (cubic): fast at first, settling at the end.
pub(crate) fn slide(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// How much of a panel sliding in since `since` (out, when `closing`) is in view at `now`: 0
/// hidden, 1 all.
pub(crate) fn slid(since: Instant, closing: bool, now: Instant) -> f32 {
    let t = slide(progress(since, now, SLIDE));
    if closing { 1.0 - t } else { t }
}

/// When a slide turned the other way at `now` must have started, to go on from where it is (no
/// jump when turned back mid-slide).
pub(crate) fn turned(since: Instant, closing: bool, now: Instant) -> Instant {
    let shown = slid(since, closing, now);
    let eased = if closing { shown } else { 1.0 - shown };
    // `slide` inverted: the progress that eases to `eased`.
    let progress = 1.0 - (1.0 - eased).cbrt();
    now.checked_sub(SLIDE.mul_f32(progress)).unwrap_or(now)
}

/// Ease-out that goes a little past 1 and settles back ("back out"), peaking near 1.13.
pub(crate) fn back_out(t: f32) -> f32 {
    const C1: f32 = 2.0;
    const C3: f32 = C1 + 1.0;
    let u = t - 1.0;
    1.0 + C3 * u * u * u + C1 * u * u
}

fn snap(scale: f32) -> f32 {
    (scale / SCALE_STEP).round() * SCALE_STEP
}

/// A new block pops in: from 88% past 100% and back, fading in over the first 70%.
pub(crate) fn opening(t: f32) -> Look {
    if t >= 1.0 {
        return Look::REST;
    }
    Look {
        scale: snap(0.88 + 0.12 * back_out(t)),
        opacity: (t / 0.7).min(1.0),
    }
}

/// A closed block shrinks to 92% and fades out, speeding up.
pub(crate) fn closing(t: f32) -> Look {
    let e = t * t;
    Look {
        scale: snap(1.0 - 0.08 * e),
        opacity: 1.0 - e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> impl Iterator<Item = f32> {
        (0..=100).map(|i| i as f32 / 100.0)
    }

    fn on_a_step(scale: f32) -> bool {
        let k = scale / SCALE_STEP;
        (k - k.round()).abs() < 1e-3
    }

    #[test]
    fn progress_runs_from_0_to_1_and_clamps() {
        let t0 = Instant::now();
        assert_eq!(progress(t0, t0, OPEN), 0.0);
        assert_eq!(progress(t0, t0 + OPEN / 2, OPEN), 0.5);
        assert_eq!(progress(t0, t0 + OPEN * 3, OPEN), 1.0);
        // A clock read before `since` (another thread's Instant) is the start.
        assert_eq!(progress(t0 + OPEN, t0, OPEN), 0.0);
    }

    #[test]
    fn back_out_overshoots_a_little_then_lands() {
        assert_eq!(back_out(0.0), 0.0);
        assert!((back_out(1.0) - 1.0).abs() < 1e-6);
        let peak = samples().map(back_out).fold(0.0, f32::max);
        assert!(peak > 1.05 && peak < 1.2, "peak {peak}");
    }

    #[test]
    fn opening_pops_from_88_percent() {
        assert_eq!(
            opening(0.0),
            Look {
                scale: 0.88,
                opacity: 0.0
            }
        );
        assert_eq!(opening(1.0), Look::REST);
        let peak = samples().map(|t| opening(t).scale).fold(0.0, f32::max);
        assert!(peak > 1.0 && peak <= 1.02, "peak {peak}");
    }

    #[test]
    fn closing_shrinks_to_92_percent_and_fades() {
        assert_eq!(closing(0.0), Look::REST);
        let end = closing(1.0);
        assert!(
            (end.scale - 0.92).abs() < 1e-6 && end.opacity == 0.0,
            "{end:?}"
        );
    }

    #[test]
    fn scales_are_drawn_in_steps() {
        for t in samples() {
            let (a, b) = (opening(t).scale, closing(t).scale);
            assert!(on_a_step(a) && on_a_step(b), "t {t}: {a} {b}");
        }
    }

    #[test]
    fn copied_fades_in_stays_then_fades_out() {
        let t0 = Instant::now();
        assert_eq!(copied(t0, t0), Some(0.0));
        assert_eq!(copied(t0, t0 + HOVER), Some(1.0));
        assert_eq!(copied(t0, t0 + COPIED - CLOSE), Some(1.0));
        let fading = copied(t0, t0 + COPIED - CLOSE / 2).unwrap();
        assert!((fading - 0.5).abs() < 1e-3, "{fading}");
        assert_eq!(copied(t0, t0 + COPIED), None);
        assert_eq!(copied(t0, t0 + COPIED * 2), None);
    }

    #[test]
    fn slide_eases_out() {
        assert_eq!(slide(0.0), 0.0);
        assert_eq!(slide(1.0), 1.0);
        assert!(slide(0.5) > 0.8, "most of the way by half time");
    }
}
