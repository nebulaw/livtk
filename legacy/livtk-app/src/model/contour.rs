//! A year of days as GitHub draws contributions, for any count a day has: the days in whole
//! weeks (Monday first) to today, each given a level by the range's own busy days, and cut into
//! months; and how a pick is made in it (a click anchors a day, a second closes the range, a
//! month is taken whole). Plain data and arithmetic, drawn by `ui::contour_grid`.

use jiff::ToSpan;
use jiff::civil::Date;

/// One day in the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub day: Date,
    /// What the day counts.
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

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A month's short name.
pub fn month_name(d: Date) -> &'static str {
    MONTHS[usize::try_from(d.month() - 1).unwrap_or(0)]
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

/// The year to `today` laid out, each day counting `count(day)`.
pub fn layout(today: Date, count: impl Fn(Date) -> u64) -> Drawn {
    let (first, last) = bounds(today);
    // The levels split the range's busy days in five, as GitHub's quartiles do in four;
    // each step's top taken below the busiest, so the busiest day reaches the top step.
    let mut busy: Vec<u64> = Vec::new();
    let mut d = first;
    while d <= last {
        let n = count(d);
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
            let count = if future { 0 } else { count(d) };
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

/// What is picked in a grid: the days from first to last, and whether a day was clicked and
/// waits for a second to close the range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pick {
    pub span: Option<(Date, Date)>,
    pub anchored: bool,
}

impl Pick {
    /// A day clicked: it anchors a range (its own day picked meanwhile), or closes the range a
    /// day anchored.
    pub fn click(&mut self, day: Date) {
        match (self.span, self.anchored) {
            (Some((a, _)), true) => {
                self.span = Some((a.min(day), a.max(day)));
                self.anchored = false;
            }
            _ => {
                self.span = Some((day, day));
                self.anchored = true;
            }
        }
    }

    /// A span of days picked at once: a month.
    pub fn take(&mut self, first: Date, last: Date) {
        self.span = Some((first, last));
        self.anchored = false;
    }

    /// Nothing picked. Whether something was.
    pub fn clear(&mut self) -> bool {
        self.anchored = false;
        self.span.take().is_some()
    }
}
