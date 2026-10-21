//! Signals: what happened in a project, as something outside it (a sub-agent, a notifier, a
//! hook) wants to hear it. Derived from the event log, never stored: the log stays the one
//! truth. A small vocabulary, versioned: `v` only ever grows, a reader ignores fields it does
//! not know.
//!
//! Each signal names the event it comes from, which is its cursor: a reader that keeps the last
//! one it saw reads on from there, in order, at least once, and can tell a repeat by it.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use uuid::Uuid;

use crate::event::{Event, Op};
use crate::model::{Kind, Rel, State};
use crate::workspace::Gate;

/// The vocabulary's version.
pub const VERSION: u32 = 1;

/// Every signal there is.
pub const NAMES: &[&str] = &[
    "node.created",
    "node.moved",
    "node.edited",
    "node.noted",
    "link.added",
    "link.removed",
    "doc.linked",
    "doc.unlinked",
    "decision.needed",
    "decision.made",
    "work.unblocked",
];

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Signal {
    pub v: u32,
    pub signal: &'static str,
    /// The event it comes from: the cursor to read on from.
    pub event: Uuid,
    pub at: String,
    pub by: String,
    pub node: Uuid,
    pub key: String,
    pub kind: Kind,
    #[serde(flatten)]
    pub detail: Detail,
}

/// What a signal says beyond which node.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Detail {
    None {},
    State { state: State },
    Moved { from: State, to: State },
    Edited { fields: Vec<&'static str> },
    Noted { text: String },
    Link { rel: Rel, other: String },
    Doc { path: String },
}

/// The `by` of whatever a hook does through livtk: a hook never hears it (see `on`).
pub const BY_HOOK: &str = "claude:hook";

/// Signals from events, read in log order. It keeps what it needs of each node (its key, kind
/// and state, what it waits for), so the signals of a later event are right whatever came
/// before; feed it the whole log, or the log up to a cursor and then what follows.
pub struct Signals {
    gates: Vec<Gate>,
    nodes: HashMap<Uuid, (String, Kind, State)>,
    /// Each node's dependencies (what it waits for).
    waits: HashMap<Uuid, HashSet<Uuid>>,
}

impl Signals {
    /// For a project with `gates`.
    pub fn new(gates: &[Gate]) -> Signals {
        Signals {
            gates: gates.to_vec(),
            nodes: HashMap::new(),
            waits: HashMap::new(),
        }
    }

    /// Whether a `kind` in `state` waits for the human: a plan to acknowledge, a lesson or a
    /// decision to accept, or what a gate of the project's holds there.
    fn waits_for_human(&self, kind: Kind, state: State) -> bool {
        matches!(
            (kind, state),
            (Kind::Task, State::Planned)
                | (Kind::Lesson, State::Proposed)
                | (Kind::Decision, State::Proposed)
                | (Kind::Project, State::Proposed)
        ) || self
            .gates
            .iter()
            .any(|g| g.kind == kind && g.from == Some(state))
    }

    fn human_only(&self, kind: Kind, from: State, to: State) -> bool {
        kind.human_only(from, to) || self.gates.iter().any(|g| g.holds(kind, from, to))
    }

    /// The signals of `e`, in order.
    pub fn feed(&mut self, e: &Event) -> Vec<Signal> {
        let mut out = Vec::new();
        let signal =
            |signal: &'static str, node: Uuid, key: &str, kind: Kind, detail: Detail| Signal {
                v: VERSION,
                signal,
                event: e.id,
                at: e.at.clone(),
                by: e.by.clone(),
                node,
                key: key.to_owned(),
                kind,
                detail,
            };
        match &e.op {
            Op::NodeCreate { node, data } => {
                self.nodes
                    .insert(*node, (data.key.clone(), data.kind, data.state));
                out.push(signal(
                    "node.created",
                    *node,
                    &data.key,
                    data.kind,
                    Detail::State { state: data.state },
                ));
                if self.waits_for_human(data.kind, data.state) {
                    out.push(signal(
                        "decision.needed",
                        *node,
                        &data.key,
                        data.kind,
                        Detail::State { state: data.state },
                    ));
                }
            }
            Op::NodeSet { node, data } => {
                let Some((key, kind, from)) = self.nodes.get(node).cloned() else {
                    return out;
                };
                let mut fields = Vec::new();
                if data.title.is_some() {
                    fields.push("title");
                }
                if data.body.is_some() {
                    fields.push("body");
                }
                if data.priority.is_some() {
                    fields.push("priority");
                }
                if data.props.is_some() {
                    fields.push("props");
                }
                if !fields.is_empty() {
                    out.push(signal(
                        "node.edited",
                        *node,
                        &key,
                        kind,
                        Detail::Edited { fields },
                    ));
                }
                if let Some(to) = data.state.filter(|to| *to != from) {
                    if let Some(n) = self.nodes.get_mut(node) {
                        n.2 = to;
                    }
                    out.push(signal(
                        "node.moved",
                        *node,
                        &key,
                        kind,
                        Detail::Moved { from, to },
                    ));
                    if self.human_only(kind, from, to) {
                        out.push(signal(
                            "decision.made",
                            *node,
                            &key,
                            kind,
                            Detail::Moved { from, to },
                        ));
                    }
                    if self.waits_for_human(kind, to) {
                        out.push(signal(
                            "decision.needed",
                            *node,
                            &key,
                            kind,
                            Detail::State { state: to },
                        ));
                    }
                    if kind.is_closed(to) && !kind.is_closed(from) {
                        out.extend(self.unblocked(*node).into_iter().map(|(id, k, kind)| {
                            signal("work.unblocked", id, &k, kind, Detail::None {})
                        }));
                    }
                }
            }
            Op::NodeNote { node, data } => {
                if let Some((key, kind, _)) = self.nodes.get(node) {
                    out.push(signal(
                        "node.noted",
                        *node,
                        key,
                        *kind,
                        Detail::Noted {
                            text: data.text.clone(),
                        },
                    ));
                }
            }
            Op::EdgeAdd { data } | Op::EdgeRemove { data } => {
                let added = matches!(e.op, Op::EdgeAdd { .. });
                // Whether it waited on something open before this link changed: only then
                // can taking a dependency away unblock it.
                let was_blocked = self.unblocked_one(data.src).is_none()
                    && self
                        .nodes
                        .get(&data.src)
                        .is_some_and(|n| !n.1.is_closed(n.2));
                if data.rel == Rel::DependsOn {
                    let waits = self.waits.entry(data.src).or_default();
                    if added {
                        waits.insert(data.dst);
                    } else {
                        waits.remove(&data.dst);
                    }
                }
                if let (Some((key, kind, _)), Some((other, ..))) =
                    (self.nodes.get(&data.src), self.nodes.get(&data.dst))
                {
                    out.push(signal(
                        if added { "link.added" } else { "link.removed" },
                        data.src,
                        key,
                        *kind,
                        Detail::Link {
                            rel: data.rel,
                            other: other.clone(),
                        },
                    ));
                }
                if !added && data.rel == Rel::DependsOn && was_blocked {
                    // Taking away the last open thing it waited for unblocks it too.
                    out.extend(self.unblocked_one(data.src).map(|(id, k, kind)| {
                        signal("work.unblocked", id, &k, kind, Detail::None {})
                    }));
                }
            }
            Op::DocLink { node, data } | Op::DocUnlink { node, data } => {
                if let Some((key, kind, _)) = self.nodes.get(node) {
                    let linked = matches!(e.op, Op::DocLink { .. });
                    out.push(signal(
                        if linked { "doc.linked" } else { "doc.unlinked" },
                        *node,
                        key,
                        *kind,
                        Detail::Doc {
                            path: data.path.clone(),
                        },
                    ));
                }
            }
        }
        out
    }

    /// The nodes that waited on `done` and now wait on nothing open.
    fn unblocked(&self, done: Uuid) -> Vec<(Uuid, String, Kind)> {
        let mut out: Vec<_> = self
            .waits
            .iter()
            .filter(|(_, deps)| deps.contains(&done))
            .filter_map(|(id, _)| self.unblocked_one(*id))
            .collect();
        out.sort_by(|a, b| a.1.cmp(&b.1));
        out
    }

    /// `id`, if nothing it waits for is still open and it is itself still open.
    fn unblocked_one(&self, id: Uuid) -> Option<(Uuid, String, Kind)> {
        let (key, kind, state) = self.nodes.get(&id)?;
        if kind.is_closed(*state) {
            return None;
        }
        let open = self
            .waits
            .get(&id)
            .into_iter()
            .flatten()
            .any(|d| self.nodes.get(d).is_some_and(|n| !n.1.is_closed(n.2)));
        (!open).then(|| (id, key.clone(), *kind))
    }
}
