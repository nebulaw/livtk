//! A kind's states as a flow, and changing state from anywhere, without a window.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use livtk_app::model::{Model, View, flow_of, has_flow, state_name, steps};
use livtk_core::{Actor, Command, Kind, State, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

/// (state, current, a move there, the human's decision), for every step.
fn seen(kind: Kind, state: State) -> Vec<(State, bool, bool, bool)> {
    steps(kind, state)
        .iter()
        .map(|s| (s.state, s.current, s.to.is_some(), s.decides()))
        .collect()
}

#[test]
fn a_planned_task_sees_the_whole_flow_and_where_it_can_go() {
    use State::*;
    assert_eq!(
        seen(Kind::Task, Planned),
        [
            (Backlog, false, true, false),
            (Planned, true, false, false),
            (Acked, false, true, true),
            (Doing, false, false, false),
            (Review, false, false, false),
            (Done, false, false, false),
            (Parked, false, true, false),
            (Canceled, false, true, false),
        ],
        "acknowledging is the human's; backlog, parked and canceled are moves; the rest waits"
    );
    let main: Vec<bool> = steps(Kind::Task, Planned).iter().map(|s| s.main).collect();
    assert_eq!(main, [true, true, true, true, true, true, false, false]);
}

#[test]
fn every_step_can_be_reached_only_as_the_core_allows() {
    for kind in [Kind::Task, Kind::Lesson, Kind::Decision, Kind::Finding] {
        let (main, side) = flow_of(kind);
        for &from in main.iter().chain(side) {
            for s in steps(kind, from) {
                assert_eq!(
                    s.to.is_some() || s.standing.is_some(),
                    kind.can_move(from, s.state),
                    "{kind:?} {from:?} -> {:?}",
                    s.state
                );
                assert!(
                    !(s.current && s.to.is_some()),
                    "never a move to where it is"
                );
            }
        }
        let mut all: Vec<State> = main.iter().chain(side).copied().collect();
        all.sort_by_key(|s| s.as_str());
        let mut want = kind.states().to_vec();
        want.sort_by_key(|s| s.as_str());
        assert_eq!(all, want, "the flow shows every state of a {kind:?}");
    }
}

#[test]
fn a_proposed_lessons_ratify_step_offers_the_standing_rule_too() {
    let steps = steps(Kind::Lesson, State::Proposed);
    let ratified = steps.iter().find(|s| s.state == State::Ratified).unwrap();
    assert!(ratified.decides());
    assert!(!ratified.to.as_ref().unwrap().always);
    assert!(ratified.standing.as_ref().unwrap().always);
    let rejected = steps.iter().find(|s| s.state == State::Rejected).unwrap();
    assert!(!rejected.main && rejected.decides());
}

#[test]
fn a_finding_has_no_flow_and_states_have_their_names() {
    assert!(!has_flow(Kind::Finding));
    assert!(has_flow(Kind::Task) && has_flow(Kind::Lesson) && has_flow(Kind::Decision));
    assert_eq!(state_name(State::Acked), "Acknowledged");
    assert_eq!(state_name(State::Review), "In review");
}

fn create(ws: &mut Workspace, title: &str, to: Option<State>) -> String {
    let e = ws
        .execute(
            Command::Create {
                kind: Kind::Task,
                title: title.into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &CLAUDE,
        )
        .unwrap();
    let key = ws.node(&e[0].op.subjects().0.to_string()).unwrap().key;
    if let Some(to) = to {
        ws.execute(
            Command::Move {
                node: key.clone(),
                to,
            },
            &CLAUDE,
        )
        .unwrap();
    }
    key
}

#[test]
fn a_state_changes_on_any_row_and_the_selection_stays_put() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    for t in ["one", "two", "three"] {
        create(&mut ws, t, Some(State::Planned));
    }
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    assert_eq!(m.view, View::Inbox);
    let ids: Vec<_> = m.rows.iter().map(|r| r.id).collect();
    m.select(ids[2]);

    // Acknowledge the first row, which is not the selected one.
    let ack = steps(Kind::Task, State::Planned)
        .into_iter()
        .find(|s| s.state == State::Acked)
        .unwrap()
        .to
        .unwrap();
    m.choose_on(ids[0], &ack);
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.node("LIV-1").unwrap().state, State::Acked);
    assert_eq!(
        m.selected().unwrap().id,
        ids[2],
        "the selection stays on LIV-3"
    );
    assert_eq!(m.leaving.len(), 1);
    assert_eq!(m.leaving[0].1, 0, "LIV-1 fades from its own place, the top");
}
