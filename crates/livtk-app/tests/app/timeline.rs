//! A node's activity laid out on a rail: lanes for Claude's sessions, folds for runs of small
//! changes, the states still ahead. Plain logic, tested without a window.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use livtk_app::model::timeline::{FOLD_AT, Item, Mark, RunStep, ahead_of, timeline};
use livtk_core::event::{EdgeData, NodePatch, NoteData, Op, RunData, RunMark, Stage};
use livtk_core::{Actor, Event, Kind, Rel, State};
use uuid::Uuid;

/// Events as a test writes them, each after the one before.
struct Log(Vec<Event>, Uuid);

impl Log {
    fn new() -> Log {
        Log(Vec::new(), Uuid::now_v7())
    }

    fn push(&mut self, by: &str, session: Option<&str>, op: Op) -> &mut Log {
        let actor = if by == "human" {
            Actor::Human
        } else {
            Actor::Claude { agent: None }
        };
        let mut e = Event::new(self.0.last().map(|e| e.id), &actor, op);
        e.session = session.map(str::to_owned);
        self.0.push(e);
        self
    }

    fn moved(&mut self, by: &str, to: State) -> &mut Log {
        let node = self.1;
        self.push(
            by,
            None,
            Op::NodeSet {
                node,
                data: NodePatch {
                    state: Some(to),
                    ..NodePatch::default()
                },
            },
        )
    }

    fn renamed(&mut self, by: &str) -> &mut Log {
        let node = self.1;
        self.push(
            by,
            None,
            Op::NodeSet {
                node,
                data: NodePatch {
                    title: Some("t".into()),
                    ..NodePatch::default()
                },
            },
        )
    }

    fn note(&mut self, by: &str, session: Option<&str>) -> &mut Log {
        let node = self.1;
        self.push(
            by,
            session,
            Op::NodeNote {
                node,
                data: NoteData {
                    text: "said".into(),
                    health: None,
                    run: None,
                },
            },
        )
    }

    fn run(&mut self, session: &str, mark: RunMark) -> &mut Log {
        self.run_by("claude", session, mark)
    }

    /// A run's note written by `by`: the app writes a run started, answered or stopped from it
    /// as the human.
    fn run_by(&mut self, by: &str, session: &str, mark: RunMark) -> &mut Log {
        let node = self.1;
        self.push(
            by,
            Some(session),
            Op::NodeNote {
                node,
                data: NoteData {
                    text: "run".into(),
                    health: None,
                    run: Some(RunData {
                        session: session.into(),
                        agent: None,
                        stage: Stage::Build,
                        mark,
                    }),
                },
            },
        )
    }

    fn linked(&mut self, by: &str, session: Option<&str>) -> &mut Log {
        let node = self.1;
        self.push(
            by,
            session,
            Op::EdgeAdd {
                data: EdgeData {
                    src: node,
                    dst: Uuid::now_v7(),
                    rel: Rel::DependsOn,
                },
            },
        )
    }
}

fn lanes_of(items: &[Item]) -> Vec<usize> {
    items
        .iter()
        .filter_map(|i| match i {
            Item::Event { lane, .. } => Some(*lane),
            _ => None,
        })
        .collect()
}

#[test]
fn the_human_alone_is_one_rail() {
    let mut log = Log::new();
    log.note("human", None).note("human", None);
    let t = timeline(&log.0, Kind::Task, State::Backlog, |_| false);
    assert_eq!(t.lanes, 1);
    assert_eq!(lanes_of(&t.items), [0, 0]);
}

#[test]
fn a_session_branches_off_and_merges_back() {
    let mut log = Log::new();
    log.moved("human", State::Acked)
        .run("s1", RunMark::Started)
        .note("claude", Some("s1"))
        .note("human", None)
        .run(
            "s1",
            RunMark::Ended {
                how: livtk_core::event::Ending::Done,
            },
        )
        .note("human", None);
    let t = timeline(&log.0, Kind::Task, State::Review, |_| false);
    assert_eq!(t.lanes, 2);
    assert_eq!(lanes_of(&t.items), [0, 1, 1, 0, 1, 0]);
    let flags: Vec<(bool, bool)> = t
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Event { opens, closes, .. } => Some((*opens, *closes)),
            _ => None,
        })
        .collect();
    assert_eq!(flags[1], (true, false), "branches off at its first");
    assert_eq!(flags[4], (false, true), "merges back after its last");
    assert!(matches!(
        t.items[1],
        Item::Event {
            mark: Mark::Run(RunStep::Started),
            ..
        }
    ));
}

#[test]
fn sessions_at_once_take_lanes_side_by_side_and_free_them() {
    let mut log = Log::new();
    log.note("claude", Some("a"))
        .note("claude", Some("b"))
        .note("claude", Some("a"))
        .note("claude", Some("b"))
        // `a` and `b` are done; `c` takes the first lane again.
        .note("claude", Some("c"));
    let t = timeline(&log.0, Kind::Task, State::Doing, |_| false);
    assert_eq!(lanes_of(&t.items), [1, 2, 1, 2, 1]);
    assert_eq!(t.lanes, 3);
}

#[test]
fn small_changes_in_a_row_fold_past_the_first_unless_opened() {
    let mut log = Log::new();
    log.note("human", None);
    for _ in 0..FOLD_AT + 1 {
        log.renamed("human");
    }
    log.note("human", None);
    let t = timeline(&log.0, Kind::Task, State::Backlog, |_| false);
    let folds: Vec<usize> = t
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Fold { count, .. } => Some(*count),
            _ => None,
        })
        .collect();
    assert_eq!(folds, [FOLD_AT], "all but the first folded");
    assert_eq!(
        lanes_of(&t.items).len(),
        3,
        "a note, the first change, a note"
    );
    // Opened: every one shows.
    let key = log.0[1].id;
    let t = timeline(&log.0, Kind::Task, State::Backlog, |k| k == key);
    assert!(!t.items.iter().any(|i| matches!(i, Item::Fold { .. })));
    // Fewer than FOLD_AT do not fold, and notes never do.
    let mut short = Log::new();
    for _ in 0..FOLD_AT - 1 {
        short.renamed("human");
    }
    for _ in 0..FOLD_AT + 2 {
        short.note("human", None);
    }
    let t = timeline(&short.0, Kind::Task, State::Backlog, |_| false);
    assert!(!t.items.iter().any(|i| matches!(i, Item::Fold { .. })));
}

#[test]
fn a_lane_branching_or_merging_is_never_folded_away() {
    let mut log = Log::new();
    for _ in 0..FOLD_AT + 2 {
        log.linked("claude", Some("s"));
    }
    let t = timeline(&log.0, Kind::Task, State::Doing, |_| false);
    // The first (branching) and the last (merging) stay; the ones between fold.
    let shown = lanes_of(&t.items).len();
    assert!(shown >= 2, "{:?}", t.items);
    assert!(matches!(
        t.items.iter().find(|i| matches!(i, Item::Event { .. })),
        Some(Item::Event { opens: true, .. })
    ));
    assert!(
        t.items
            .iter()
            .any(|i| matches!(i, Item::Event { closes: true, .. }))
    );
}

#[test]
fn the_states_ahead_follow_the_last_event() {
    assert_eq!(
        ahead_of(Kind::Task, State::Doing),
        [State::Review, State::Done]
    );
    assert!(ahead_of(Kind::Task, State::Done).is_empty());
    assert!(
        ahead_of(Kind::Task, State::Parked).is_empty(),
        "off the path, nothing ahead"
    );
    let mut log = Log::new();
    log.moved("human", State::Doing);
    let t = timeline(&log.0, Kind::Task, State::Doing, |_| false);
    assert_eq!(
        &t.items[1..],
        [Item::Ahead(State::Review), Item::Ahead(State::Done)]
    );
    assert_eq!(t.now(), Some(0), "the rail has reached the last event");
}

#[test]
fn a_runs_notes_stay_on_its_lane_whoever_wrote_them() {
    let mut log = Log::new();
    log.note("human", None)
        // Started and answered from the app, as the human; its own words as Claude.
        .run_by("human", "r", RunMark::Started)
        .note("claude", Some("r"))
        .run("r", RunMark::Asks { allow: vec![] })
        .run_by("human", "r", RunMark::Allowed)
        .note("human", None)
        .run_by(
            "human",
            "r",
            RunMark::Ended {
                how: livtk_core::event::Ending::Stopped,
            },
        );
    let t = timeline(&log.0, Kind::Task, State::Doing, |_| false);
    assert_eq!(lanes_of(&t.items), [0, 1, 1, 1, 1, 0, 1]);
    assert!(
        matches!(t.items[1], Item::Event { opens: true, .. }),
        "branches at its start"
    );
    assert!(
        matches!(t.items[6], Item::Event { closes: true, .. }),
        "merges at its end"
    );
}
