//! The times of an automation (docs/specs/automations.md#time-batch-1): the next time a `When`
//! falls due, cron's five fields, the daylight-saving rule and the words. Pure: the clock is a
//! `TimeZone` and the dates are arguments, so the tests fix both.

#[cfg(test)]
use chrono::Timelike;
use chrono::{DateTime, Datelike, Days, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

use super::When;

/// The most minutes an Every runs apart: 7 days.
pub(crate) const EVERY_MOST: u32 = 7 * 24 * 60;
/// A calendar time further off than this many days is never.
pub(crate) const HORIZON_DAYS: u64 = 5 * 366;

const FIVE_FIELDS: &str = "Use five fields: minute hour day month weekday.";
const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
/// Sunday first, as cron numbers them.
const CRON_DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
/// Monday first, as the weekdays of a `When` are numbered.
const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// A cron expression's five fields, each as the set of values it accepts.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Cron {
    minutes: u64,
    hours: u32,
    days: u32,
    months: u16,
    /// Bit 0 is Sunday.
    weekdays: u8,
    /// A day field that starts with `*` doesn't restrict: when either does not, both must match.
    days_any: bool,
    weekdays_any: bool,
}

impl Cron {
    /// Vixie cron's five fields: `*`, `n`, `a-b`, `*/s`, `a-b/s`, `a/s` and comma lists, `jan`–`dec`
    /// and `sun`–`sat` in any case. Err: what is wrong, in the editor's words.
    pub(crate) fn parse(expr: &str) -> Result<Cron, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        let [minute, hour, day, month, weekday] = fields[..] else {
            return Err(FIVE_FIELDS.to_string());
        };
        let weekdays = field(weekday, "Weekday", 0, 7, &CRON_DAYS, 0)?;
        Ok(Cron {
            minutes: field(minute, "Minute", 0, 59, &[], 0)?,
            hours: field(hour, "Hour", 0, 23, &[], 0)? as u32,
            days: field(day, "Day", 1, 31, &[], 0)? as u32,
            months: field(month, "Month", 1, 12, &MONTHS, 1)? as u16,
            // 7 is a second name for Sunday.
            weekdays: ((weekdays | weekdays >> 7) & 0x7f) as u8,
            days_any: day.starts_with('*'),
            weekdays_any: weekday.starts_with('*'),
        })
    }

    fn day_matches(&self, date: NaiveDate) -> bool {
        if self.months >> date.month() & 1 == 0 {
            return false;
        }
        let day = self.days >> date.day() & 1 == 1;
        let weekday = self.weekdays >> date.weekday().num_days_from_sunday() & 1 == 1;
        if self.days_any || self.weekdays_any {
            day && weekday
        } else {
            day || weekday
        }
    }

    #[cfg(test)]
    pub(crate) fn matches(&self, at: NaiveDateTime) -> bool {
        self.minutes >> at.minute() & 1 == 1
            && self.hours >> at.hour() & 1 == 1
            && self.day_matches(at.date())
    }

    /// The times of day this expression runs at, on a day it matches.
    fn times(&self) -> Vec<NaiveTime> {
        let hours = (0..24).filter(|h| self.hours >> h & 1 == 1);
        hours
            .flat_map(|h| {
                (0..60)
                    .filter(|m| self.minutes >> m & 1 == 1)
                    .filter_map(move |m| NaiveTime::from_hms_opt(h, m, 0))
            })
            .collect()
    }
}

/// One field as a set of values (bit n for n) between `lo` and `hi`; `names` are the words that
/// stand for `first`, `first + 1`…
fn field(
    text: &str,
    name: &str,
    lo: u32,
    hi: u32,
    names: &[&str],
    first: u32,
) -> Result<u64, String> {
    let number = |word: &str| -> Result<u32, String> {
        let lower = word.to_ascii_lowercase();
        let value = match names.iter().position(|n| *n == lower) {
            Some(at) => at as u32 + first,
            None if !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit()) => {
                word.parse::<u32>().unwrap_or(u32::MAX)
            }
            None => return Err(format!("{name}: \"{word}\" isn't a number or a name.")),
        };
        if value < lo {
            return Err(format!("{name}: {value} is before {lo}."));
        }
        if value > hi {
            return Err(format!("{name}: {value} is past {hi}."));
        }
        Ok(value)
    };
    let mut set = 0u64;
    for item in text.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => (range, Some(step)),
            None => (item, None),
        };
        let step = match step {
            None => 1,
            Some(step) => match step.parse::<u32>() {
                Ok(0) => return Err(format!("{name}: a step of 0 never moves.")),
                Ok(step) if step > hi => {
                    return Err(format!("{name}: a step of {step} is past {hi}."));
                }
                Ok(step) => step,
                Err(_) => return Err(format!("{name}: \"{step}\" isn't a number or a name.")),
            },
        };
        let (from, to) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            let (a, b) = (number(a)?, number(b)?);
            if a > b {
                return Err(format!("{name}: {range} runs backwards."));
            }
            (a, b)
        } else {
            let from = number(range)?;
            // `a/s` runs to the field's end.
            (from, if item.contains('/') { hi } else { from })
        };
        let mut value = from;
        while value <= to {
            set |= 1 << value;
            value += step;
        }
    }
    Ok(set)
}

/// `at` on `tz`'s clock: a time a daylight-saving change skips is the hour after, one it repeats
/// the first.
pub(crate) fn resolve<Tz: TimeZone>(tz: &Tz, at: NaiveDateTime) -> Option<DateTime<Tz>> {
    tz.from_local_datetime(&at).earliest().or_else(|| {
        tz.from_local_datetime(&(at + Duration::hours(1)))
            .earliest()
    })
}

/// The last day of `date`'s month.
fn last_day(date: NaiveDate) -> u32 {
    let (year, month) = (date.year(), date.month());
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next.and_then(|next| next.pred_opt())
        .map_or(28, |d| d.day())
}

/// The first time strictly after `after` that `when` falls due; `since` is where Every counts
/// from. None: never.
pub(crate) fn next_after<Tz: TimeZone>(
    when: &When,
    since: &DateTime<Tz>,
    after: &DateTime<Tz>,
) -> Option<DateTime<Tz>> {
    let tz = after.timezone();
    // The first time of the first day, on, that `resolve`s after `after`.
    let walk = |times_on: &dyn Fn(NaiveDate) -> Vec<NaiveTime>| {
        let first = after.date_naive();
        (0..=HORIZON_DAYS).find_map(|n| {
            let date = first.checked_add_days(Days::new(n))?;
            times_on(date)
                .into_iter()
                .find_map(|t| resolve(&tz, date.and_time(t)).filter(|r| r > after))
        })
    };
    match when {
        When::Once { at } => resolve(&tz, *at).filter(|r| r > after),
        // Nothing divides by a period of 0: it never comes (`when_problem` refuses it).
        When::Every { minutes: 0 } => None,
        When::Every { minutes } => {
            let period = i64::from(*minutes) * 60_000;
            let behind = (after.clone() - since.clone()).num_milliseconds();
            let k = if behind < 0 { 0 } else { behind / period + 1 };
            Some(since.clone() + Duration::milliseconds(k * period))
        }
        When::Daily { at } => walk(&|_| vec![*at]),
        When::Weekly { days, at } => walk(&|date| {
            let today = u8::try_from(date.weekday().number_from_monday()).unwrap_or(0);
            if days.contains(&today) {
                vec![*at]
            } else {
                vec![]
            }
        }),
        When::Monthly { days, at } => walk(&|date| {
            let (day, last) = (date.day(), last_day(date));
            let chosen = |d: &u8| u32::from(*d) == day || (day == last && u32::from(*d) > last);
            if days.iter().any(chosen) {
                vec![*at]
            } else {
                vec![]
            }
        }),
        // An event has no time: it is heard.
        When::Agent { .. } | When::Command { .. } => None,
        When::Cron { expr } => {
            let cron = Cron::parse(expr).ok()?;
            walk(&|date| {
                if cron.day_matches(date) {
                    cron.times()
                } else {
                    vec![]
                }
            })
        }
    }
}

/// What stops `when` from being saved at `now` (local), in the editor's words; None: nothing.
pub(crate) fn when_problem(when: &When, now: NaiveDateTime) -> Option<String> {
    // The checks of an event are the Workspace's: what it hears must be open.
    if when.is_event() {
        return None;
    }
    match when {
        When::Weekly { days, .. } if days.is_empty() => {
            return Some("Choose at least one weekday.".into());
        }
        When::Monthly { days, .. } if days.is_empty() => {
            return Some("Choose at least one day.".into());
        }
        When::Every { minutes } if !(1..=EVERY_MOST).contains(minutes) => {
            return Some("Every runs from 1 minute to 7 days.".into());
        }
        When::Cron { expr } => {
            if let Err(why) = Cron::parse(expr) {
                return Some(why);
            }
        }
        When::Once { at } if *at <= now => return Some("That time has passed.".into()),
        _ => {}
    }
    // Only the calendar kinds can find nothing; a zone with no daylight saving suffices to ask.
    let now = chrono::Utc.from_utc_datetime(&now);
    next_after(when, &now, &now)
        .is_none()
        .then(|| "It would never run.".into())
}

/// "1st", "2nd", "3rd", "4th", "11th", "21st".
fn ordinal(n: u8) -> String {
    let suffix = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// "a", "a and b", "a, b and c" (`and` is the last joiner: "or" for "a, b or c").
pub(super) fn list(items: &[String], and: &str) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} {and} {last}", init.join(", ")),
    }
}

/// `when` as the list and the preview say it: "Every weekday at 09:00". `today` decides whether a
/// Once names its year.
pub(crate) fn when_words(when: &When, today: NaiveDate) -> String {
    match when {
        When::Once { at } => {
            let day = if at.year() == today.year() {
                "%-d %b"
            } else {
                "%-d %b %Y"
            };
            format!("On {} at {}", at.format(day), at.format("%H:%M"))
        }
        When::Every { minutes: 1 } => "Every minute".into(),
        When::Every { minutes: 60 } => "Every hour".into(),
        When::Every { minutes } if minutes % 60 == 0 => format!("Every {} hours", minutes / 60),
        When::Every { minutes } => format!("Every {minutes} minutes"),
        When::Daily { at } => format!("Every day at {}", at.format("%H:%M")),
        When::Weekly { days, at } => {
            let mut days = days.clone();
            days.sort_unstable();
            days.dedup();
            let which = match days[..] {
                [1, 2, 3, 4, 5] => "weekday".to_string(),
                [1, 2, 3, 4, 5, 6, 7] => "day".to_string(),
                _ => {
                    let names: Vec<&str> = (days.iter())
                        .filter_map(|d| DAYS.get(usize::from(*d).wrapping_sub(1)).copied())
                        .collect();
                    match names[..] {
                        [] => "no weekday".to_string(),
                        [one] => one.to_string(),
                        _ => list(
                            &names.iter().map(|n| n[..3].to_string()).collect::<Vec<_>>(),
                            "and",
                        ),
                    }
                }
            };
            format!("Every {which} at {}", at.format("%H:%M"))
        }
        When::Monthly { days, at } => {
            let mut days = days.clone();
            days.sort_unstable();
            days.dedup();
            let days: Vec<String> = days.into_iter().map(ordinal).collect();
            format!(
                "On the {} of each month at {}",
                list(&days, "and"),
                at.format("%H:%M")
            )
        }
        When::Cron { expr } => format!("At cron \"{expr}\""),
        When::Agent { .. } | When::Command { .. } => {
            super::event_words(when, &|u| u.to_string(), &|k| k.to_string(), None)
                .unwrap_or_default()
        }
    }
}

/// `t` as a row says when: "17:00" today, "Tomorrow 09:00", "Yesterday 16:00", "Mon 09:00" within
/// a week, "3 Nov 09:00", "3 Jan 2027 09:00" in another year.
pub(crate) fn time_words(t: NaiveDateTime, now: NaiveDateTime) -> String {
    let apart = (t.date() - now.date()).num_days();
    let clock = t.format("%H:%M");
    match apart {
        0 => clock.to_string(),
        1 => format!("Tomorrow {clock}"),
        -1 => format!("Yesterday {clock}"),
        -6..=6 => format!("{} {clock}", t.format("%a")),
        _ if t.year() == now.year() => format!("{} {clock}", t.format("%-d %b")),
        _ => format!("{} {clock}", t.format("%-d %b %Y")),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, MappedLocalTime, Utc};

    use super::*;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap()
    }

    fn utc(s: &str) -> DateTime<Utc> {
        Utc.from_utc_datetime(&dt(s))
    }

    fn at(s: &str) -> NaiveTime {
        NaiveTime::parse_from_str(s, "%H:%M").unwrap()
    }

    fn cron_matches(expr: &str, when: &str) -> bool {
        Cron::parse(expr).unwrap().matches(dt(when))
    }

    /// The test zone, a European-style daylight saving (UTC+1, UTC+2 from 2027-03-28 01:00 UTC to
    /// 2027-10-31 01:00 UTC).
    #[derive(Clone, Copy, Debug)]
    struct Dst;

    impl Dst {
        fn summer(utc: &NaiveDateTime) -> bool {
            let (from, to) = (dt("2027-03-28T01:00"), dt("2027-10-31T01:00"));
            *utc >= from && *utc < to
        }

        fn local(s: &str) -> DateTime<Dst> {
            resolve(&Dst, dt(s)).unwrap()
        }
    }

    impl TimeZone for Dst {
        type Offset = FixedOffset;
        fn from_offset(_: &FixedOffset) -> Self {
            Dst
        }
        fn offset_from_local_date(&self, _: &NaiveDate) -> MappedLocalTime<FixedOffset> {
            unimplemented!()
        }
        fn offset_from_local_datetime(
            &self,
            local: &NaiveDateTime,
        ) -> MappedLocalTime<FixedOffset> {
            let (one, two) = (
                FixedOffset::east_opt(3600).unwrap(),
                FixedOffset::east_opt(7200).unwrap(),
            );
            let fits = |o: FixedOffset| self.offset_from_utc_datetime(&(*local - o)) == o;
            match (fits(one), fits(two)) {
                (true, true) => MappedLocalTime::Ambiguous(two, one), // summer's 02:30 came first
                (true, false) => MappedLocalTime::Single(one),
                (false, true) => MappedLocalTime::Single(two),
                (false, false) => MappedLocalTime::None,
            }
        }
        fn offset_from_utc_date(&self, _: &NaiveDate) -> FixedOffset {
            unimplemented!()
        }
        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
            FixedOffset::east_opt(if Dst::summer(utc) { 7200 } else { 3600 }).unwrap()
        }
    }

    #[test]
    fn cron_reads_every_field_form() {
        let weekdays = "*/15 9-17 * * mon-fri";
        assert!(cron_matches(weekdays, "2026-10-26T09:00"));
        assert!(cron_matches(weekdays, "2026-10-26T09:15"));
        assert!(cron_matches(weekdays, "2026-10-30T17:45"));
        assert!(!cron_matches(weekdays, "2026-10-26T08:45"));
        assert!(!cron_matches(weekdays, "2026-10-26T18:00"));
        assert!(!cron_matches(weekdays, "2026-10-26T09:10"));
        assert!(!cron_matches(weekdays, "2026-10-24T09:00"), "a Saturday");
        let twice = "0 0 1,15 * *";
        assert!(cron_matches(twice, "2026-11-01T00:00") && cron_matches(twice, "2026-11-15T00:00"));
        assert!(!cron_matches(twice, "2026-11-02T00:00"));
        // Sunday, three ways.
        for sunday in ["5 4 * * 0", "5 4 * * 7", "5 4 * * SUN"] {
            assert!(cron_matches(sunday, "2026-10-25T04:05"), "{sunday}");
            assert!(!cron_matches(sunday, "2026-10-26T04:05"), "{sunday}");
        }
        let months = "0 9 * jan,JUL *";
        assert!(
            cron_matches(months, "2027-01-05T09:00") && cron_matches(months, "2027-07-05T09:00")
        );
        assert!(!cron_matches(months, "2027-02-05T09:00"));
        let stepped = "0 12 * * 1-5/2";
        assert!(cron_matches(stepped, "2026-10-26T12:00"), "Monday");
        assert!(!cron_matches(stepped, "2026-10-27T12:00"), "Tuesday");
        assert!(cron_matches(stepped, "2026-10-28T12:00"), "Wednesday");
        assert!(cron_matches(stepped, "2026-10-30T12:00"), "Friday");
        let from = "30 6/4 * * *";
        for hour in ["06", "10", "14", "18", "22"] {
            assert!(
                cron_matches(from, &format!("2026-10-24T{hour}:30")),
                "{hour}"
            );
        }
        assert!(!cron_matches(from, "2026-10-24T08:30"));
    }

    #[test]
    fn cron_says_what_is_wrong() {
        let wrong = |expr| Cron::parse(expr).err().unwrap();
        assert_eq!(
            wrong("* * *"),
            "Use five fields: minute hour day month weekday."
        );
        assert_eq!(
            wrong("@daily"),
            "Use five fields: minute hour day month weekday."
        );
        assert_eq!(wrong("0 25 * * *"), "Hour: 25 is past 23.");
        assert_eq!(wrong("0 0 0 * *"), "Day: 0 is before 1.");
        assert_eq!(
            wrong("x * * * *"),
            "Minute: \"x\" isn't a number or a name."
        );
        assert_eq!(wrong("*/0 * * * *"), "Minute: a step of 0 never moves.");
        assert_eq!(wrong("0 9 * * 5-1"), "Weekday: 5-1 runs backwards.");
        assert_eq!(
            wrong("1/4294967295 * * * *"),
            "Minute: a step of 4294967295 is past 59."
        );
        assert_eq!(wrong("* */24 * * *"), "Hour: a step of 24 is past 23.");
    }

    #[test]
    fn cron_day_fields_match_either_when_both_are_set() {
        let both = "0 9 13 * fri";
        assert!(cron_matches(both, "2026-11-06T09:00"), "a Friday");
        assert!(
            cron_matches(both, "2026-11-13T09:00"),
            "a Friday and the 13th"
        );
        assert!(cron_matches(both, "2026-12-13T09:00"), "a Sunday, the 13th");
        assert!(!cron_matches(both, "2026-11-09T09:00"), "a Monday, the 9th");
        // One of them left open: both must match.
        assert!(!cron_matches("0 9 13 * *", "2026-11-06T09:00"));
        assert!(!cron_matches("0 9 * * fri", "2026-11-13T10:00"));
    }

    #[test]
    fn next_times_of_each_kind() {
        // Saturday, 12:00; Every counts from 10:13.
        let (after, since) = (utc("2026-10-24T12:00"), utc("2026-10-24T10:13"));
        let next = |when: When| next_after(&when, &since, &after);
        let daily = |time| When::Daily { at: at(time) };
        assert_eq!(next(daily("17:00")), Some(utc("2026-10-24T17:00")));
        assert_eq!(next(daily("09:00")), Some(utc("2026-10-25T09:00")));
        assert_eq!(
            next(daily("12:00")),
            Some(utc("2026-10-25T12:00")),
            "strictly after"
        );
        let weekdays = When::Weekly {
            days: vec![1, 2, 3, 4, 5],
            at: at("09:00"),
        };
        assert_eq!(next(weekdays), Some(utc("2026-10-26T09:00")));
        let every = |minutes| When::Every { minutes };
        assert_eq!(next(every(120)), Some(utc("2026-10-24T12:13")));
        assert_eq!(
            next_after(&every(120), &since, &utc("2026-10-24T12:13")),
            Some(utc("2026-10-24T14:13"))
        );
        assert_eq!(
            next_after(&every(120), &since, &utc("2026-10-24T09:00")),
            Some(utc("2026-10-24T10:13"))
        );
        let once = |time| When::Once { at: dt(time) };
        assert_eq!(
            next(once("2026-10-24T18:00")),
            Some(utc("2026-10-24T18:00"))
        );
        assert_eq!(next(once("2026-10-24T11:00")), None);
        let cron = When::Cron {
            expr: "0 9 * * 1-5".into(),
        };
        assert_eq!(next(cron), Some(utc("2026-10-26T09:00")));
    }

    #[test]
    fn a_month_without_the_day_runs_on_its_last() {
        let monthly = |days: &[u8]| When::Monthly {
            days: days.to_vec(),
            at: at("09:00"),
        };
        let next = |when: &When, after: &str| {
            let after = utc(after);
            next_after(when, &after, &after)
        };
        assert_eq!(
            next(&monthly(&[31]), "2027-02-01T00:00"),
            Some(utc("2027-02-28T09:00"))
        );
        assert_eq!(
            next(&monthly(&[31]), "2028-02-01T00:00"),
            Some(utc("2028-02-29T09:00"))
        );
        let both = monthly(&[30, 31]);
        assert_eq!(
            next(&both, "2027-04-01T00:00"),
            Some(utc("2027-04-30T09:00"))
        );
        assert_eq!(
            next(&both, "2027-04-30T09:00"),
            Some(utc("2027-05-30T09:00")),
            "April runs once"
        );
        assert_eq!(
            next(&both, "2027-05-30T09:00"),
            Some(utc("2027-05-31T09:00"))
        );
    }

    #[test]
    fn an_every_of_zero_minutes_is_never() {
        let after = utc("2026-10-24T12:00");
        assert_eq!(
            next_after(&When::Every { minutes: 0 }, &after, &after),
            None
        );
    }

    #[test]
    fn a_time_that_never_comes_is_never() {
        let after = utc("2026-10-24T12:00");
        let cron = |expr: &str| When::Cron { expr: expr.into() };
        assert_eq!(next_after(&cron("0 0 30 2 *"), &after, &after), None);
        assert_eq!(
            next_after(&cron("0 0 29 2 *"), &after, &after),
            Some(utc("2028-02-29T00:00"))
        );
        assert_eq!(next_after(&cron("0 25 * * *"), &after, &after), None);
    }

    #[test]
    fn daylight_saving_is_counted() {
        let next = |when: &When, since: &str, after: &str| {
            next_after(when, &Dst::local(since), &Dst::local(after))
        };
        let daily = When::Daily { at: at("02:30") };
        // The spring night skips 02:00–03:00: the hour after.
        let spring = next(&daily, "2027-03-27T12:00", "2027-03-27T12:00").unwrap();
        assert_eq!(spring, Dst::local("2027-03-28T03:30"));
        // The autumn night repeats 02:00–03:00: the first.
        let autumn = next(&daily, "2027-10-30T12:00", "2027-10-30T12:00").unwrap();
        assert_eq!(autumn.with_timezone(&Utc), utc("2027-10-31T00:30"));
        // Every counts real time: an hour after 01:30 (UTC+1) is 03:30 (UTC+2).
        let hourly = When::Every { minutes: 60 };
        let hour = next(&hourly, "2027-03-28T00:30", "2027-03-28T01:45").unwrap();
        assert_eq!(hour, Dst::local("2027-03-28T03:30"));
    }

    #[test]
    fn problems_in_the_editors_words() {
        let now = dt("2026-10-24T12:00");
        let problem = |when: When| when_problem(&when, now);
        assert_eq!(
            problem(When::Weekly {
                days: vec![],
                at: at("09:00")
            })
            .unwrap(),
            "Choose at least one weekday."
        );
        assert_eq!(
            problem(When::Monthly {
                days: vec![],
                at: at("09:00")
            })
            .unwrap(),
            "Choose at least one day."
        );
        for minutes in [0, 10081] {
            assert_eq!(
                problem(When::Every { minutes }).unwrap(),
                "Every runs from 1 minute to 7 days."
            );
        }
        assert_eq!(
            problem(When::Once {
                at: dt("2026-10-24T11:00")
            })
            .unwrap(),
            "That time has passed."
        );
        assert_eq!(
            problem(When::Cron {
                expr: "0 25 * * *".into()
            })
            .unwrap(),
            "Hour: 25 is past 23."
        );
        assert_eq!(
            problem(When::Cron {
                expr: "0 0 30 2 *".into()
            })
            .unwrap(),
            "It would never run."
        );
        assert_eq!(problem(When::Daily { at: at("09:00") }), None);
        assert_eq!(
            problem(When::Once {
                at: dt("2026-10-24T18:00")
            }),
            None
        );
        assert_eq!(
            problem(When::Every {
                minutes: EVERY_MOST
            }),
            None
        );
    }

    #[test]
    fn an_event_has_no_time() {
        use super::super::{Heard, Who};
        let idle = When::Agent {
            event: Heard::Idle,
            who: Who::Blocks {
                blocks: vec!["abc12".into()],
                all: false,
            },
        };
        assert_eq!(
            next_after(&idle, &utc("2027-01-04T08:00"), &utc("2027-01-04T08:00")),
            None
        );
        assert_eq!(when_problem(&idle, dt("2027-01-04T08:00")), None);
        assert_eq!(
            when_words(&idle, dt("2027-01-04T08:00").date()),
            "When abc12 finishes its turn"
        );
    }

    #[test]
    fn words_of_each_kind() {
        let today = dt("2026-10-24T00:00").date();
        let words = |when: When| when_words(&when, today);
        let weekly = |days: &[u8]| When::Weekly {
            days: days.to_vec(),
            at: at("09:00"),
        };
        assert_eq!(words(When::Daily { at: at("17:00") }), "Every day at 17:00");
        assert_eq!(words(weekly(&[1, 2, 3, 4, 5])), "Every weekday at 09:00");
        assert_eq!(words(weekly(&[1])), "Every Monday at 09:00");
        assert_eq!(words(weekly(&[5, 1, 3])), "Every Mon, Wed and Fri at 09:00");
        assert_eq!(words(weekly(&[1, 2, 3, 4, 5, 6, 7])), "Every day at 09:00");
        let monthly = |days: &[u8]| When::Monthly {
            days: days.to_vec(),
            at: at("09:00"),
        };
        assert_eq!(
            words(monthly(&[1, 15])),
            "On the 1st and 15th of each month at 09:00"
        );
        assert_eq!(
            words(monthly(&[2, 3, 22, 31])),
            "On the 2nd, 3rd, 22nd and 31st of each month at 09:00"
        );
        assert_eq!(
            words(monthly(&[11, 12, 13, 21])),
            "On the 11th, 12th, 13th and 21st of each month at 09:00"
        );
        for (minutes, said) in [
            (1, "Every minute"),
            (10, "Every 10 minutes"),
            (60, "Every hour"),
            (120, "Every 2 hours"),
            (90, "Every 90 minutes"),
        ] {
            assert_eq!(words(When::Every { minutes }), said);
        }
        assert_eq!(
            words(When::Once {
                at: dt("2026-10-24T17:00")
            }),
            "On 24 Oct at 17:00"
        );
        assert_eq!(
            words(When::Once {
                at: dt("2027-01-03T09:00")
            }),
            "On 3 Jan 2027 at 09:00"
        );
        assert_eq!(
            words(When::Cron {
                expr: "0 9 * * 1-5".into()
            }),
            "At cron \"0 9 * * 1-5\""
        );
    }

    #[test]
    fn time_words_say_the_day() {
        // A Saturday.
        let now = dt("2026-10-24T12:00");
        for (time, said) in [
            ("2026-10-24T17:00", "17:00"),
            ("2026-10-25T09:00", "Tomorrow 09:00"),
            ("2026-10-23T16:00", "Yesterday 16:00"),
            ("2026-10-26T09:00", "Mon 09:00"),
            ("2026-10-21T16:00", "Wed 16:00"),
            ("2026-11-03T09:00", "3 Nov 09:00"),
            ("2027-01-03T09:00", "3 Jan 2027 09:00"),
        ] {
            assert_eq!(time_words(dt(time), now), said, "{time}");
        }
    }
}
