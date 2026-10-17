//! The activity grid's data: how much happened in the project each day, above the activity
//! feed. Counts come from one grouped query (`Workspace::activity_by_day`); here they are
//! filtered by who and what, and laid out as `contour` lays out any count by day. The days
//! picked in it filter the feed below.

use std::collections::BTreeMap;

use jiff::civil::Date;
use jiff::tz::TimeZone;
use livtk_core::DayEvents;

use super::contour::{self, Drawn, Pick};

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

/// The grid's choices and the days it counts.
#[derive(Clone, Debug)]
pub struct Grid {
    pub who: Who,
    pub what: What,
    pub folded: bool,
    /// The days the feed is filtered to.
    pub pick: Pick,
    /// Per day: the human's, then Claude's.
    days: BTreeMap<Date, [Counts; 2]>,
}

impl Default for Grid {
    fn default() -> Grid {
        Grid {
            who: Who::All,
            what: What::All,
            folded: false,
            pick: Pick::default(),
            days: BTreeMap::new(),
        }
    }
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

    /// The year to `today` as drawn, each day counting by the grid's who and what.
    pub fn drawn(&self, today: Date) -> Drawn {
        contour::layout(today, |d| self.what.of(&self.counts(d)))
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
