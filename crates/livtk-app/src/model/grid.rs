//! The activity grid: a year of days as GitHub draws contributions, each day's colour by how
//! much happened in the project, above the activity feed. Counts come from one grouped query
//! (`Workspace::activity_by_day`); here they are filtered by who and what, laid out in weeks
//! (Monday first), given levels, and summed up. A day picked here filters the feed below.

use std::collections::BTreeMap;

use jiff::ToSpan;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use livtk_core::DayEvents;

/// What happened on a day, by kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub total: u64,
    pub created: u64,
    pub moves: u64,
    pub done: u64,
    pub decisions: u64,
    pub notes: u64,
    pub links: u64,
    pub edits: u64,
}

impl Counts {
    fn add(&mut self, o: &Counts) {
        self.total += o.total;
        self.created += o.created;
        self.moves += o.moves;
        self.done += o.done;
        self.decisions += o.decisions;
        self.notes += o.notes;
        self.links += o.links;
        self.edits += o.edits;
    }
}

/// The states a node reaches only by the human's decision.
const DECIDED: &[&str] = &["acked", "ratified", "accepted", "rejected", "retired"];

/// Which stretch of days the grid shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    /// The last 12 months, to this week.
    Recent,
    Year(i16),
}

/// Whose activity counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    All,
    Human,
    Claude,
}

/// What activity counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum What {
    All,
    Done,
    Decisions,
    Notes,
}

impl What {
    fn of(self, c: &Counts) -> u64 {
        match self {
            What::All => c.total,
            What::Done => c.done,
            What::Decisions => c.decisions,
            What::Notes => c.notes,
        }
    }
}

/// One day in the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub day: Date,
    /// What counts that day, by the grid's who and what.
    pub count: u64,
    /// 0 for nothing, then 1 to 4 by the range's own busy days.
    pub level: u8,
    /// Everything that happened, by kind (for the grid's who).
    pub counts: Counts,
    pub picked: bool,
}

/// The range in numbers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub total: u64,
    pub busiest: Option<(Date, u64)>,
    /// Days in a row with something, to today (or yesterday, today being young).
    pub current: u64,
    pub longest: u64,
    pub done: u64,
    pub decisions: u64,
}

/// The grid as drawn: its weeks (a day out of the range is `None`), where each month's label
/// goes (a week's index), and the summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawn {
    pub weeks: Vec<[Option<Cell>; 7]>,
    pub months: Vec<(usize, &'static str)>,
    pub summary: Summary,
}

/// The grid's choices and the days it counts.
#[derive(Clone, Debug)]
pub struct Grid {
    pub span: Span,
    pub who: Who,
    pub what: What,
    pub folded: bool,
    /// The days the feed is filtered to, first and last.
    pub pick: Option<(Date, Date)>,
    /// Per day: the human's, then Claude's.
    days: BTreeMap<Date, [Counts; 2]>,
}

impl Default for Grid {
    fn default() -> Grid {
        Grid {
            span: Span::Recent,
            who: Who::All,
            what: What::All,
            folded: false,
            pick: None,
            days: BTreeMap::new(),
        }
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

impl Grid {
    /// Take in the counts per day.
    pub fn set_days(&mut self, rows: &[DayEvents]) {
        self.days.clear();
        for r in rows {
            let Ok(day) = r.day.parse::<Date>() else {
                continue;
            };
            let mut c = Counts {
                total: r.count,
                ..Counts::default()
            };
            match (r.op.as_str(), r.state.as_deref()) {
                ("node.create", _) => c.created = r.count,
                ("node.set", Some(state)) => {
                    c.moves = r.count;
                    if state == "done" {
                        c.done = r.count;
                    }
                    if DECIDED.contains(&state) {
                        c.decisions = r.count;
                    }
                }
                ("node.set", None) => c.edits = r.count,
                ("node.note", _) => c.notes = r.count,
                _ => c.links = r.count,
            }
            let slot = &mut self.days.entry(day).or_default()[usize::from(!r.human)];
            slot.add(&c);
        }
    }

    /// The years the log has days in, newest first, this one included.
    pub fn years(&self, today: Date) -> Vec<i16> {
        let mut years: Vec<i16> = self.days.keys().map(|d| d.year()).collect();
        years.push(today.year());
        years.sort_unstable_by(|a, b| b.cmp(a));
        years.dedup();
        years
    }

    /// A day's counts, for the grid's who.
    fn counts(&self, day: Date) -> Counts {
        let Some([human, claude]) = self.days.get(&day) else {
            return Counts::default();
        };
        match self.who {
            Who::All => {
                let mut c = *human;
                c.add(claude);
                c
            }
            Who::Human => *human,
            Who::Claude => *claude,
        }
    }

    /// The first and last day of the span, as `today` sees it.
    pub fn bounds(&self, today: Date) -> (Date, Date) {
        match self.span {
            Span::Recent => (today.checked_sub(364.days()).unwrap_or(today), today),
            Span::Year(y) => (
                Date::new(y, 1, 1).unwrap_or(today),
                Date::new(y, 12, 31).unwrap_or(today),
            ),
        }
    }

    /// The grid as drawn on `today`.
    pub fn drawn(&self, today: Date) -> Drawn {
        let (first, last) = self.bounds(today);
        // From the Monday of the first day's week to the Sunday of the last's.
        let start = first
            .checked_sub((i64::from(first.weekday().to_monday_zero_offset())).days())
            .unwrap_or(first);
        let in_range = |d: Date| d >= first && d <= last && d <= today;
        // The levels, by the range's busy days, as GitHub's quartiles.
        let mut busy: Vec<u64> = Vec::new();
        let end = last.min(today);
        let mut d = first;
        while d <= end {
            let n = self.what.of(&self.counts(d));
            if n > 0 {
                busy.push(n);
            }
            match d.tomorrow() {
                Ok(next) => d = next,
                Err(_) => break,
            }
        }
        busy.sort_unstable();
        // Quartiles over the positions below the top, so the busiest day reaches the top step.
        let q = |p: usize| busy.get(busy.len().saturating_sub(1) * p / 4);
        let (q1, q2, q3) = (q(1).copied(), q(2).copied(), q(3).copied());
        let level = |n: u64| -> u8 {
            if n == 0 {
                0
            } else if q1.is_none_or(|q| n <= q) {
                1
            } else if q2.is_none_or(|q| n <= q) {
                2
            } else if q3.is_none_or(|q| n <= q) {
                3
            } else {
                4
            }
        };
        let mut weeks = Vec::new();
        let mut months: Vec<(usize, &'static str)> = Vec::new();
        let mut summary = Summary::default();
        let mut run = 0u64;
        let mut day = start;
        loop {
            let mut week: [Option<Cell>; 7] = Default::default();
            for slot in &mut week {
                if in_range(day) {
                    let counts = self.counts(day);
                    let count = self.what.of(&counts);
                    summary.total += count;
                    summary.done += counts.done;
                    summary.decisions += counts.decisions;
                    if count > 0 && summary.busiest.is_none_or(|(_, b)| count > b) {
                        summary.busiest = Some((day, count));
                    }
                    run = if count > 0 { run + 1 } else { 0 };
                    summary.longest = summary.longest.max(run);
                    if day.day() == 1 || (weeks.is_empty() && months.is_empty()) {
                        let w = weeks.len();
                        // Two labels never crowd each other.
                        if months.last().is_none_or(|(at, _)| w >= at + 3) {
                            months.push((w, MONTHS[usize::try_from(day.month() - 1).unwrap_or(0)]));
                        }
                    }
                    *slot = Some(Cell {
                        day,
                        count,
                        level: level(count),
                        counts,
                        picked: self.pick.is_some_and(|(a, b)| day >= a && day <= b),
                    });
                }
                day = match day.tomorrow() {
                    Ok(d) => d,
                    Err(_) => break,
                };
            }
            weeks.push(week);
            if day > last.min(today) {
                break;
            }
        }
        summary.current = self.streak_to(today);
        Drawn {
            weeks,
            months,
            summary,
        }
    }

    /// Days in a row with something, ending today, or yesterday when nothing happened today
    /// yet.
    fn streak_to(&self, today: Date) -> u64 {
        let has = |d: Date| self.what.of(&self.counts(d)) > 0;
        let mut day = if has(today) {
            today
        } else {
            match today.yesterday() {
                Ok(d) => d,
                Err(_) => return 0,
            }
        };
        let mut n = 0;
        while has(day) {
            n += 1;
            day = match day.yesterday() {
                Ok(d) => d,
                Err(_) => break,
            };
        }
        n
    }

    /// Pick `day` for the feed: alone, or with `extend` the days from the one picked before.
    pub fn pick(&mut self, day: Date, extend: bool) {
        self.pick = match (self.pick, extend) {
            (Some((a, b)), true) => Some((a.min(day), b.max(day))),
            _ => Some((day, day)),
        };
    }
}

/// The span of the log a pick covers, as the log writes times (UTC, milliseconds): from the
/// first day's local midnight to the midnight after the last.
pub fn pick_bounds(pick: (Date, Date)) -> Option<(String, String)> {
    let tz = TimeZone::system();
    let from = pick.0.to_zoned(tz.clone()).ok()?.timestamp();
    let to = pick.1.tomorrow().ok()?.to_zoned(tz).ok()?.timestamp();
    Some((format!("{from:.3}"), format!("{to:.3}")))
}

/// Today, where this machine is.
pub fn today() -> Date {
    jiff::Timestamp::now().to_zoned(TimeZone::system()).date()
}

/// A weekday's short name, for the grid's side.
pub fn weekday_name(i: usize) -> &'static str {
    match i {
        0 => "Mon",
        2 => "Wed",
        4 => "Fri",
        _ => "",
    }
}
