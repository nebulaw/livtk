//! Calendar dates, as the date picker offers and reads them: a month's grid, the quick picks,
//! and a date typed in words. Dates only, never a time or a zone, so no daylight-saving change
//! or midnight can move one. No UI here: the picker draws what this works out.

use jiff::ToSpan;
use jiff::civil::{Date, Weekday};

/// The day a week starts on, as the grid lays its columns out and "end of week" counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeekStart {
    Monday,
    Sunday,
}

impl WeekStart {
    fn first(self) -> Weekday {
        match self {
            WeekStart::Monday => Weekday::Monday,
            WeekStart::Sunday => Weekday::Sunday,
        }
    }

    /// The days' initials, in the grid's column order.
    pub fn initials(self) -> [&'static str; 7] {
        match self {
            WeekStart::Monday => ["M", "T", "W", "T", "F", "S", "S"],
            WeekStart::Sunday => ["S", "M", "T", "W", "T", "F", "S"],
        }
    }
}

/// The dates a pick may fall between, both ends included; either may be open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub min: Option<Date>,
    pub max: Option<Date>,
}

impl Range {
    pub fn contains(self, d: Date) -> bool {
        self.min.is_none_or(|m| d >= m) && self.max.is_none_or(|m| d <= m)
    }
}

/// A month: its first day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Month(Date);

impl Month {
    pub fn of(d: Date) -> Month {
        Month(d.first_of_month())
    }

    pub fn first(self) -> Date {
        self.0
    }

    pub fn last(self) -> Date {
        self.0.last_of_month()
    }

    /// `n` months on (back when negative).
    pub fn plus(self, n: i32) -> Month {
        Month(self.0.saturating_add(n.months()))
    }

    /// "October 2026".
    pub fn title(self) -> String {
        self.0.strftime("%B %Y").to_string()
    }

    /// The month's days laid out by week, `start` first: whole weeks, the days of the months
    /// either side as `None`, so a column is always the same weekday.
    pub fn grid(self, start: WeekStart) -> Vec<[Option<Date>; 7]> {
        let lead = days_from(start.first(), self.0.weekday());
        let mut weeks = Vec::new();
        let mut week = [None; 7];
        let mut col = usize::from(lead);
        let mut d = self.0;
        loop {
            week[col] = Some(d);
            col += 1;
            if col == 7 {
                weeks.push(week);
                week = [None; 7];
                col = 0;
            }
            // A month's last day is never the calendar's, so a next day always follows it.
            match d.tomorrow() {
                Ok(next) if d < self.last() => d = next,
                _ => break,
            }
        }
        if col > 0 {
            weeks.push(week);
        }
        weeks
    }
}

/// Days from weekday `from` on to weekday `to`, 0 to 6.
fn days_from(from: Weekday, to: Weekday) -> u8 {
    u8::try_from((to.to_monday_zero_offset() - from.to_monday_zero_offset()).rem_euclid(7))
        .unwrap_or(0)
}

/// A quick pick: what it says, and the date it is from `today`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quick {
    EndOfWeek,
    EndOfMonth,
    EndOfQuarter,
    EndOfYear,
}

impl Quick {
    /// The ones a target date offers, in order.
    pub const TARGET: [Quick; 3] = [Quick::EndOfWeek, Quick::EndOfMonth, Quick::EndOfQuarter];

    pub fn label(self) -> &'static str {
        match self {
            Quick::EndOfWeek => "End of this week",
            Quick::EndOfMonth => "End of this month",
            Quick::EndOfQuarter => "End of the quarter",
            Quick::EndOfYear => "End of the year",
        }
    }

    pub fn from(self, today: Date, start: WeekStart) -> Date {
        match self {
            // The week's last day, today when it is.
            Quick::EndOfWeek => {
                let last = start.first().previous();
                today.saturating_add(i64::from(days_from(today.weekday(), last)).days())
            }
            Quick::EndOfMonth => today.last_of_month(),
            Quick::EndOfQuarter => {
                let q_end = ((today.month() - 1) / 3 + 1) * 3;
                Date::new(today.year(), q_end, 1).map_or(today, |d| d.last_of_month())
            }
            Quick::EndOfYear => Date::new(today.year(), 12, 31).unwrap_or(today),
        }
    }
}

/// What typed text could not be read as, said back in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unread(pub String);

/// Read `text` as a date, from `today`:
/// - `2026-12-15`, and `12/15` or `15.12` are not guessed at: only ISO for figures;
/// - a month and a day, either order, with or without a year: `Dec 15`, `15 december 2027`;
///   without a year, the next one to come (today included);
/// - `today`, `tomorrow`, `yesterday`;
/// - a weekday, `next friday` or `friday`: the next one after today;
/// - `in 3 days`, `in 2 weeks`, `in 1 month`, `in a year`;
/// - `eow`, `eom`, `eoq`, `eoy` and their spelled-out forms.
pub fn parse(text: &str, today: Date, start: WeekStart) -> Result<Date, Unread> {
    let t = text.trim().to_lowercase();
    let unread = || {
        Unread(format!(
            "Can't read \u{201c}{}\u{201d} as a date",
            text.trim()
        ))
    };
    if t.is_empty() {
        return Err(unread());
    }
    if let Ok(d) = t.parse::<Date>() {
        return Ok(d);
    }
    let quick = match t.as_str() {
        "today" => return Ok(today),
        "tomorrow" => return today.tomorrow().map_err(|_| unread()),
        "yesterday" => return today.yesterday().map_err(|_| unread()),
        "eow" | "end of week" | "end of the week" | "end of this week" => Some(Quick::EndOfWeek),
        "eom" | "end of month" | "end of the month" | "end of this month" => {
            Some(Quick::EndOfMonth)
        }
        "eoq" | "end of quarter" | "end of the quarter" | "end of this quarter" => {
            Some(Quick::EndOfQuarter)
        }
        "eoy" | "end of year" | "end of the year" | "end of this year" => Some(Quick::EndOfYear),
        _ => None,
    };
    if let Some(q) = quick {
        return Ok(q.from(today, start));
    }
    let words: Vec<&str> = t
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .collect();
    // A weekday: the next one after today.
    let day = match words.as_slice() {
        ["next", w] | [w] => weekday(w),
        _ => None,
    };
    if let Some(w) = day {
        let ahead = match days_from(today.weekday(), w) {
            0 => 7,
            n => n,
        };
        return Ok(today.saturating_add(i64::from(ahead).days()));
    }
    // In some days, weeks, months or years.
    if let ["in", n, unit] = words.as_slice() {
        let n: i64 = match *n {
            "a" | "an" | "one" => 1,
            n => n.parse().map_err(|_| unread())?,
        };
        let span = match unit.trim_end_matches('s') {
            "day" => n.days(),
            "week" => n.weeks(),
            "month" => n.months(),
            "year" => n.years(),
            _ => return Err(unread()),
        };
        return today.checked_add(span).map_err(|_| unread());
    }
    // A month and a day, either way round, then perhaps a year.
    let (month, day, year) = match words.as_slice() {
        [a, b] => month_day(a, b).map(|(m, d)| (m, d, None)),
        [a, b, y] => month_day(a, b).map(|(m, d)| (m, d, Some(*y))),
        _ => None,
    }
    .ok_or_else(unread)?;
    match year {
        Some(y) => {
            let y: i16 = y.parse().map_err(|_| unread())?;
            Date::new(y, month, day).map_err(|_| unread())
        }
        // The next one to come: this year's, or next year's once it has gone.
        None => {
            let this = Date::new(today.year(), month, day);
            match this {
                Ok(d) if d >= today => Ok(d),
                _ => {
                    // Feb 29 waits for the next leap year.
                    (1..=8)
                        .find_map(|k| Date::new(today.year() + k, month, day).ok())
                        .ok_or_else(unread)
                }
            }
        }
    }
}

fn month_day(a: &str, b: &str) -> Option<(i8, i8)> {
    let day = |w: &str| -> Option<i8> {
        let n = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        n.parse().ok().filter(|d| (1..=31).contains(d))
    };
    month(a).zip(day(b)).or_else(|| month(b).zip(day(a)))
}

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// A month by its name or its first three letters (and `sept`).
fn month(w: &str) -> Option<i8> {
    let w = w.trim_end_matches('.');
    if w.len() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|m| m.starts_with(w) || (w == "sept" && *m == "september"))
        .and_then(|i| i8::try_from(i + 1).ok())
}

fn weekday(w: &str) -> Option<Weekday> {
    const DAYS: [(&str, Weekday); 7] = [
        ("monday", Weekday::Monday),
        ("tuesday", Weekday::Tuesday),
        ("wednesday", Weekday::Wednesday),
        ("thursday", Weekday::Thursday),
        ("friday", Weekday::Friday),
        ("saturday", Weekday::Saturday),
        ("sunday", Weekday::Sunday),
    ];
    if w.len() < 3 {
        return None;
    }
    DAYS.iter()
        .find(|(name, _)| name.starts_with(w))
        .map(|(_, d)| *d)
}

/// How a date reads on a chip: "Dec 15", with the year when it isn't `today`'s.
pub fn short(d: Date, today: Date) -> String {
    if d.year() == today.year() {
        d.strftime("%b %-d").to_string()
    } else {
        d.strftime("%b %-d, %Y").to_string()
    }
}

/// Where the keys move a highlighted day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Day(i32),
    Week(i32),
    Month(i32),
    WeekStart,
    WeekEnd,
}

/// The day the keys move `from` to, kept within `range`: a step past it stops at its edge.
pub fn step(from: Date, by: Step, start: WeekStart, range: Range) -> Date {
    let to = match by {
        Step::Day(n) => from.saturating_add(i64::from(n).days()),
        Step::Week(n) => from.saturating_add(i64::from(n).weeks()),
        // Jan 31 a month on is Feb's last day, never March.
        Step::Month(n) => from.saturating_add(n.months()),
        Step::WeekStart => {
            from.saturating_sub(i64::from(days_from(start.first(), from.weekday())).days())
        }
        Step::WeekEnd => {
            let last = start.first().previous();
            from.saturating_add(i64::from(days_from(from.weekday(), last)).days())
        }
    };
    let to = range.min.map_or(to, |m| to.max(m));
    range.max.map_or(to, |m| to.min(m))
}
