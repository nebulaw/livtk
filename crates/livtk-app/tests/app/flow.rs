//! The state flow's geometry: where states and threads go. Plain
//! arithmetic, so tested without a window.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use livtk_app::model::{Step, arrows, flow_of, steps};
use livtk_app::ui::state_flow::geometry::{
    BRANCH_Y, CURRENT, Glow, HEIGHT, INSET, Layout, MAIN_Y, bezier, curve, layout,
};
use livtk_core::{Kind, State};

fn flow(kind: Kind, state: State, width: f32) -> (Vec<Step>, Layout) {
    let steps = steps(kind, state);
    let l = layout(&steps, &arrows(kind, state), width);
    (steps, l)
}

fn index(l: &Layout, s: State) -> usize {
    l.nodes.iter().position(|n| n.state == s).unwrap()
}

#[test]
fn the_main_path_runs_left_to_right_and_the_states_off_it_hang_below() {
    for kind in [Kind::Task, Kind::Lesson, Kind::Decision] {
        let (steps, l) = flow(kind, flow_of(kind).0[0], 400.);
        let main: Vec<_> = l.nodes.iter().filter(|n| n.main).collect();
        assert_eq!(main.first().unwrap().at.x, INSET, "{kind:?}");
        assert_eq!(main.last().unwrap().at.x, 400. - INSET, "{kind:?}");
        for pair in main.windows(2) {
            assert!(pair[0].at.x < pair[1].at.x, "{kind:?}: left to right");
            assert_eq!(pair[0].at.y, MAIN_Y);
        }
        for n in l.nodes.iter().filter(|n| !n.main) {
            assert_eq!(n.at.y, BRANCH_Y, "{kind:?}: {:?} below", n.state);
            assert!(n.at.x > INSET && n.at.x < 400. - INSET, "under the path");
        }
        assert_eq!(steps.len(), l.nodes.len(), "a node for every state");
    }
}

#[test]
fn everything_fits_the_band_at_the_narrowest_panel() {
    // The narrowest panel's content: 360 less its padding.
    let (_, l) = flow(Kind::Task, State::Planned, 320.);
    let main: Vec<_> = l.nodes.iter().filter(|n| n.main).collect();
    for pair in main.windows(2) {
        assert!(
            pair[1].at.x - pair[0].at.x >= 50.,
            "room for a label between: {pair:?}"
        );
    }
    for t in &l.threads {
        for strand in [-1., 1.] {
            let c = curve(&l, t, strand);
            for k in 0..=10 {
                let p = bezier(&c, k as f32 / 10.);
                assert!(
                    (0. ..=320.).contains(&p.x) && (0. ..=HEIGHT).contains(&p.y),
                    "{t:?} at {p:?}"
                );
            }
        }
    }
}

#[test]
fn threads_join_neighbours_and_drop_to_the_states_below_lit_as_the_moves_are() {
    // A planned task: acknowledging is the human's decision; back to backlog anyone can do.
    let (steps, l) = flow(Kind::Task, State::Planned, 400.);
    let along: Vec<_> = l.threads.iter().filter(|t| t.along).collect();
    assert_eq!(along.len(), 5, "one between each pair of neighbours");
    let planned = index(&l, State::Planned);
    let acked = index(&l, State::Acked);
    let to_acked = along.iter().find(|t| t.to == acked).unwrap();
    assert_eq!(to_acked.from, planned, "lit from here toward it");
    assert_eq!(to_acked.glow, Glow::Decision);
    let done = index(&l, State::Done);
    let to_done = along.iter().find(|t| t.to == done).unwrap();
    assert_eq!(to_done.glow, Glow::Idle, "out of reach from planned");
    let down: Vec<_> = l.threads.iter().filter(|t| !t.along).collect();
    assert_eq!(down.len(), 2, "to parked and canceled");
    for t in down {
        assert_eq!(t.from, planned);
        let reachable = steps[t.to].to.is_some();
        assert_eq!(t.glow != Glow::Idle, reachable, "{:?}", l.nodes[t.to].state);
        // From under the labels down to the state's top.
        let c = curve(&l, t, 1.);
        assert!(
            c[0].y > MAIN_Y + CURRENT / 2. + 12.,
            "below the labels: {c:?}"
        );
        assert!(c[3].y < BRANCH_Y, "{c:?}");
    }
}

#[test]
fn a_parked_task_rises_to_the_states_it_can_go_back_to() {
    let (steps, l) = flow(Kind::Task, State::Parked, 400.);
    let parked = index(&l, State::Parked);
    let up: Vec<_> = l.threads.iter().filter(|t| !t.along).collect();
    assert!(!up.is_empty());
    for t in up {
        assert_eq!(t.from, parked);
        assert!(
            steps[t.to].main && steps[t.to].to.is_some(),
            "reachable only"
        );
    }
    assert!(
        l.threads
            .iter()
            .filter(|t| t.along)
            .all(|t| t.glow == Glow::Idle),
        "nothing along the path is lit from below it"
    );
}
