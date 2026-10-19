//! The domain: kinds, states, keys, priorities, relations, actors, and the rules that bind them.
//! Pure: no IO, no database.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Error;

/// What a node is. Every kind shares the same storage and the same event operations. The four
/// built-in kinds are constants; a project may declare more (`[[kinds]]` in its config), which
/// are registered by name as the config loads and behave by their declaration.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Kind(u16);

#[allow(non_upper_case_globals)]
impl Kind {
    /// Work to do, planned by Claude and acknowledged by the human.
    pub const Task: Kind = Kind(0);
    /// A rule learned from experience; binding once the human ratifies it.
    pub const Lesson: Kind = Kind(1);
    /// A choice and its reasons; binding once the human accepts it.
    pub const Decision: Kind = Kind(2);
    /// Something found out, kept for later; Claude records these freely.
    pub const Finding: Kind = Kind(3);

    /// The built-in kinds.
    pub const ALL: [Kind; 4] = [Kind::Task, Kind::Lesson, Kind::Decision, Kind::Finding];
    const BUILT_IN: u16 = 4;

    /// Every kind known now: the built-in ones, then those declared, in their order.
    pub fn all() -> Vec<Kind> {
        let declared = registry().kinds.len();
        Kind::ALL
            .into_iter()
            .chain((0..declared).map(|i| Kind(Kind::BUILT_IN + i as u16)))
            .collect()
    }

    /// Whether this kind is one a project declared rather than a built-in one.
    pub const fn is_declared(self) -> bool {
        self.0 >= Kind::BUILT_IN
    }

    /// A declared kind's declaration.
    fn declared(self) -> Option<KindDef> {
        let i = usize::from(self.0.checked_sub(Kind::BUILT_IN)?);
        registry().kinds.get(i).cloned()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Task => "task",
            Kind::Lesson => "lesson",
            Kind::Decision => "decision",
            Kind::Finding => "finding",
            _ => self.declared().map_or("unknown", |d| d.name),
        }
    }

    /// The key prefix of this kind. Tasks use the project's own prefix instead.
    pub fn fixed_prefix(self) -> Option<&'static str> {
        match self {
            Kind::Task => None,
            Kind::Lesson => Some("LES"),
            Kind::Decision => Some("DEC"),
            Kind::Finding => Some("FND"),
            _ => self.declared().map(|d| d.key),
        }
    }

    /// What recall draws from: lessons, decisions, findings, and declared kinds that say so.
    pub fn is_memory(self) -> bool {
        match self {
            Kind::Task => false,
            Kind::Lesson | Kind::Decision | Kind::Finding => true,
            _ => self.declared().is_some_and(|d| d.memory),
        }
    }

    /// Every state a node of this kind can be in.
    pub fn states(self) -> &'static [State] {
        use states::*;
        match self {
            Kind::Task => &[
                Backlog, Planned, Acked, Doing, Review, Done, Parked, Canceled,
            ],
            Kind::Lesson => &[Proposed, Ratified, Rejected, Retired],
            Kind::Decision => &[Proposed, Accepted, Rejected, Superseded],
            Kind::Finding => &[Recorded],
            _ => self.declared().map_or(&[], |d| d.states),
        }
    }

    pub fn initial_state(self) -> State {
        match self {
            Kind::Task => State::Backlog,
            Kind::Lesson | Kind::Decision => State::Proposed,
            Kind::Finding => State::Recorded,
            _ => self.declared().map_or(State::Backlog, |d| d.start),
        }
    }

    /// The states a node of this kind may move to from `from`. A declared kind moves between
    /// any two of its states.
    pub fn next_states(self, from: State) -> &'static [State] {
        use states::*;
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
            (k, from) if k.is_declared() => k
                .declared()
                .and_then(|d| d.next.iter().find(|(s, _)| *s == from).map(|(_, n)| *n))
                .unwrap_or(&[]),
            _ => &[],
        }
    }

    pub fn can_move(self, from: State, to: State) -> bool {
        self.next_states(from).contains(&to)
    }

    /// Whether only a human may make this move: acknowledging a plan, and deciding what binds
    /// Claude (ratifying, rejecting and retiring lessons; accepting and rejecting decisions);
    /// for a declared kind, the moves its declaration keeps for the human.
    pub fn human_only(self, from: State, to: State) -> bool {
        use states::*;
        if self.is_declared() {
            return self
                .declared()
                .is_some_and(|d| d.human.contains(&(from, to)));
        }
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
    pub fn on_edit(self, state: State) -> Option<State> {
        match (self, state) {
            // The human approved the plan as it was: changed while acknowledged, under way or
            // in review, it goes back to planned for a fresh acknowledgement.
            (Kind::Task, State::Acked | State::Doing | State::Review) => Some(State::Planned),
            (Kind::Lesson, State::Ratified) | (Kind::Decision, State::Accepted) => {
                Some(State::Proposed)
            }
            _ => None,
        }
    }

    /// The kind named `name`, built-in or declared.
    pub fn named(name: &str) -> Option<Kind> {
        Kind::ALL
            .into_iter()
            .find(|k| k.as_str() == name)
            .or_else(|| {
                registry()
                    .kinds
                    .iter()
                    .position(|d| d.name == name)
                    .map(|i| Kind(Kind::BUILT_IN + i as u16))
            })
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Kind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Kind::named(s).ok_or_else(|| {
            Error::Invalid(format!(
                "unknown kind `{s}`; one of: {}",
                Kind::all()
                    .into_iter()
                    .map(Kind::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
    }
}

impl Serialize for Kind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        name.parse().map_err(serde::de::Error::custom)
    }
}

/// Where a node stands. The built-in states are constants; a declared kind's states are
/// registered by name. Each kind uses its own subset.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct State(u16);

#[allow(non_upper_case_globals)]
impl State {
    pub const Backlog: State = State(0);
    pub const Planned: State = State(1);
    pub const Acked: State = State(2);
    pub const Doing: State = State(3);
    pub const Review: State = State(4);
    pub const Done: State = State(5);
    pub const Parked: State = State(6);
    pub const Canceled: State = State(7);
    pub const Proposed: State = State(8);
    pub const Ratified: State = State(9);
    pub const Rejected: State = State(10);
    pub const Retired: State = State(11);
    pub const Accepted: State = State(12);
    pub const Superseded: State = State(13);
    pub const Recorded: State = State(14);
    const BUILT_IN: u16 = 15;

    /// Every built-in state, in display order: what is in hand first, what is over last.
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

    const NAMES: [&'static str; 15] = [
        "backlog",
        "planned",
        "acked",
        "doing",
        "review",
        "done",
        "parked",
        "canceled",
        "proposed",
        "ratified",
        "rejected",
        "retired",
        "accepted",
        "superseded",
        "recorded",
    ];

    pub fn as_str(self) -> &'static str {
        match State::NAMES.get(usize::from(self.0)) {
            Some(name) => name,
            None => registry()
                .states
                .get(usize::from(self.0 - State::BUILT_IN))
                .copied()
                .unwrap_or("unknown"),
        }
    }

    /// Whether this state belongs to a declared kind.
    pub const fn is_declared(self) -> bool {
        self.0 >= State::BUILT_IN
    }

    /// Position in [`State::ALL`]; a declared state after every built-in one, in the order
    /// it was declared.
    pub fn rank(self) -> u8 {
        if self.is_declared() {
            return u8::try_from(State::ALL.len() + usize::from(self.0 - State::BUILT_IN))
                .unwrap_or(u8::MAX);
        }
        State::ALL
            .iter()
            .position(|s| *s == self)
            .map_or(u8::MAX, |i| i as u8)
    }

    /// Finished one way or another. A closed dependency no longer blocks. A declared state is
    /// closed when a declared kind says so.
    pub fn is_closed(self) -> bool {
        if self.is_declared() {
            return registry().kinds.iter().any(|d| d.closed.contains(&self));
        }
        matches!(self, State::Done | State::Canceled)
    }

    /// Set aside without being done: canceled, rejected, retired or superseded. The app hides
    /// these unless asked; done work stays in view.
    pub fn is_dismissed(self) -> bool {
        matches!(
            self,
            State::Canceled | State::Rejected | State::Retired | State::Superseded
        )
    }

    /// Still part of the picture: shown in lists unless asked for everything.
    pub fn is_active(self) -> bool {
        if self.is_declared() {
            return !self.is_closed();
        }
        !matches!(
            self,
            State::Done | State::Canceled | State::Rejected | State::Retired | State::Superseded
        )
    }

    /// The state named `name`, built-in or declared.
    pub fn named(name: &str) -> Option<State> {
        State::NAMES
            .iter()
            .position(|n| *n == name)
            .map(|i| State(i as u16))
            .or_else(|| {
                registry()
                    .states
                    .iter()
                    .position(|n| *n == name)
                    .map(|i| State(State::BUILT_IN + i as u16))
            })
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for State {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        State::named(s).ok_or_else(|| {
            Error::Invalid(format!(
                "unknown state `{s}`; one of: {}",
                State::ALL.map(State::as_str).join(", ")
            ))
        })
    }
}

impl Serialize for State {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for State {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        name.parse().map_err(serde::de::Error::custom)
    }
}

/// The built-in states by their bare names, for the rules' tables.
#[allow(non_upper_case_globals, dead_code)]
mod states {
    use super::State;
    pub const Backlog: State = State::Backlog;
    pub const Planned: State = State::Planned;
    pub const Acked: State = State::Acked;
    pub const Doing: State = State::Doing;
    pub const Review: State = State::Review;
    pub const Done: State = State::Done;
    pub const Parked: State = State::Parked;
    pub const Canceled: State = State::Canceled;
    pub const Proposed: State = State::Proposed;
    pub const Ratified: State = State::Ratified;
    pub const Rejected: State = State::Rejected;
    pub const Retired: State = State::Retired;
    pub const Accepted: State = State::Accepted;
    pub const Superseded: State = State::Superseded;
    pub const Recorded: State = State::Recorded;
}

// ---- declared kinds ---------------------------------------------------------------------------

/// A kind a project declares in its config (`[[kinds]]`), as written there.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KindSpec {
    pub name: String,
    pub states: Vec<String>,
    /// Where a new node starts: the first state unless given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    /// The states that mean it is over: the last one unless given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed: Option<Vec<String>>,
    /// Moves only the human may make, each `from -> to`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub human: Vec<String>,
    /// The key prefix of its nodes, as `INC` in `INC-3`.
    pub key: String,
    /// Whether recall draws from it, as from lessons, decisions and findings.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub memory: bool,
}

/// A declared kind as the rules read it.
#[derive(Clone, Debug)]
struct KindDef {
    name: &'static str,
    states: &'static [State],
    start: State,
    closed: &'static [State],
    human: Vec<(State, State)>,
    /// From each state, every other one.
    next: &'static [(State, &'static [State])],
    key: &'static str,
    memory: bool,
}

#[derive(Default)]
struct Registry {
    kinds: Vec<KindDef>,
    states: Vec<&'static str>,
    /// Declared relations: each name, and what it reads as from its other end.
    rels: Vec<(&'static str, Option<&'static str>)>,
}

static REGISTRY: std::sync::RwLock<Registry> = std::sync::RwLock::new(Registry {
    kinds: Vec::new(),
    states: Vec::new(),
    rels: Vec::new(),
});

fn registry() -> std::sync::RwLockReadGuard<'static, Registry> {
    REGISTRY
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Text for the life of the process: declared names live as long as the kinds they name.
fn forever<T>(v: Vec<T>) -> &'static [T] {
    Box::leak(v.into_boxed_slice())
}

/// Register the kinds a project declares, checked: each name new (not a built-in kind), its
/// states distinct and named, its start, closed states and human moves among them, its key a
/// valid prefix no other kind uses. A kind declared again (another project, or the same one read
/// again) takes its newest declaration.
pub fn declare_kinds(specs: &[KindSpec]) -> Result<(), Error> {
    let bad = |name: &str, msg: String| Error::Invalid(format!("[[kinds]] `{name}`: {msg}"));
    let mut seen_names = Vec::new();
    let mut seen_keys = vec!["LES", "DEC", "FND"];
    for spec in specs {
        let name = spec.name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return Err(bad(
                name,
                "a name is lowercase letters and underscores, like `incident`".into(),
            ));
        }
        if Kind::ALL.iter().any(|k| k.as_str() == name) || seen_names.contains(&name) {
            return Err(bad(name, "that kind exists already".into()));
        }
        seen_names.push(name);
        if spec.states.is_empty() {
            return Err(bad(name, "it needs at least one state".into()));
        }
        for (i, st) in spec.states.iter().enumerate() {
            if st.is_empty() || !st.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                return Err(bad(
                    name,
                    format!("state `{st}`: lowercase letters and underscores"),
                ));
            }
            if spec.states[..i].contains(st) {
                return Err(bad(name, format!("state `{st}` is listed twice")));
            }
        }
        let known = |st: &str| spec.states.iter().any(|s| s == st);
        if let Some(start) = &spec.start
            && !known(start)
        {
            return Err(bad(
                name,
                format!("start `{start}` is not one of its states"),
            ));
        }
        for c in spec.closed.iter().flatten() {
            if !known(c) {
                return Err(bad(name, format!("closed `{c}` is not one of its states")));
            }
        }
        for m in &spec.human {
            let Some((from, to)) = m.split_once("->").map(|(a, b)| (a.trim(), b.trim())) else {
                return Err(bad(
                    name,
                    format!("human `{m}`: write a move as `from -> to`"),
                ));
            };
            if !known(from) || !known(to) || from == to {
                return Err(bad(
                    name,
                    format!("human `{m}`: not a move between two of its states"),
                ));
            }
        }
        validate_prefix(&spec.key).map_err(|e| bad(name, e.to_string()))?;
        if seen_keys.contains(&spec.key.as_str()) {
            return Err(bad(name, format!("key `{}` is another kind's", spec.key)));
        }
        seen_keys.push(&spec.key);
    }
    let mut reg = REGISTRY
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for spec in specs {
        let mut state = |n: &str| -> State {
            if let Some(i) = State::NAMES.iter().position(|b| *b == n) {
                return State(i as u16);
            }
            match reg.states.iter().position(|s| *s == n) {
                Some(i) => State(State::BUILT_IN + i as u16),
                None => {
                    reg.states.push(Box::leak(n.to_owned().into_boxed_str()));
                    State(State::BUILT_IN + (reg.states.len() - 1) as u16)
                }
            }
        };
        let states: Vec<State> = spec.states.iter().map(|s| state(s)).collect();
        let start = spec.start.as_deref().map_or(states[0], &mut state);
        let closed: Vec<State> = match &spec.closed {
            Some(c) => c.iter().map(|s| state(s)).collect(),
            None => vec![states[states.len() - 1]],
        };
        let human: Vec<(State, State)> = spec
            .human
            .iter()
            .filter_map(|m| m.split_once("->"))
            .map(|(a, b)| (state(a.trim()), state(b.trim())))
            .collect();
        let states = forever(states);
        let next = forever(
            states
                .iter()
                .map(|&from| {
                    let others: Vec<State> =
                        states.iter().copied().filter(|s| *s != from).collect();
                    (from, forever(others))
                })
                .collect(),
        );
        let def = KindDef {
            name: Box::leak(spec.name.trim().to_owned().into_boxed_str()),
            states,
            start,
            closed: forever(closed),
            human,
            next,
            key: Box::leak(spec.key.clone().into_boxed_str()),
            memory: spec.memory,
        };
        match reg.kinds.iter().position(|d| d.name == def.name) {
            Some(i) => reg.kinds[i] = def,
            None => reg.kinds.push(def),
        }
    }
    Ok(())
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
    if Kind::all().iter().any(|k| k.fixed_prefix() == Some(prefix)) {
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

/// How two nodes relate. `depends_on` points from the waiting node to the one it waits for. The
/// five built-in relations are constants; a project may declare more (`[[relations]]`), each a
/// name and, optionally, the name it reads by from its other end.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rel(u16);

#[allow(non_upper_case_globals)]
impl Rel {
    pub const DependsOn: Rel = Rel(0);
    pub const Relates: Rel = Rel(1);
    pub const Supersedes: Rel = Rel(2);
    pub const LearnedFrom: Rel = Rel(3);
    pub const Parent: Rel = Rel(4);
    /// The built-in relations.
    pub const ALL: [Rel; 5] = [
        Rel::DependsOn,
        Rel::Relates,
        Rel::Supersedes,
        Rel::LearnedFrom,
        Rel::Parent,
    ];
    const BUILT_IN: u16 = 5;
    const NAMES: [&'static str; 5] = [
        "depends_on",
        "relates",
        "supersedes",
        "learned_from",
        "parent",
    ];

    pub fn as_str(self) -> &'static str {
        match Rel::NAMES.get(usize::from(self.0)) {
            Some(n) => n,
            None => self.declared().map_or("unknown", |d| d.0),
        }
    }

    /// Whether a project declared it.
    pub const fn is_declared(self) -> bool {
        self.0 >= Rel::BUILT_IN
    }

    fn declared(self) -> Option<(&'static str, Option<&'static str>)> {
        let i = usize::from(self.0.checked_sub(Rel::BUILT_IN)?);
        registry().rels.get(i).copied()
    }

    /// What a declared relation reads as from its other end (`implemented_by`), if it says.
    pub fn inverse(self) -> Option<&'static str> {
        self.declared().and_then(|d| d.1)
    }

    /// The relations the project declares, in their order.
    pub fn declared_all() -> Vec<Rel> {
        (0..registry().rels.len())
            .map(|i| Rel(Rel::BUILT_IN + i as u16))
            .collect()
    }

    /// The relation named `name`, built-in or declared.
    pub fn named(name: &str) -> Option<Rel> {
        Rel::NAMES
            .iter()
            .position(|n| *n == name)
            .map(|i| Rel(i as u16))
            .or_else(|| {
                registry()
                    .rels
                    .iter()
                    .position(|d| d.0 == name)
                    .map(|i| Rel(Rel::BUILT_IN + i as u16))
            })
    }
}

impl fmt::Display for Rel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Rel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Rel {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Rel::named(s).ok_or_else(|| Error::Invalid(format!("unknown relation `{s}`")))
    }
}

impl Serialize for Rel {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Rel {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        name.parse().map_err(serde::de::Error::custom)
    }
}

/// A relation a project declares in its config (`[[relations]]`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RelSpec {
    pub name: String,
    /// What it reads as from its other end: `implemented_by` for `implements`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inverse: Option<String>,
}

/// Register the relations a project declares, checked: each name new (no built-in, no kind of
/// relation declared twice), lowercase and underscores, its inverse too.
pub fn declare_relations(specs: &[RelSpec]) -> Result<(), Error> {
    let word = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    let mut seen: Vec<&str> = Rel::NAMES.to_vec();
    for spec in specs {
        let bad = |msg: String| Error::Invalid(format!("[[relations]] `{}`: {msg}", spec.name));
        for name in std::iter::once(spec.name.as_str()).chain(spec.inverse.as_deref()) {
            if !word(name) {
                return Err(bad(format!(
                    "`{name}`: lowercase letters and underscores, like `implements`"
                )));
            }
            if seen.contains(&name) {
                return Err(bad(format!("`{name}` is another relation's name")));
            }
            seen.push(name);
        }
    }
    let mut reg = REGISTRY
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for spec in specs {
        let name: &'static str = Box::leak(spec.name.clone().into_boxed_str());
        let inverse = spec
            .inverse
            .as_ref()
            .map(|i| &*Box::leak(i.clone().into_boxed_str()));
        match reg.rels.iter().position(|d| d.0 == name) {
            Some(i) => reg.rels[i] = (name, inverse),
            None => reg.rels.push((name, inverse)),
        }
    }
    Ok(())
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
        use states::*;
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
        assert_eq!(Kind::Task.on_edit(State::Doing), Some(State::Planned));
        assert_eq!(Kind::Task.on_edit(State::Review), Some(State::Planned));
        assert_eq!(Kind::Task.on_edit(State::Backlog), None);
        assert_eq!(Kind::Task.on_edit(State::Done), None);
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
