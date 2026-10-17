//! The activity grid: a year of days as GitHub draws contributions, each day's colour by how
//! much happened in the project, above the activity feed. Counts come from one grouped query
//! (`Workspace::activity_by_day`); here they are filtered by who and what, laid out in weeks
//! (Monday first), given levels, and cut into months. A day, or the days
//! between two clicked, or a month, picked here filters the feed below.

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
    /// 0 for nothing, then 1 to 5 by the range's own busy days.
    pub level: u8,
    /// After today: not yet.
    pub future: bool,
}

/// A month in the grid: its name, the week its name goes over (its first whole week), and its
/// first and last day shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Month {
    pub name: &'static str,
    pub label_week: usize,
    pub first: Date,
    pub last: Date,
}

/// The grid as drawn: its weeks, Monday first, and its months.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawn {
    pub weeks: Vec<[Cell; 7]>,
    pub months: Vec<Month>,
}

impl Drawn {
    /// Where `day` sits: its week and its weekday (0 for Monday).
    pub fn at(&self, day: Date) -> Option<(usize, usize)> {
        let first = self.weeks.first()?[0].day;
        let n = usize::try_from((day - first).get_days()).ok()?;
        (n < self.weeks.len() * 7).then_some((n / 7, n % 7))
    }
}

/// How many levels there are, nothing included.
pub const LEVELS: usize = 6;

/// The grid's choices and the days it counts.
#[derive(Clone, Debug)]
pub struct Grid {
    pub who: Who,
    pub what: What,
    pub folded: bool,
    /// The days the feed is filtered to, first and last.
    pub pick: Option<(Date, Date)>,
    /// A day was clicked and waits for a second to close the range.
    pub anchored: bool,
    /// Per day: the human's, then Claude's.
    days: BTreeMap<Date, [Counts; 2]>,
}

impl Default for Grid {
    fn default() -> Grid {
        Grid {
            who: Who::All,
            what: What::All,
            folded: false,
            pick: None,
            anchored: false,
            days: BTreeMap::new(),
        }
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A month's short name.
pub fn month_name(d: Date) -> &'static str {
    MONTHS[usize::try_from(d.month() - 1).unwrap_or(0)]
}

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

    /// The first and last day shown: from the Monday a year back, so every week is whole, to
    /// `today`.
    pub fn bounds(today: Date) -> (Date, Date) {
        let back = today.checked_sub(1.year()).unwrap_or(today);
        let monday = back
            .checked_sub(i64::from(back.weekday().to_monday_zero_offset()).days())
            .unwrap_or(back);
        (monday, today)
    }

    /// The grid as drawn on `today`.
    pub fn drawn(&self, today: Date) -> Drawn {
        let (first, last) = Grid::bounds(today);
        // The levels split the range's busy days in five, as GitHub's quartiles do in four;
        // each step's top taken below the busiest, so the busiest day reaches the top step.
        let mut busy: Vec<u64> = Vec::new();
        let mut d = first;
        while d <= last {
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
        let tops: Vec<u64> = if busy.is_empty() {
            Vec::new()
        } else {
            (1..LEVELS - 1)
                .map(|p| busy[(busy.len() - 1) * p / (LEVELS - 1)])
                .collect()
        };
        let level = |n: u64| -> u8 {
            if n == 0 {
                return 0;
            }
            let step = tops.iter().position(|&t| n <= t).unwrap_or(tops.len());
            u8::try_from(step + 1).unwrap_or(0)
        };
        let mut weeks = Vec::new();
        let mut months: Vec<Month> = Vec::new();
        let mut day = first;
        while day <= last {
            let w = weeks.len();
            let week: [Cell; 7] = std::array::from_fn(|i| {
                let d = day.checked_add((i as i64).days()).unwrap_or(day);
                let future = d > last;
                let count = if future {
                    0
                } else {
                    self.what.of(&self.counts(d))
                };
                Cell {
                    day: d,
                    count,
                    level: level(count),
                    future,
                }
            });
            for c in &week {
                match months.last_mut() {
                    Some(m) if m.first.month() == c.day.month() => m.last = c.day,
                    _ => months.push(Month {
                        name: month_name(c.day),
                        // A month's name goes over its first whole week.
                        label_week: if c.day.weekday().to_monday_zero_offset() == 0 {
                            w
                        } else {
                            w + 1
                        },
                        first: c.day,
                        last: c.day,
                    }),
                }
            }
            weeks.push(week);
            day = match day.checked_add(7.days()) {
                Ok(d) => d,
                Err(_) => break,
            };
        }
        // The first month's name goes over the first week, however much of it there is.
        if let Some(m) = months.first_mut() {
            m.label_week = 0;
        }
        Drawn { weeks, months }
    }

    /// A day clicked: it anchors a range (the feed shows that day meanwhile), or closes the
    /// range a day anchored.
    pub fn pick(&mut self, day: Date) {
        self.pick = match (self.pick, self.anchored) {
            (Some((a, _)), true) => {
                self.anchored = false;
                Some((a.min(day), a.max(day)))
            }
            _ => {
                self.anchored = true;
                Some((day, day))
            }
        };
    }

    /// A span of days picked at once: a month.
    pub fn pick_span(&mut self, first: Date, last: Date) {
        self.pick = Some((first, last));
        self.anchored = false;
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
