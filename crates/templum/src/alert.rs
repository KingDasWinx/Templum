//! News from a terminal nobody is looking at: a long command ended, or it asked for attention (a
//! bell, or a notification from an agent). Shown as a dot on its block and its workspace, and as a
//! notification.

use std::time::Duration;

use gpui_kit::component::notification::NotificationType;
use templum_terminal::{Outcome, Status};

/// Commands that end sooner aren't news: the user just ran them.
pub(crate) const LONG: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Alert {
    /// A command that ran at least [`LONG`] ended.
    Finished(Outcome),
    /// A bell (no text), or a notification.
    Attention(Option<String>),
}

/// What changed in `status` since the counts in `heard` (finished, attention).
pub(crate) fn news(status: &Status, (finished, attention): (u64, u64)) -> Option<Alert> {
    if status.attention > attention {
        return Some(Alert::Attention(status.message.clone()));
    }
    match status.last {
        Some(outcome) if status.finished > finished && outcome.took >= LONG => {
            Some(Alert::Finished(outcome))
        }
        _ => None,
    }
}

impl Alert {
    pub(crate) fn failed(&self) -> bool {
        matches!(self, Alert::Finished(o) if failed(o))
    }

    pub(crate) fn text(&self) -> String {
        match self {
            Alert::Finished(o) => finished(o),
            Alert::Attention(Some(message)) => message.clone(),
            Alert::Attention(None) => "Needs your attention".to_string(),
        }
    }

    pub(crate) fn kind(&self) -> NotificationType {
        match self {
            Alert::Attention(_) => NotificationType::Warning,
            Alert::Finished(o) if failed(o) => NotificationType::Error,
            Alert::Finished(_) => NotificationType::Success,
        }
    }
}

fn failed(o: &Outcome) -> bool {
    o.code.is_some_and(|c| c != 0)
}

/// "Finished after 12 s", "Failed (exit 2) after 3 min".
pub(crate) fn finished(o: &Outcome) -> String {
    match o.code {
        Some(code) if code != 0 => format!("Failed (exit {code}) after {}", took(o.took)),
        _ => format!("Finished after {}", took(o.took)),
    }
}

/// "12 s", "3 min", "1 h 5 min".
pub(crate) fn took(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min", s / 60),
        _ if s % 3600 / 60 == 0 => format!("{} h", s / 3600),
        _ => format!("{} h {} min", s / 3600, s % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ended(code: Option<i32>, secs: u64, finished: u64) -> Status {
        Status {
            finished,
            last: Some(Outcome {
                code,
                took: Duration::from_secs(secs),
            }),
            ..Status::default()
        }
    }

    #[test]
    fn a_long_command_ending_is_news_once() {
        let status = ended(Some(0), 7, 1);
        assert_eq!(
            news(&status, (0, 0)),
            Some(Alert::Finished(status.last.unwrap()))
        );
        assert_eq!(news(&status, (1, 0)), None, "already heard");
        assert_eq!(news(&ended(Some(0), 1, 1), (0, 0)), None, "too short");
    }

    #[test]
    fn a_call_for_attention_is_news_even_after_a_command() {
        let status = Status {
            attention: 2,
            message: Some("Claude needs your permission".into()),
            ..ended(Some(1), 60, 3)
        };
        assert_eq!(
            news(&status, (3, 1)),
            Some(Alert::Attention(Some(
                "Claude needs your permission".into()
            )))
        );
    }

    #[test]
    fn texts() {
        let o = |code, secs| Outcome {
            code,
            took: Duration::from_secs(secs),
        };
        assert_eq!(
            Alert::Finished(o(Some(0), 12)).text(),
            "Finished after 12 s"
        );
        assert_eq!(Alert::Finished(o(None, 125)).text(), "Finished after 2 min");
        assert_eq!(
            Alert::Finished(o(Some(2), 3700)).text(),
            "Failed (exit 2) after 1 h 1 min"
        );
        assert_eq!(Alert::Attention(None).text(), "Needs your attention");
        assert_eq!(took(Duration::from_secs(7200)), "2 h");
        assert!(Alert::Finished(o(Some(2), 9)).failed());
        assert!(!Alert::Finished(o(None, 9)).failed());
    }
}
