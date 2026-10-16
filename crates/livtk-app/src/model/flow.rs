//! A kind's states as a flow: the main path left to right, the states off it (parked, canceled,
//! rejected) beside it, and for a node in a given state which of them it can move to. The state
//! menu on rows and cards and the flow in the detail panel both draw this.

use livtk_core::{Kind, State};
use uuid::Uuid;

use super::{Choice, Model, choices};

/// One state of a kind's flow, as seen from the state a node is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub state: State,
    /// On the main path, else off it.
    pub main: bool,
    pub current: bool,
    /// The move there, when the node can make it.
    pub to: Option<Choice>,
    /// For a lesson's Ratified: ratifying as a standing rule instead.
    pub standing: Option<Choice>,
}

impl Step {
    /// A step only the human takes: acknowledging, ratifying, accepting, rejecting, retiring.
    pub fn decides(&self) -> bool {
        self.to.as_ref().is_some_and(|c| c.decides)
    }
}

/// The main path and the states off it, in order.
pub fn flow_of(kind: Kind) -> (&'static [State], &'static [State]) {
    use State::*;
    match kind {
        Kind::Task => (
            &[Backlog, Planned, Acked, Doing, Review, Done],
            &[Parked, Canceled],
        ),
        Kind::Lesson => (&[Proposed, Ratified, Retired], &[Rejected]),
        Kind::Decision => (&[Proposed, Accepted, Superseded], &[Rejected]),
        Kind::Finding => (&[Recorded], &[]),
    }
}

/// Every state of `kind`'s flow, seen from `state`: which is current, which can be reached.
pub fn steps(kind: Kind, state: State) -> Vec<Step> {
    let moves = choices(kind, state);
    let (main, side) = flow_of(kind);
    main.iter()
        .map(|&s| (s, true))
        .chain(side.iter().map(|&s| (s, false)))
        .map(|(s, on_main)| Step {
            state: s,
            main: on_main,
            current: s == state,
            to: moves.iter().find(|c| c.to == s && !c.always).cloned(),
            standing: moves.iter().find(|c| c.to == s && c.always).cloned(),
        })
        .collect()
}

/// Whether a kind has a flow worth drawing: a finding has one state and no moves.
pub fn has_flow(kind: Kind) -> bool {
    flow_of(kind).0.len() > 1
}

/// A state's name, as people say it.
pub fn state_name(state: State) -> &'static str {
    match state {
        State::Backlog => "Backlog",
        State::Planned => "Planned",
        State::Acked => "Acknowledged",
        State::Doing => "Doing",
        State::Review => "In review",
        State::Done => "Done",
        State::Parked => "Parked",
        State::Canceled => "Canceled",
        State::Proposed => "Proposed",
        State::Ratified => "Ratified",
        State::Rejected => "Rejected",
        State::Retired => "Retired",
        State::Accepted => "Accepted",
        State::Superseded => "Superseded",
        State::Recorded => "Recorded",
    }
}

/// A state's name where room is short: the flow in the detail panel.
pub fn state_short(state: State) -> &'static str {
    match state {
        State::Acked => "Acked",
        State::Review => "Review",
        s => state_name(s),
    }
}

impl Model {
    /// Take `choice` on node `id`, wherever it shows (a row, a card, the panel), leaving the
    /// selection where it is.
    pub fn choose_on(&mut self, id: Uuid, choice: &Choice) {
        let cmd = if choice.to == State::Ratified {
            livtk_core::Command::Ratify {
                node: id.to_string(),
                always: choice.always,
            }
        } else {
            livtk_core::Command::Move {
                node: id.to_string(),
                to: choice.to,
            }
        };
        let row = self.rows.iter().find(|r| r.id == id).cloned();
        self.run(cmd, row);
    }
}
