//! A node's activity as a timeline on a rail: its events in order, each a mark on the rail, the
//! work of each Claude session on a lane of its own (branching off at its first event, merging
//! back after its last), runs of small changes folded, and the states still ahead after the
//! last. Plain logic, tested on its own: the rail draws exactly this.

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
}

/// What a line of the timeline is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// The event `at` in the node's history, on `lane`; `opens` when its session's lane
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

/// The session an event belongs to, for its lane: the run a run's note marks, whoever wrote it
/// (a run started, answered or stopped from the app is written as the human); else the one
/// Claude wrote it from. The human's own are on the rail itself.
fn session_of(e: &Event) -> Option<&str> {
    if let Op::NodeNote { data, .. } = &e.op
        && let Some(run) = &data.run
    {
        return Some(run.session.as_str());
    }
    if e.by == "human" {
        None
    } else {
        e.session.as_deref()
    }
}

/// The timeline of `events` (a node's history, oldest first), a node of `kind` now in `state`;
/// `open(key)` says whether a fold was opened.
pub fn timeline(
    events: &[Event],
    kind: Kind,
    state: State,
    open: impl Fn(Uuid) -> bool,
) -> Timeline {
    // Each session's first and last event: where its lane branches off and merges back.
    let mut span: HashMap<&str, (usize, usize)> = HashMap::new();
    for (i, e) in events.iter().enumerate() {
        if let Some(s) = session_of(e) {
            span.entry(s)
                .and_modify(|(_, last)| *last = i)
                .or_insert((i, i));
        }
    }
    // Lanes, as a git graph's: a session takes the lowest free one past the rail's while it
    // works, and gives it back after its last event.
    let mut held: Vec<Option<&str>> = Vec::new();
    let mut lanes = 1;
    let mut placed = Vec::with_capacity(events.len());
    for (i, e) in events.iter().enumerate() {
        let (lane, opens, closes) = match session_of(e) {
            None => (0, false, false),
            Some(s) => {
                let (first, last) = span[s];
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
