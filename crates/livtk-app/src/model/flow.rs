//! A kind's states as a flow: the main path left to right, the states off it (parked, canceled,
//! rejected) beside it, and for a node in a given state which of them it can move to. The state
//! menu on rows and cards and the flow in the detail panel both draw this.

use livtk_core::{Gate, Kind, State};
use uuid::Uuid;

use super::{Choice, Model, choices_with};

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

/// How a step is drawn: where the node is, a move waiting for the human's decision, a move
/// anyone can make, or out of reach from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Current,
    Decision,
    Move,
    Out,
}

impl Step {
    /// A step only the human takes: acknowledging, ratifying, accepting, rejecting, retiring.
    pub fn decides(&self) -> bool {
        self.to.as_ref().is_some_and(|c| c.decides)
    }

    pub fn look(&self) -> Look {
        if self.current {
            Look::Current
        } else if self.decides() {
            Look::Decision
        } else if self.to.is_some() || self.standing.is_some() {
            Look::Move
        } else {
            Look::Out
        }
    }
}

/// The main path and the states off it, in order.
pub fn flow_of(kind: Kind) -> (&'static [State], &'static [State]) {
    match kind {
        Kind::Task => (
            &[
                State::Backlog,
                State::Planned,
                State::Acked,
                State::Doing,
                State::Review,
                State::Done,
            ],
            &[State::Parked, State::Canceled],
        ),
        Kind::Lesson => (
            &[State::Proposed, State::Ratified, State::Retired],
            &[State::Rejected],
        ),
        Kind::Decision => (
            &[State::Proposed, State::Accepted, State::Superseded],
            &[State::Rejected],
        ),
        Kind::Finding => (&[State::Recorded], &[]),
        // A declared kind's states, in the order it declares them.
        _ => (kind.states(), &[]),
    }
}

/// Every state of `kind`'s flow, seen from `state`: which is current, which can be reached.
pub fn steps(kind: Kind, state: State) -> Vec<Step> {
    steps_with(kind, state, &[])
}

/// `steps`, with the gates a project's config adds marked as the human's decisions.
pub fn steps_with(kind: Kind, state: State, gates: &[Gate]) -> Vec<Step> {
    let moves = choices_with(kind, state, gates);
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

/// An arrow between two neighbouring states of the main path, as seen from where the node is:
/// lit, pointing the way a move from here goes, or idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrow {
    Idle,
    /// Towards the end of the path, by a move that looks like this.
    Forward(Look),
    /// Back towards the start.
    Back(Look),
}

/// The arrows between the main path's states, one for each gap: those a move from `state`
/// passes along are lit and point its way, the human's decision winning where moves overlap.
pub fn arrows(kind: Kind, state: State) -> Vec<Arrow> {
    arrows_with(kind, state, &[])
}

/// `arrows`, with the gates a project's config adds.
pub fn arrows_with(kind: Kind, state: State, gates: &[Gate]) -> Vec<Arrow> {
    let all = steps_with(kind, state, gates);
    let main: Vec<&Step> = all.iter().filter(|s| s.main).collect();
    let mut out = vec![Arrow::Idle; main.len().saturating_sub(1)];
    let Some(here) = main.iter().position(|s| s.current) else {
        // Off the path (parked, canceled, rejected): its moves leave from the row below.
        return out;
    };
    for (to, s) in main.iter().enumerate() {
        let look = s.look();
        if !matches!(look, Look::Move | Look::Decision) {
            continue;
        }
        let (gaps, arrow) = if to > here {
            (here..to, Arrow::Forward(look))
        } else {
            (to..here, Arrow::Back(look))
        };
        for g in gaps {
            if !matches!(
                out[g],
                Arrow::Forward(Look::Decision) | Arrow::Back(Look::Decision)
            ) {
                out[g] = arrow;
            }
        }
    }
    out
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
        // A declared state's own name, as a heading.
        _ => titled(state.as_str()),
    }
}

/// `name` with a capital, spaces for underscores: a declared state's name as people say it.
fn titled(name: &'static str) -> &'static str {
    static TITLES: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<&'static str, &'static str>>,
    > = std::sync::LazyLock::new(Default::default);
    let mut titles = TITLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    titles.entry(name).or_insert_with(|| {
        let text = name.replace('_', " ");
        let mut c = text.chars();
        let t: String = c
            .next()
            .map(|f| f.to_uppercase().chain(c).collect())
            .unwrap_or_default();
        Box::leak(t.into_boxed_str())
    })
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
    /// selection where it is; whether it moved.
    pub fn choose_on(&mut self, id: Uuid, choice: &Choice) -> bool {
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
        let row = self.pane().rows.iter().find(|r| r.id == id).cloned();
        self.run(cmd, row)
    }
}
