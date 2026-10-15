//! The domain: kinds, states, keys, priorities, relations, actors, and the rules that bind them.
//! Pure: no IO, no database.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Error;

/// What a node is. Every kind shares the same storage and the same five event operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Task,
}

impl Kind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Task => "task",
        }
    }

    pub const fn initial_state(self) -> State {
        match self {
            Kind::Task => State::Backlog,
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
        }
    }

    pub fn can_move(self, from: State, to: State) -> bool {
        self.next_states(from).contains(&to)
    }

    /// Whether only a human may make this move. Acknowledging a plan is the human gate.
    pub const fn human_only(self, from: State, to: State) -> bool {
        matches!((self, from, to), (Kind::Task, State::Planned, State::Acked))
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
        match s {
            "task" => Ok(Kind::Task),
            _ => Err(Error::Invalid(format!(
                "unknown kind `{s}`; this livtk may be older than the data"
            ))),
        }
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
}

impl State {
    pub const ALL: [State; 8] = [
        State::Doing,
        State::Review,
        State::Acked,
        State::Planned,
        State::Backlog,
        State::Parked,
        State::Done,
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
        }
    }

    /// Display order: the work in hand first, the finished last.
    pub const fn rank(self) -> u8 {
        match self {
            State::Doing => 0,
            State::Review => 1,
            State::Acked => 2,
            State::Planned => 3,
            State::Backlog => 4,
            State::Parked => 5,
            State::Done => 6,
            State::Canceled => 7,
        }
    }

    /// Finished one way or another. A closed dependency no longer blocks.
    pub const fn is_closed(self) -> bool {
        matches!(self, State::Done | State::Canceled)
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

/// The default prefix for a project directory: its first three letters or digits, uppercased.
pub fn default_prefix(project_name: &str) -> String {
    let prefix: String = project_name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(3)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if validate_prefix(&prefix).is_ok() {
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
    fn every_listed_move_is_legal_and_nothing_else() {
        for from in State::ALL {
            for to in State::ALL {
                let listed = Kind::Task.next_states(from).contains(&to);
                assert_eq!(Kind::Task.can_move(from, to), listed, "{from} -> {to}");
            }
            assert!(!Kind::Task.can_move(from, from), "{from} -> itself");
        }
    }

    #[test]
    fn only_acknowledging_a_plan_is_human_only() {
        for from in State::ALL {
            for to in State::ALL {
                let expected = from == State::Planned && to == State::Acked;
                assert_eq!(Kind::Task.human_only(from, to), expected, "{from} -> {to}");
            }
        }
    }

    #[test]
    fn every_state_can_reach_every_open_state_again() {
        // No dead ends: from anywhere, some path leads back to backlog.
        for start in State::ALL {
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
        assert_eq!(default_prefix(""), "T");
    }

    #[test]
    fn priority_is_bounded() {
        assert!(Priority::new(0).is_err());
        assert!(Priority::new(5).is_err());
        assert_eq!(Priority::new(1).map(Priority::get).ok(), Some(1));
    }
}
