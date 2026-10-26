//! A node's activity as a timeline on a rail: its events in order, each a mark on it, runs of
//! small changes folded, and the states still ahead after the last. Plain logic, tested on its
//! own: the rail draws exactly this.
//!
//! Two levels. The rail (lane 0) is the node's life: its making, every state move outside an
//! agent's own work, the human's words and edits, and bookkeeping. A lane is one *stint*, one
//! agent working on it: a run from its start to its end, or an interactive agent (its actor and
//! its conversation) from its first event after a move to its own move, which is its last
//! mark, or to the event before someone else's move. A move closes every stint but a run still
//! going. Stints at once take lanes side by side, as a git graph's.

use std::collections::HashMap;

use livtk_core::event::{Op, RunMark};
use livtk_core::{Event, Kind, State};
use uuid::Uuid;

use super::flow_of;

/// How many small changes in a row fold into one line.
pub const FOLD_AT: usize = 4;

/// What an event is, for its mark and whether it folds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Created,
    /// Moved to a state.
    Moved(State),
    /// Renamed, its plan edited, its priority or scope set.
    Changed,
    Linked,
    Unlinked,
    Doc,
    /// A session said it worked on it.
    Session,
    /// A note, the human's or another's.
    Note {
        human: bool,
    },
    /// A run of Claude on it: started, asked, answered, ended.
    Run(RunStep),
}

/// Where a run's note stands in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStep {
    Started,
    Asks,
    Answered,
    Ended,
}

impl Mark {
    pub fn of(e: &Event) -> Mark {
        match &e.op {
            Op::NodeCreate { .. } => Mark::Created,
            Op::NodeSet { data, .. } => data.state.map_or(Mark::Changed, Mark::Moved),
            Op::NodeNote { data, .. } => match data.run.as_ref().map(|r| &r.mark) {
                Some(RunMark::Started) => Mark::Run(RunStep::Started),
                Some(RunMark::Asks { .. }) => Mark::Run(RunStep::Asks),
                Some(RunMark::Allowed | RunMark::Denied) => Mark::Run(RunStep::Answered),
                Some(RunMark::Ended { .. }) => Mark::Run(RunStep::Ended),
                None => Mark::Note {
                    human: e.by == "human",
                },
            },
            Op::EdgeAdd { .. } => Mark::Linked,
            Op::EdgeRemove { .. } => Mark::Unlinked,
            Op::DocLink { .. } | Op::DocUnlink { .. } => Mark::Doc,
            Op::NodeSession { .. } => Mark::Session,
        }
    }

    /// Whether it is a small change, that folds with others in a row: never what was said, a
    /// run's turn or the node's making.
    pub const fn folds(self) -> bool {
        matches!(
            self,
            Mark::Moved(_)
                | Mark::Changed
                | Mark::Linked
                | Mark::Unlinked
                | Mark::Doc
                | Mark::Session
        )
    }

    /// Whether it is on the rail whoever wrote it: the node's making and bookkeeping.
    pub const fn on_rail(self) -> bool {
        matches!(self, Mark::Created | Mark::Session)
    }

    /// Whether it ends the stints open before it, all but a run still going: a state move.
    pub const fn resets(self) -> bool {
        matches!(self, Mark::Moved(_))
    }
}

/// What a line of the timeline is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// The event `at` in the node's history, on `lane`; `opens` when its stint's lane
    /// branches off here, `closes` when it merges back after it.
    Event {
        at: usize,
        mark: Mark,
        lane: usize,
        opens: bool,
        closes: bool,
    },
    /// `count` small changes after the line above, folded; `key` opens them.
    Fold {
        lane: usize,
        count: usize,
        key: Uuid,
    },
    /// A state still ahead on the kind's path, not reached yet.
    Ahead(State),
}

/// A node's timeline: its lines, oldest first, and how many lanes it takes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timeline {
    pub items: Vec<Item>,
    pub lanes: usize,
}

impl Timeline {
    /// The line the rail has reached: the last event.
    pub fn now(&self) -> Option<usize> {
        self.items
            .iter()
            .rposition(|i| matches!(i, Item::Event { .. } | Item::Fold { .. }))
    }
}

/// Who works a stint: a run, by its session, whoever wrote its notes (a run started, answered
/// or stopped from the app is written as the human); or an interactive agent, its actor and
/// conversation (none for Claude's events from before sessions were recorded).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Worker<'a> {
    Run(&'a str),
    Agent(&'a str, Option<&'a str>),
}

/// Which stint each event is in, numbered in the order they open; `None` on the rail.
fn stints_of(events: &[Event]) -> Vec<Option<usize>> {
    let mut open: HashMap<Worker<'_>, usize> = HashMap::new();
    let mut opened = 0;
    let mut stints = Vec::with_capacity(events.len());
    for e in events {
        let mark = Mark::of(e);
        let run = match &e.op {
            Op::NodeNote { data, .. } => data.run.as_ref(),
            _ => None,
        };
        // Whose work it is: none on the rail.
        let worker = if let Some(run) = run {
            Some(Worker::Run(run.session.as_str()))
        } else if mark.on_rail() || e.by == "human" {
            None
        } else {
            match e.session.as_deref() {
                // A run's own work, on its lane.
                Some(s) if open.contains_key(&Worker::Run(s)) => Some(Worker::Run(s)),
                session => Some(Worker::Agent(e.by.as_str(), session)),
            }
        };
        let stint = worker.and_then(|w| match open.get(&w) {
            Some(&s) => Some(s),
            // A move with no stint of its own open is on the rail.
            None if mark.resets() && matches!(w, Worker::Agent(..)) => None,
            None => {
                open.insert(w, opened);
                opened += 1;
                Some(opened - 1)
            }
        });
        if mark == Mark::Run(RunStep::Ended)
            && let Some(w) = worker
        {
            open.remove(&w);
        }
        // A move ends every stint open before it but a run still going; its own, after it.
        if mark.resets() {
            open.retain(|w, _| matches!(w, Worker::Run(_)));
        }
        stints.push(stint);
    }
    stints
}

/// The timeline of `events` (a node's history, oldest first), a node of `kind` now in `state`;
/// `open(key)` says whether a fold was opened.
pub fn timeline(
    events: &[Event],
    kind: Kind,
    state: State,
    open: impl Fn(Uuid) -> bool,
) -> Timeline {
    let stints = stints_of(events);
    // Each stint's first and last event: where its lane branches off and merges back.
    let mut span: HashMap<usize, (usize, usize)> = HashMap::new();
    for (i, s) in stints.iter().enumerate() {
        if let Some(s) = *s {
            span.entry(s)
                .and_modify(|(_, last)| *last = i)
                .or_insert((i, i));
        }
    }
    // Lanes, as a git graph's: a stint takes the lowest free one past the rail's while it
    // works, and gives it back after its last event.
    let mut held: Vec<Option<usize>> = Vec::new();
    let mut lanes = 1;
    let mut placed = Vec::with_capacity(events.len());
    for (i, e) in events.iter().enumerate() {
        let (lane, opens, closes) = match stints[i] {
            None => (0, false, false),
            Some(s) => {
                let (first, last) = span[&s];
                let lane = if i == first {
                    let free = held.iter().position(Option::is_none);
                    let ix = free.unwrap_or_else(|| {
                        held.push(None);
                        held.len() - 1
                    });
                    held[ix] = Some(s);
                    ix + 1
                } else {
                    held.iter()
                        .position(|h| *h == Some(s))
                        .map_or(0, |ix| ix + 1)
                };
                if i == last && lane > 0 {
                    held[lane - 1] = None;
                }
                (lane, i == first, i == last)
            }
        };
        lanes = lanes.max(lane + 1);
        placed.push((Mark::of(e), lane, opens, closes));
    }
    // Small changes in a row on one lane fold, past the first, unless opened; a lane's
    // branching or merging is never folded away.
    let mut items = Vec::new();
    let mut i = 0;
    while i < placed.len() {
        let lane = placed[i].1;
        let mut j = i;
        while j < placed.len()
            && placed[j].0.folds()
            && placed[j].1 == lane
            && !placed[j].2
            && !placed[j].3
        {
            j += 1;
        }
        // A group too short to fold is one line; one opened, every line of it.
        let folded = j - i >= FOLD_AT && !open(events[i].id);
        let end = if j - i >= FOLD_AT { j } else { i + 1 };
        let shown = if folded { i + 1 } else { end };
        for (at, &(mark, lane, opens, closes)) in placed.iter().enumerate().take(shown).skip(i) {
            items.push(Item::Event {
                at,
                mark,
                lane,
                opens,
                closes,
            });
        }
        if folded {
            items.push(Item::Fold {
                lane,
                count: j - i - 1,
                key: events[i].id,
            });
        }
        i = end;
    }
    items.extend(ahead_of(kind, state).into_iter().map(Item::Ahead));
    Timeline { items, lanes }
}

/// The states after `state` on `kind`'s path, still to reach; none off it.
pub fn ahead_of(kind: Kind, state: State) -> Vec<State> {
    let (main, _) = flow_of(kind);
    main.iter()
        .position(|&s| s == state)
        .map(|at| main[at + 1..].to_vec())
        .unwrap_or_default()
}
