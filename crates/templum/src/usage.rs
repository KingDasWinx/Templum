//! Agents' session usage and plan limits, as Templum shows them: the title bar's meter, its
//! popover, a terminal's chip (docs/specs/usage.md).

use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, TITLE_BAR_HEIGHT};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, BoxShadow, ClickEvent, Context, Div, FontWeight,
    Hsla, MouseButton, MouseDownEvent, SharedString, TestSupportExt, div, point, px,
};
use templum_usage::{REACHED, Session, Window};

use crate::grid::hsla;
use crate::motion;
use crate::notes;
use crate::title_bar;
use crate::workspace::Workspace;

/// The meter shows its next agent after this many seconds.
pub(crate) const TURN_SECS: u64 = 8;

/// The agents the title bar's meter shows in turn, with their windows for every model, shortest
/// first: those with a limit known that the user didn't hide (Settings ▸ Usage).
pub(crate) fn meter_agents(ws: &Workspace, now: u64) -> Vec<(&str, Vec<&Window>)> {
    ws.limits
        .iter()
        .filter(|(agent, _)| !ws.config.usage.hide.contains(agent))
        .filter_map(|(agent, l)| {
            let mut windows: Vec<&Window> = l
                .windows
                .iter()
                .filter(|w| w.resets_at > now && w.scope.is_none())
                .collect();
            windows.sort_by_key(|w| w.minutes);
            (!windows.is_empty()).then_some((agent.as_str(), windows))
        })
        .collect()
}

/// The longest name the meter shows, in characters.
const NAME_MAX: usize = 12;

/// An agent's name in the title bar's meter, which is narrow: "Claude Code" is "Claude" and
/// "Claude Code 2" is "Claude 2"; any other name is its own, cut when long.
fn meter_name(title: &str) -> String {
    let title = match title.strip_prefix("Claude Code") {
        Some(rest) => format!("Claude{rest}"),
        None => title.to_string(),
    };
    if title.chars().count() <= NAME_MAX {
        return title;
    }
    title.chars().take(NAME_MAX - 1).chain(['…']).collect()
}

/// The title bar's meter: one agent's limits at a time, each with when it resets ("Claude 5h 27%
/// 4h 20m 7d 18% 3d 4h"), the next every few seconds, with a dot per agent. None when none is
/// known.
pub(crate) fn meter(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<AnyElement> {
    let now = notes::now();
    let agents = meter_agents(ws, now);
    if agents.is_empty() {
        return None;
    }
    let turn = ws.meter_turn % agents.len();
    let (agent, windows) = &agents[turn];
    let ui = ws.ui;
    let name = meter_name(&ws.agent_title(agent));
    let content = div()
        .id(SharedString::from(format!("usage-meter-{agent}")))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(name)
        .children(windows.iter().map(|w| {
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(format!(
                    "{} {:.0}%",
                    window_label(w.minutes),
                    w.used_percent
                ))
                .child(bar(
                    w.used_percent,
                    24.0,
                    level(ws, w.used_percent),
                    ui.hairline,
                ))
                .child(
                    div()
                        .text_color(ui.title_dim)
                        .child(resets(w.resets_at, now, true)),
                )
        }))
        .test_support()
        // The next agent fades in.
        .with_animation(
            SharedString::from(format!("usage-meter-fade-{agent}")),
            Animation::new(motion::HOVER),
            |el, t| el.opacity(t),
        );
    let dots = (agents.len() > 1).then(|| {
        div()
            .flex()
            .gap(px(3.0))
            .children((0..agents.len()).map(|i| {
                div().size(px(4.0)).rounded_full().bg(if i == turn {
                    ui.title
                } else {
                    ui.hairline
                })
            }))
    });
    Some(
        title_bar::button("usage-meter", ui)
            .gap(px(6.0))
            .child(
                Icon::new(IconName::Gauge)
                    .size(px(14.0))
                    .text_color(ui.title),
            )
            .child(content)
            .children(dots)
            .on_hover(cx.listener(|ws, hovered: &bool, _, _| ws.meter_hovered = *hovered))
            .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| ws.toggle_usage(cx)))
            .test_support()
            .into_any_element(),
    )
}

/// The usage popover under the meter: each agent's limits, then the sessions open, which show
/// their terminals.
pub(crate) fn popover(ws: &Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let now = notes::now();
    let ui = ws.ui;
    let foreground = hsla(ws.theme.palette.foreground, 1.0);
    let heading = |text: &'static str| {
        div()
            .pt(px(4.0))
            .text_size(px(11.0))
            .text_color(ui.title_dim)
            .child(text)
    };
    let mut list = div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .p(px(12.0))
        .child(heading("Plan limits"));
    let mut any = false;
    for (agent, limits) in &ws.limits {
        let windows: Vec<&Window> = limits
            .windows
            .iter()
            .filter(|w| w.resets_at > now)
            .collect();
        if windows.is_empty() {
            continue;
        }
        any = true;
        list = list.child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .child(ws.agent_title(agent)),
        );
        for w in windows {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_size(px(12.0))
                    .child(
                        div()
                            .w(px(84.0))
                            .truncate()
                            .text_color(ui.title_dim)
                            .child(capitalized(&limit_name(w.minutes, w.scope.as_deref()))),
                    )
                    .child(bar(
                        w.used_percent,
                        64.0,
                        level(ws, w.used_percent),
                        ui.hairline,
                    ))
                    .child(div().w(px(34.0)).child(format!("{:.0}%", w.used_percent)))
                    .child(
                        div()
                            .text_color(ui.title_dim)
                            .child(format!("resets {}", resets(w.resets_at, now, false))),
                    ),
            );
        }
    }
    if !any {
        list = list.child(
            div()
                .text_size(px(12.0))
                .text_color(ui.title_dim)
                .child("None known yet"),
        );
    }
    let mut sessions = Vec::new();
    for space in ws.spaces() {
        let mut found: Vec<_> = space
            .terminals()
            .filter_map(|(id, pane)| {
                let (text, _) = chip(pane.usage.as_ref()?)?;
                let agent = ws.agent_title(pane.seen.agent.as_deref()?);
                let folder = pane
                    .seen
                    .folder
                    .as_ref()
                    .and_then(|f| f.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Some((id, format!("{agent} · {folder}"), text))
            })
            .collect();
        found.sort_by_key(|(id, ..)| *id);
        sessions.extend(
            found
                .into_iter()
                .map(|(id, name, text)| (space.id.clone(), id, name, text)),
        );
    }
    if !sessions.is_empty() {
        list = list.child(heading("Open sessions"));
    }
    for (i, (space, block, name, text)) in sessions.into_iter().enumerate() {
        list = list.child(
            div()
                .id(("usage-session", i))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .h(px(26.0))
                .px(px(6.0))
                .mx(px(-6.0))
                .rounded(px(6.0))
                .text_size(px(12.0))
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .child(div().truncate().child(name))
                .child(div().flex_shrink_0().text_color(ui.title_dim).child(text))
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.show_session(&space, block, window, cx)
                }))
                .test_support(),
        );
    }
    let panel = div()
        .id("usage-popover")
        .absolute()
        .top(TITLE_BAR_HEIGHT + px(4.0))
        .right(px(8.0))
        .w(px(340.0))
        .max_h(px(480.0))
        .overflow_y_scroll()
        .bg(ui.surface)
        .text_color(foreground)
        .border_1()
        .border_color(ui.hairline)
        .rounded(px(10.0))
        .shadow(vec![BoxShadow {
            color: ui.shadow,
            offset: point(px(0.0), px(12.0)),
            blur_radius: px(32.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        // Presses inside the panel don't reach the backdrop.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(list)
        .test_support();
    // A press anywhere outside the panel closes it, and nothing below gets it.
    div()
        .id("usage-backdrop")
        .absolute()
        .inset_0()
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, _: &MouseDownEvent, _, cx| {
                ws.toggle_usage(cx);
                cx.stop_propagation();
            }),
        )
        .child(panel)
        .into_any_element()
}

/// The chip's context part: "42%", or "84k ctx" when the window's size is unknown.
pub(crate) fn context_text(s: &Session) -> Option<String> {
    s.context_percent
        .map(|p| format!("{p:.0}%"))
        .or_else(|| s.context_tokens.map(|t| format!("{} ctx", tokens(t))))
}

/// A chip's tooltip with the effort of the agent after it ("Effort: high"), when its model has one.
pub(crate) fn with_effort(tip: String, effort: Option<&str>) -> String {
    match effort {
        Some(effort) => format!("{tip}\nEffort: {effort}"),
        None => tip,
    }
}

/// A terminal's chip: its text ("42% · $3.10") and its tooltip. None when nothing is known.
pub(crate) fn chip(s: &Session) -> Option<(String, String)> {
    let context = context_text(s);
    let spent = s
        .cost_usd
        .map(|c| format!("${c:.2}"))
        .or_else(|| s.tokens.map(tokens));
    let text = match (context, spent) {
        (Some(c), Some(s)) => format!("{c} · {s}"),
        (Some(one), None) | (None, Some(one)) => one,
        (None, None) => return None,
    };
    let mut tip = Vec::new();
    if let Some(model) = &s.model {
        tip.push(model.clone());
    }
    match (s.context_percent, s.context_tokens) {
        (Some(p), _) => tip.push(format!("Context: {p:.0}% used")),
        (None, Some(t)) => tip.push(format!("Context: {} tokens", tokens(t))),
        _ => {}
    }
    if let Some(t) = s.tokens {
        tip.push(format!("Tokens: {}", tokens(t)));
    }
    if let Some(c) = s.cost_usd {
        tip.push(format!("Cost: ${c:.2} at API prices"));
    }
    Some((text, tip.join("\n")))
}

/// A token count, short: 950, 84k, 1.2M.
fn tokens(n: u64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 1000 => format!("{}k", n / 1000),
        n => n.to_string(),
    }
}

/// How close to its limit a window is, as a color: red when reached, yellow from the warning
/// line, the accent otherwise.
fn level(ws: &Workspace, used: f32) -> Hsla {
    let warn = ws.config.usage.warn;
    if used >= REACHED {
        hsla(ws.theme.palette.ansi[1], 1.0)
    } else if warn > 0 && used >= f32::from(warn) {
        hsla(ws.theme.palette.ansi[3], 1.0)
    } else {
        ws.ui.accent
    }
}

/// A thin bar `width` wide, filled to `percent`.
pub(crate) fn bar(percent: f32, width: f32, color: Hsla, track: Hsla) -> Div {
    div()
        .flex_shrink_0()
        .w(px(width))
        .h(px(4.0))
        .rounded_full()
        .bg(track)
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(color)
                .w(px(width * percent.clamp(0.0, 100.0) / 100.0)),
        )
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// A window's length in a sentence: "5-hour", "weekly", "30-day".
pub(crate) fn window_name(minutes: u32) -> String {
    match minutes {
        10080 => "weekly".to_string(),
        m if m % 1440 == 0 => format!("{}-day", m / 1440),
        m if m % 60 == 0 => format!("{}-hour", m / 60),
        m => format!("{m}-minute"),
    }
}

/// A limit in a sentence: "5-hour", "weekly", "weekly Fable" (one model's).
pub(crate) fn limit_name(minutes: u32, scope: Option<&str>) -> String {
    match scope {
        Some(model) => format!("{} {model}", window_name(minutes)),
        None => window_name(minutes),
    }
}

/// A window's length, short: "5h", "7d".
pub(crate) fn window_label(minutes: u32) -> String {
    match minutes {
        m if m % 1440 == 0 => format!("{}d", m / 1440),
        m if m % 60 == 0 => format!("{}h", m / 60),
        m => format!("{m}m"),
    }
}

/// How long until a limit resets (`at`, seconds since the epoch), at `now`, in at most two
/// units, the largest first: "in 3 days 4 hours", "in 4 hours 20 min", "in 25 min"; `short`:
/// "3d 4h", "4h 20m", "25m".
pub(crate) fn resets(at: u64, now: u64, short: bool) -> String {
    let left = at.saturating_sub(now).div_ceil(60).max(1);
    let (big, small) = match left {
        1440.. => ((left / 1440, "day"), (left % 1440 / 60, "hour")),
        60.. => ((left / 60, "hour"), (left % 60, "min")),
        _ => ((left, "min"), (0, "min")),
    };
    let part = |(n, name): (u64, &str)| match (short, n) {
        (true, _) => format!("{n}{}", &name[..1]),
        (false, 1) => format!("{n} {name}"),
        (false, _) if name == "min" => format!("{n} min"),
        _ => format!("{n} {name}s"),
    };
    let mut text = part(big);
    if small.0 > 0 {
        text = format!("{text} {}", part(small));
    }
    if short { text } else { format!("in {text}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_named_by_their_length() {
        assert_eq!(window_name(300), "5-hour");
        assert_eq!(window_name(10080), "weekly");
        assert_eq!(window_name(43200), "30-day");
        assert_eq!(window_label(300), "5h");
        assert_eq!(window_label(10080), "7d");
    }

    #[test]
    fn the_meter_tells_accounts_apart_by_name() {
        assert_eq!(meter_name("Claude Code"), "Claude");
        assert_eq!(meter_name("Claude Code 2"), "Claude 2");
        assert_eq!(meter_name("Work"), "Work");
        assert_eq!(meter_name("Codex"), "Codex");
        assert_eq!(meter_name("My personal account"), "My personal…");
    }

    #[test]
    fn a_chip_says_the_context_and_the_cost_or_the_tokens() {
        let s = |context_percent, context_tokens, tokens, cost_usd| Session {
            model: None,
            context_percent,
            context_tokens,
            tokens,
            cost_usd,
        };
        let text = |session| chip(&session).map(|(t, _)| t);
        assert_eq!(
            text(s(Some(42.4), None, Some(9), Some(3.1))).as_deref(),
            Some("42% · $3.10")
        );
        assert_eq!(
            text(s(Some(18.0), None, Some(1_234_567), None)).as_deref(),
            Some("18% · 1.2M")
        );
        assert_eq!(
            text(s(None, Some(84_321), None, None)).as_deref(),
            Some("84k ctx")
        );
        assert_eq!(text(s(None, None, None, None)), None);
        let (_, tip) = chip(&s(Some(42.0), None, None, Some(3.1))).unwrap();
        assert!(tip.contains("at API prices"), "{tip}");
    }

    #[test]
    fn a_reset_counts_down_in_two_units() {
        let now = 1_791_990_000;
        let long = |left: u64| resets(now + left, now, false);
        let short = |left: u64| resets(now + left, now, true);
        assert_eq!(long(25 * 60), "in 25 min");
        assert_eq!(long(10), "in 1 min");
        assert_eq!(long(3600), "in 1 hour");
        assert_eq!(long(4 * 3600), "in 4 hours");
        assert_eq!(long(4 * 3600 + 20 * 60), "in 4 hours 20 min");
        assert_eq!(long(86_400 - 30), "in 1 day");
        assert_eq!(long(2 * 86_400), "in 2 days");
        assert_eq!(long(3 * 86_400 + 4 * 3600 + 59 * 60), "in 3 days 4 hours");
        assert_eq!(short(25 * 60), "25m");
        assert_eq!(short(4 * 3600), "4h");
        assert_eq!(short(4 * 3600 + 20 * 60), "4h 20m");
        assert_eq!(short(3 * 86_400 + 4 * 3600), "3d 4h");
        assert_eq!(short(86_400), "1d");
    }

    #[test]
    fn the_effort_ends_the_tooltip() {
        assert_eq!(with_effort("a".into(), Some("high")), "a\nEffort: high");
        assert_eq!(with_effort("a".into(), None), "a");
    }
}
