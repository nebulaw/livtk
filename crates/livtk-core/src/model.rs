//! The domain: kinds, states, keys, priorities, relations, actors, and the rules that bind them.
//! Pure: no IO, no database.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Error;

/// What a node is. Every kind shares the same storage and the same event operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Work to do, planned by Claude and acknowledged by the human.
    Task,
    /// A rule learned from experience; binding once the human ratifies it.
    Lesson,
    /// A choice and its reasons; binding once the human accepts it.
    Decision,
    /// Something found out, kept for later; Claude records these freely.
    Finding,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Task, Kind::Lesson, Kind::Decision, Kind::Finding];

    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Task => "task",
            Kind::Lesson => "lesson",
            Kind::Decision => "decision",
            Kind::Finding => "finding",
        }
    }

    /// The key prefix of this kind. Tasks use the project's own prefix instead.
    pub const fn fixed_prefix(self) -> Option<&'static str> {
        match self {
            Kind::Task => None,
            Kind::Lesson => Some("LES"),
            Kind::Decision => Some("DEC"),
            Kind::Finding => Some("FND"),
        }
    }

    /// What recall draws from.
    pub const fn is_memory(self) -> bool {
        !matches!(self, Kind::Task)
    }

    /// Every state a node of this kind can be in.
    pub const fn states(self) -> &'static [State] {
        use State::*;
        match self {
            Kind::Task => &[
                Backlog, Planned, Acked, Doing, Review, Done, Parked, Canceled,
            ],
            Kind::Lesson => &[Proposed, Ratified, Rejected, Retired],
            Kind::Decision => &[Proposed, Accepted, Rejected, Superseded],
            Kind::Finding => &[Recorded],
        }
    }

    pub const fn initial_state(self) -> State {
        match self {
            Kind::Task => State::Backlog,
            Kind::Lesson | Kind::Decision => State::Proposed,
            Kind::Finding => State::Recorded,
        }
    }

    /// The states a node of this kind may move to from `from`.
    pub const fn next_states(self, from: State) -> &'static [State] {
        use State::*;
        match (self, from) {
            (Kind::Task, Backlog) => &[Planned, Parked, Canceled],
            (Kind::Task, Planned) => &[Acked, Backlog, Parked, Canceled],
            (Kind::Task, Acked) => &[Doing, Planned, Parked, Canceled],
            (Kind::Task, Doing) => &[Review, Acked, Parked, Canceled],
            (Kind::Task, Review) => &[Done, Doing],
            (Kind::Task, Done) => &[Doing],
            (Kind::Task, Parked) => &[Backlog, Planned],
            (Kind::Task, Canceled) => &[Backlog],
            (Kind::Lesson, Proposed) => &[Ratified, Rejected],
            (Kind::Lesson, Ratified) => &[Retired],
            (Kind::Lesson, Rejected) => &[Proposed],
            (Kind::Lesson, Retired) => &[Ratified],
            (Kind::Decision, Proposed) => &[Accepted, Rejected],
            (Kind::Decision, Accepted) => &[Superseded],
            (Kind::Decision, Rejected) => &[Proposed],
            _ => &[],
        }
    }

    pub fn can_move(self, from: State, to: State) -> bool {
        self.next_states(from).contains(&to)
    }

    /// Whether only a human may make this move: acknowledging a plan, and deciding what binds
    /// Claude (ratifying, rejecting and retiring lessons; accepting and rejecting decisions).
    pub const fn human_only(self, from: State, to: State) -> bool {
        use State::*;
        matches!(
            (self, from, to),
            (Kind::Task, Planned, Acked)
                | (Kind::Lesson, Proposed, Ratified | Rejected)
                | (Kind::Lesson, Ratified, Retired)
                | (Kind::Lesson, Retired, Ratified)
                | (Kind::Decision, Proposed, Accepted | Rejected)
        )
    }

    /// Where a node goes when its text changes while it binds: what the human approved is no
    /// longer what it says, so it needs approving again.
    pub const fn on_edit(self, state: State) -> Option<State> {
        match (self, state) {
            (Kind::Task, State::Acked) => Some(State::Planned),
            (Kind::Lesson, State::Ratified) | (Kind::Decision, State::Accepted) => {
                Some(State::Proposed)
            }
            _ => None,
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Kind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Kind::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "unknown kind `{s}`; one of: {}",
                    Kind::ALL.map(Kind::as_str).join(", ")
                ))
            })
    }
}

/// Where a node stands. One enum for every kind; each kind uses its own subset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Backlog,
    Planned,
    Acked,
    Doing,
    Review,
    Done,
    Parked,
    Canceled,
    Proposed,
    Ratified,
    Rejected,
    Retired,
    Accepted,
    Superseded,
    Recorded,
}

impl State {
    /// Every state, in display order: what is in hand first, what is over last.
    pub const ALL: [State; 15] = [
        State::Doing,
        State::Review,
        State::Acked,
        State::Planned,
        State::Proposed,
        State::Ratified,
        State::Accepted,
        State::Recorded,
        State::Backlog,
        State::Parked,
        State::Done,
        State::Superseded,
        State::Retired,
        State::Rejected,
        State::Canceled,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            State::Backlog => "backlog",
            State::Planned => "planned",
            State::Acked => "acked",
            State::Doing => "doing",
            State::Review => "review",
            State::Done => "done",
            State::Parked => "parked",
            State::Canceled => "canceled",
            State::Proposed => "proposed",
            State::Ratified => "ratified",
            State::Rejected => "rejected",
            State::Retired => "retired",
            State::Accepted => "accepted",
            State::Superseded => "superseded",
            State::Recorded => "recorded",
        }
    }

    /// Position in [`State::ALL`].
    pub const fn rank(self) -> u8 {
        let mut i = 0;
        while i < State::ALL.len() {
            if State::ALL[i] as u8 == self as u8 {
                return i as u8;
            }
            i += 1;
        }
        u8::MAX
    }

    /// Finished one way or another. A closed dependency no longer blocks.
    pub const fn is_closed(self) -> bool {
        matches!(self, State::Done | State::Canceled)
    }

    /// Set aside without being done: canceled, rejected, retired or superseded. The app hides
    /// these unless asked; done work stays in view.
    pub const fn is_dismissed(self) -> bool {
        matches!(
            self,
            State::Canceled | State::Rejected | State::Retired | State::Superseded
        )
    }

    /// Still part of the picture: shown in lists unless asked for everything.
    pub const fn is_active(self) -> bool {
        !matches!(
            self,
            State::Done | State::Canceled | State::Rejected | State::Retired | State::Superseded
        )
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for State {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        State::ALL
            .into_iter()
            .find(|st| st.as_str() == s)
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "unknown state `{s}`; one of: {}",
                    State::ALL.map(State::as_str).join(", ")
                ))
            })
    }
}

/// 1 urgent, 2 high, 3 medium, 4 low.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Priority(u8);

impl Priority {
    pub const DEFAULT: Priority = Priority(3);

    pub fn new(p: u8) -> Result<Self, Error> {
        if (1..=4).contains(&p) {
            Ok(Priority(p))
        } else {
            Err(Error::Invalid(format!(
                "priority {p} is out of range; use 1 (urgent) to 4 (low)"
            )))
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Priority {
    type Error = Error;

    fn try_from(p: u8) -> Result<Self, Error> {
        Priority::new(p)
    }
}

impl From<Priority> for u8 {
    fn from(p: Priority) -> u8 {
        p.0
    }
}

/// A readable node key such as `LIV-12`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub prefix: String,
    pub number: u64,
}

impl Key {
    pub fn new(prefix: &str, number: u64) -> Self {
        Key {
            prefix: prefix.to_owned(),
            number,
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.prefix, self.number)
    }
}

impl FromStr for Key {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let bad = || Error::Invalid(format!("`{s}` is not a key like LIV-12"));
        let (prefix, number) = s.rsplit_once('-').ok_or_else(bad)?;
        validate_prefix(prefix).map_err(|_| bad())?;
        let number = number.parse::<u64>().map_err(|_| bad())?;
        if number == 0 {
            return Err(bad());
        }
        Ok(Key::new(prefix, number))
    }
}

/// A key prefix: an uppercase letter followed by up to seven uppercase letters or digits.
pub fn validate_prefix(prefix: &str) -> Result<(), Error> {
    let mut chars = prefix.chars();
    let ok = chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && prefix.len() <= 8;
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "prefix `{prefix}` must be an uppercase letter followed by up to 7 letters or digits, like LIV"
        )))
    }
}

/// A prefix a project may choose for its tasks: valid, and not one the other kinds use.
pub fn validate_project_prefix(prefix: &str) -> Result<(), Error> {
    validate_prefix(prefix)?;
    if Kind::ALL.iter().any(|k| k.fixed_prefix() == Some(prefix)) {
        return Err(Error::Invalid(format!(
            "prefix `{prefix}` is reserved for lessons, decisions or findings; pick another"
        )));
    }
    Ok(())
}

/// The default prefix for a project directory: its first three letters or digits, uppercased.
pub fn default_prefix(project_name: &str) -> String {
    let prefix: String = project_name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(3)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if validate_project_prefix(&prefix).is_ok() {
        prefix
    } else {
        "T".to_owned()
    }
}

/// How two nodes relate. `depends_on` points from the waiting node to the one it waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rel {
    DependsOn,
    Relates,
    Supersedes,
    LearnedFrom,
    Parent,
}

impl Rel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Rel::DependsOn => "depends_on",
            Rel::Relates => "relates",
            Rel::Supersedes => "supersedes",
            Rel::LearnedFrom => "learned_from",
            Rel::Parent => "parent",
        }
    }
}

impl fmt::Display for Rel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who made a change. The app is always human; the CLI is Claude when Claude Code runs it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Actor {
    Human,
    Claude { agent: Option<String> },
}

impl Actor {
    pub const fn is_human(&self) -> bool {
        matches!(self, Actor::Human)
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Actor::Human => f.write_str("human"),
            Actor::Claude { agent: None } => f.write_str("claude"),
            Actor::Claude { agent: Some(a) } => write!(f, "claude:{a}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_stay_inside_each_kinds_states() {
        for kind in Kind::ALL {
            assert!(kind.states().contains(&kind.initial_state()), "{kind}");
            for &from in kind.states() {
                for &to in kind.next_states(from) {
                    assert!(kind.states().contains(&to), "{kind}: {from} -> {to}");
                    assert_ne!(from, to, "{kind}: {from} -> itself");
                }
            }
            for from in State::ALL
                .into_iter()
                .filter(|s| !kind.states().contains(s))
            {
                assert!(
                    kind.next_states(from).is_empty(),
                    "{kind} has no state {from}"
                );
            }
        }
    }

    #[test]
    fn the_human_gates_are_exactly_these() {
        use State::*;
        let gates = [
            (Kind::Task, Planned, Acked),
            (Kind::Lesson, Proposed, Ratified),
            (Kind::Lesson, Proposed, Rejected),
            (Kind::Lesson, Ratified, Retired),
            (Kind::Lesson, Retired, Ratified),
            (Kind::Decision, Proposed, Accepted),
            (Kind::Decision, Proposed, Rejected),
        ];
        for kind in Kind::ALL {
            for from in State::ALL {
                for to in State::ALL {
                    let expected = gates.contains(&(kind, from, to));
                    assert_eq!(
                        kind.human_only(from, to),
                        expected,
                        "{kind}: {from} -> {to}"
                    );
                    if expected {
                        assert!(
                            kind.can_move(from, to),
                            "a gate on a move that cannot happen"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn task_states_can_always_return_to_backlog() {
        for &start in Kind::Task.states() {
            let mut seen = vec![start];
            let mut i = 0;
            while i < seen.len() {
                for &n in Kind::Task.next_states(seen[i]) {
                    if !seen.contains(&n) {
                        seen.push(n);
                    }
                }
                i += 1;
            }
            assert!(
                seen.contains(&State::Backlog),
                "{start} cannot return to backlog"
            );
        }
    }

    #[test]
    fn editing_what_binds_asks_for_approval_again() {
        assert_eq!(Kind::Task.on_edit(State::Acked), Some(State::Planned));
        assert_eq!(Kind::Lesson.on_edit(State::Ratified), Some(State::Proposed));
        assert_eq!(
            Kind::Decision.on_edit(State::Accepted),
            Some(State::Proposed)
        );
        assert_eq!(Kind::Task.on_edit(State::Doing), None);
        assert_eq!(Kind::Finding.on_edit(State::Recorded), None);
    }

    #[test]
    fn ranks_follow_the_display_order() {
        for (i, s) in State::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(s.rank()), i, "{s}");
        }
    }

    #[test]
    fn keys_round_trip_and_reject_garbage() {
        let k: Key = "LIV-12".parse().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(k, Key::new("LIV", 12));
        assert_eq!(k.to_string(), "LIV-12");
        for bad in [
            "LIV",
            "liv-1",
            "LIV-0",
            "LIV-x",
            "-3",
            "1LIV-2",
            "TOOLONGPFX-1",
        ] {
            assert!(bad.parse::<Key>().is_err(), "{bad}");
        }
    }

    #[test]
    fn default_prefix_is_safe() {
        assert_eq!(default_prefix("livspeil"), "LIV");
        assert_eq!(default_prefix("my-app"), "MYA");
        assert_eq!(default_prefix("9lives"), "T");
        assert!(
            validate_project_prefix("LES").is_err(),
            "reserved for lessons"
        );
        assert!(
            "LES-3".parse::<Key>().is_ok(),
            "but a lesson key still parses"
        );
        assert_eq!(
            default_prefix("lessons-app"),
            "T",
            "never defaults to a reserved prefix"
        );
        assert_eq!(default_prefix(""), "T");
    }

    #[test]
    fn priority_is_bounded() {
        assert!(Priority::new(0).is_err());
        assert!(Priority::new(5).is_err());
        assert_eq!(Priority::new(1).map(Priority::get).ok(), Some(1));
    }
}
