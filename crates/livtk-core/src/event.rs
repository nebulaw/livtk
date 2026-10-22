//! The event format: one JSON object per line of `events.jsonl`. This is the durable truth, so the
//! format is versioned and every change to it must stay readable by older and newer versions.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::model::{Actor, Health, Kind, Priority, Rel, State};
pub use crate::run::Stage;

/// The newest format version this livtk reads. Each event carries the version its operation
/// needs, so adding an operation never changes the bytes of older events, and an older livtk
/// skips the new operation with a clear "update livtk" warning.
pub const FORMAT_VERSION: u32 = 8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub v: u32,
    /// A uuidv7: time-ordered, unique across branches. Replay order is id order.
    pub id: Uuid,
    /// The id's own timestamp, for people reading the log.
    pub at: String,
    pub by: String,
    /// The Claude Code session it was written from, when Claude wrote it; never the human's.
    /// Optional, so an older livtk reads past it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(flatten)]
    pub op: Op,
}

/// Seven operations cover every kind of node and its links: to other nodes, and to docs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum Op {
    #[serde(rename = "node.create")]
    NodeCreate { node: Uuid, data: NodeData },
    #[serde(rename = "node.set")]
    NodeSet { node: Uuid, data: NodePatch },
    /// A remark in a node's history: what was tried, what failed. Changes no state.
    #[serde(rename = "node.note")]
    NodeNote { node: Uuid, data: NoteData },
    #[serde(rename = "edge.add")]
    EdgeAdd { data: EdgeData },
    #[serde(rename = "edge.remove")]
    EdgeRemove { data: EdgeData },
    /// A node linked to a doc: a file in the project, by its path from the project's folder.
    #[serde(rename = "doc.link")]
    DocLink { node: Uuid, data: DocData },
    #[serde(rename = "doc.unlink")]
    DocUnlink { node: Uuid, data: DocData },
    /// Earlier events on a node, written before livtk recorded sessions, credited to the session
    /// they came from. Changes no state.
    #[serde(rename = "node.session")]
    NodeSession { node: Uuid, data: SessionData },
}

impl Op {
    pub const fn name(&self) -> &'static str {
        match self {
            Op::NodeCreate { .. } => "node.create",
            Op::NodeSet { .. } => "node.set",
            Op::NodeNote { .. } => "node.note",
            Op::EdgeAdd { .. } => "edge.add",
            Op::EdgeRemove { .. } => "edge.remove",
            Op::DocLink { .. } => "doc.link",
            Op::DocUnlink { .. } => "doc.unlink",
            Op::NodeSession { .. } => "node.session",
        }
    }

    /// The format version a reader needs to understand this operation: 1 for tasks, 2 for
    /// notes, 3 for lessons, decisions and findings and the states they use, 4 for docs' links.
    pub fn min_version(&self) -> u32 {
        let task_state = |s: State| Kind::Task.states().contains(&s);
        let project_state = |s: State| matches!(s, State::Active | State::Paused | State::Dropped);
        match self {
            // A note of a run of Claude on the node: new in 8.
            Op::NodeNote { data, .. } if data.run.is_some() => 8,
            // A claim of earlier events for a session: new in 7.
            Op::NodeSession { .. } => 7,
            // A project (the outcome above tasks), its states and its health: new in 6.
            Op::NodeCreate { data, .. } if data.kind == Kind::Project => 6,
            Op::NodeSet { data, .. } if data.state.is_some_and(project_state) => 6,
            Op::NodeNote { data, .. } if data.health.is_some() => 6,
            // A project's own kind or state: an older livtk knows neither.
            Op::NodeCreate { data, .. } if data.kind.is_declared() => 5,
            Op::EdgeAdd { data } | Op::EdgeRemove { data } if data.rel.is_declared() => 5,
            Op::NodeSet { data, .. } if data.state.is_some_and(State::is_declared) => 5,
            Op::NodeCreate { data, .. } if data.kind != Kind::Task => 3,
            Op::NodeSet { data, .. } if data.state.is_some_and(|s| !task_state(s)) => 3,
            Op::NodeNote { .. } => 2,
            Op::DocLink { .. } | Op::DocUnlink { .. } => 4,
            _ => 1,
        }
    }

    /// The node an event is about, and for edges the node on the other end.
    pub const fn subjects(&self) -> (Uuid, Option<Uuid>) {
        match self {
            Op::NodeCreate { node, .. }
            | Op::NodeSet { node, .. }
            | Op::NodeNote { node, .. }
            | Op::DocLink { node, .. }
            | Op::DocUnlink { node, .. }
            | Op::NodeSession { node, .. } => (*node, None),
            Op::EdgeAdd { data } | Op::EdgeRemove { data } => (data.src, Some(data.dst)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeData {
    pub kind: Kind,
    pub key: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    pub state: State,
    pub priority: Priority,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub props: Map<String, Value>,
}

/// A partial update. Only the fields present change; the latest event wins per field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NodePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<State>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// Merged key by key into the node's props; a `null` value removes the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<Map<String, Value>>,
}

impl NodePatch {
    pub const fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.body.is_none()
            && self.state.is_none()
            && self.priority.is_none()
            && self.props.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteData {
    pub text: String,
    /// On a project's update: how it stands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<Health>,
    /// On a run of Claude on the node: which run, and what the note marks in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunData>,
}

/// A run's note: the session it is, the agent it runs as and the stage of the work, and what the
/// note marks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunData {
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub stage: Stage,
    #[serde(flatten)]
    pub mark: RunMark,
}

/// What a run's note marks: it started; it waits for you (a question in the note's text, or the
/// commands it was refused and asks to run); your answer to that; or how it ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mark", rename_all = "snake_case")]
pub enum RunMark {
    Started,
    Asks {
        /// The tools it was refused, as Claude Code's permission rules name them
        /// (`Bash(cargo test)`); none for a question.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allow: Vec<String>,
    },
    /// The human let it run what it asked for.
    Allowed,
    /// The human refused what it asked for.
    Denied,
    Ended {
        how: Ending,
    },
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ending {
    /// Its stage's work is done.
    Done,
    /// It stopped on an error, or its process went without a word.
    Failed,
    /// Stopped by hand, the work kept: it can be started again.
    Stopped,
    /// Stopped when the app quit: offered to continue on the next start.
    Quit,
}

impl Ending {
    /// As people say it.
    pub const fn said(self) -> &'static str {
        match self {
            Ending::Done => "done",
            Ending::Failed => "failed",
            Ending::Stopped => "stopped",
            Ending::Quit => "stopped by a quit",
        }
    }
}

/// A session's claim to earlier events: the session, and the events it wrote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionData {
    pub session: String,
    pub events: Vec<Uuid>,
}

/// A doc, by its path from the project's folder, `/` between its parts: `docs/guide/setup.md`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocData {
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeData {
    pub src: Uuid,
    pub dst: Uuid,
    pub rel: Rel,
}

impl Event {
    /// A new event whose id sorts after `after`, so the log stays in id order on this machine.
    pub fn new(after: Option<Uuid>, by: &Actor, op: Op) -> Event {
        let id = next_id(after);
        Event {
            v: op.min_version(),
            id,
            at: timestamp_of(id),
            by: by.to_string(),
            session: None,
            op,
        }
    }

    /// The event as one line of JSON, without the newline. Field order is fixed, so the same
    /// event always gives the same bytes.
    pub fn to_line(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

/// What reading one line produced.
#[derive(Debug)]
pub enum Line {
    Event(Box<Event>),
    /// Readable, but written by a newer livtk. Skipped, never an error.
    Newer {
        id: Option<Uuid>,
        v: u64,
    },
    /// Not an event at all. Skipped with a warning.
    Garbage(String),
    /// An event naming a kind, state or relation the project's config does not declare (any
    /// more). Never skipped: that would quietly lose the node or its move.
    Undeclared {
        why: String,
        /// The node it is about: its key when the line creates it, its id otherwise.
        node: Option<String>,
    },
}

pub fn parse_line(line: &str) -> Line {
    match serde_json::from_str::<Event>(line) {
        Ok(e) if e.v <= FORMAT_VERSION => Line::Event(Box::new(e)),
        Ok(e) => Line::Newer {
            id: Some(e.id),
            v: u64::from(e.v),
        },
        Err(err) => {
            // A newer writer may use an op or a field value this version does not know.
            let header = serde_json::from_str::<Value>(line).ok();
            let v = header
                .as_ref()
                .and_then(|h| h.get("v"))
                .and_then(Value::as_u64);
            let why = err.to_string();
            let undeclared = ["unknown kind", "unknown state", "unknown relation"]
                .iter()
                .any(|w| why.contains(w));
            match v {
                Some(v) if undeclared && v <= u64::from(FORMAT_VERSION) => {
                    // Just what is unknown: not the list of what is known, nor serde's place.
                    let why = why.split(';').next().unwrap_or(&why).to_owned();
                    let h = header.as_ref();
                    let node = h
                        .and_then(|h| h.get("data"))
                        .and_then(|d| d.get("key"))
                        .or_else(|| h.and_then(|h| h.get("node")))
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    Line::Undeclared { why, node }
                }
                Some(v) if v > u64::from(FORMAT_VERSION) => Line::Newer {
                    id: header
                        .as_ref()
                        .and_then(|h| h.get("id"))
                        .and_then(Value::as_str)
                        .and_then(|s| s.parse().ok()),
                    v,
                },
                _ => Line::Garbage(why),
            }
        }
    }
}

/// A fresh uuidv7 that is strictly greater than `after`.
pub fn next_id(after: Option<Uuid>) -> Uuid {
    let id = Uuid::now_v7();
    match after {
        Some(last) if id <= last => bump(last),
        _ => id,
    }
}

// uuidv7 layout, from the most significant bit: 48 bits of unix milliseconds, 4 version bits,
// 12 bits rand_a, 2 variant bits, 62 bits rand_b. Bumping counts up through rand_b, then rand_a,
// then the millisecond, and never touches the version or variant bits.
const RAND_B: u128 = (1 << 62) - 1;
const RAND_A: u128 = 0xFFF << 64;

fn bump(last: Uuid) -> Uuid {
    let x = last.as_u128();
    if x & RAND_B != RAND_B {
        Uuid::from_u128(x + 1)
    } else if x & RAND_A != RAND_A {
        Uuid::from_u128((x & !RAND_B) + (1 << 64))
    } else {
        let ms = u64::try_from(x >> 80).unwrap_or(u64::MAX);
        uuid::Builder::from_unix_timestamp_millis(ms.saturating_add(1), &[0; 10]).into_uuid()
    }
}

/// The RFC 3339 time (UTC, milliseconds) a uuidv7 carries.
pub fn timestamp_of(id: Uuid) -> String {
    let ms = id.get_timestamp().map_or(0, |t| {
        let (secs, nanos) = t.to_unix();
        i64::try_from(secs)
            .unwrap_or(i64::MAX / 1000)
            .saturating_mul(1000)
            .saturating_add(i64::from(nanos / 1_000_000))
    });
    jiff::Timestamp::from_millisecond(ms)
        .map(|t| format!("{t:.3}"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Event {
        Event::new(
            None,
            &Actor::Human,
            Op::NodeSet {
                node: Uuid::now_v7(),
                data: NodePatch {
                    state: Some(State::Acked),
                    ..NodePatch::default()
                },
            },
        )
    }

    #[test]
    fn line_round_trips_with_a_fixed_shape() {
        let e = sample();
        let line = e.to_line().unwrap();
        assert!(line.starts_with(r#"{"v":1,"id":""#), "{line}");
        assert!(line.contains(r#""op":"node.set","node":""#), "{line}");
        assert!(line.ends_with(r#""data":{"state":"acked"}}"#), "{line}");
        match parse_line(&line) {
            Line::Event(back) => assert_eq!(*back, e),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn newer_versions_and_unknown_ops_are_skipped_not_fatal() {
        let id = Uuid::now_v7();
        let newer =
            format!(r#"{{"v":9,"id":"{id}","at":"x","by":"human","op":"node.fly","data":{{}}}}"#);
        assert!(matches!(parse_line(&newer), Line::Newer { v: 9, id: Some(i) } if i == id));
        assert!(matches!(parse_line("not json"), Line::Garbage(_)));
        assert!(matches!(parse_line(r#"{"v":1}"#), Line::Garbage(_)));
    }

    #[test]
    fn ids_always_move_forward() {
        // Far in the future: every fresh id is behind it, so each one must be bumped.
        let mut last =
            uuid::Builder::from_unix_timestamp_millis(u64::MAX >> 17, &[0xFF; 10]).into_uuid();
        for _ in 0..1000 {
            let next = next_id(Some(last));
            assert!(next > last);
            assert_eq!(next.get_version_num(), 7);
            assert_eq!(next.get_variant(), uuid::Variant::RFC4122);
            last = next;
        }
    }

    #[test]
    fn bump_carries_through_every_field() {
        let full_b = uuid::Builder::from_unix_timestamp_millis(5, &[0; 10])
            .into_uuid()
            .as_u128()
            | RAND_B;
        let carried = bump(Uuid::from_u128(full_b));
        assert!(carried.as_u128() > full_b);
        assert_eq!(carried.get_version_num(), 7);

        let full_ab = full_b | RAND_A;
        let next_ms = bump(Uuid::from_u128(full_ab));
        assert!(next_ms.as_u128() > full_ab);
        assert_eq!(next_ms.get_version_num(), 7);
        assert_eq!(next_ms.get_timestamp().map(|t| t.to_unix().0), Some(0));
    }

    #[test]
    fn timestamps_come_from_the_id() {
        let id = uuid::Builder::from_unix_timestamp_millis(1_760_000_000_123, &[0; 10]).into_uuid();
        assert_eq!(timestamp_of(id), "2025-10-09T08:53:20.123Z");
    }
}
