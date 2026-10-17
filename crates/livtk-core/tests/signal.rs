//! Signals from the log: each rule, and reading on from a cursor.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use livtk_core::signal::{Signal, Signals};
use livtk_core::{Actor, Command, Kind, Rel, State, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

fn create(ws: &mut Workspace, title: &str) -> String {
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
    ws.node(&e[0].op.subjects().0.to_string()).unwrap().key
}

fn mv(ws: &mut Workspace, key: &str, to: State, by: &Actor) {
    ws.execute(
        Command::Move {
            node: key.into(),
            to,
        },
        by,
    )
    .unwrap();
}

/// Every signal of the log, in order.
fn all(ws: &Workspace) -> Vec<Signal> {
    let mut s = Signals::new(ws.gates());
    ws.events_after(None)
        .unwrap()
        .iter()
        .flat_map(|e| s.feed(e))
        .collect()
}

fn names(signals: &[Signal], key: &str) -> Vec<&'static str> {
    signals
        .iter()
        .filter(|s| s.key == key)
        .map(|s| s.signal)
        .collect()
}

#[test]
fn a_plan_needs_the_humans_decision_and_the_ack_makes_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    let k = create(&mut ws, "a plan");
    mv(&mut ws, &k, State::Planned, &CLAUDE);
    mv(&mut ws, &k, State::Acked, &Actor::Human);
    ws.execute(
        Command::Note {
            node: k.clone(),
            text: "noted".into(),
        },
        &CLAUDE,
    )
    .unwrap();
    let s = all(&ws);
    assert_eq!(
        names(&s, &k),
        [
            "node.created",
            "node.moved",
            "decision.needed",
            "node.moved",
            "decision.made",
            "node.noted"
        ]
    );
    let made = s.iter().find(|s| s.signal == "decision.made").unwrap();
    assert_eq!(made.by, "human");
    let json = serde_json::to_value(made).unwrap();
    assert_eq!(json["v"], 1);
    assert_eq!(
        (json["from"].as_str(), json["to"].as_str()),
        (Some("planned"), Some("acked"))
    );
}

#[test]
fn work_is_unblocked_when_the_last_thing_it_waits_for_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    let a = create(&mut ws, "waits");
    let b = create(&mut ws, "first dependency");
    let c = create(&mut ws, "second dependency");
    for d in [&b, &c] {
        ws.execute(
            Command::Link {
                src: a.clone(),
                rel: Rel::DependsOn,
                dst: d.clone(),
            },
            &CLAUDE,
        )
        .unwrap();
    }
    mv(&mut ws, &b, State::Canceled, &Actor::Human);
    assert!(
        !all(&ws).iter().any(|s| s.signal == "work.unblocked"),
        "one dependency still open"
    );
    mv(&mut ws, &c, State::Canceled, &Actor::Human);
    let unblocked: Vec<_> = all(&ws)
        .into_iter()
        .filter(|s| s.signal == "work.unblocked")
        .map(|s| s.key)
        .collect();
    assert_eq!(unblocked, [a], "once, when the last one closes");
}

#[test]
fn a_reader_reads_on_from_the_last_event_it_saw() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    let k = create(&mut ws, "first");
    let cursor = ws.events_after(None).unwrap().last().unwrap().id;
    mv(&mut ws, &k, State::Planned, &CLAUDE);
    // A reader that stopped at the cursor: it keeps its projection, and feeds what came after.
    let mut s = Signals::new(ws.gates());
    for e in ws.events_after(None).unwrap() {
        if e.id <= cursor {
            s.feed(&e);
        }
    }
    let later: Vec<&str> = ws
        .events_after(Some(cursor))
        .unwrap()
        .iter()
        .flat_map(|e| s.feed(e))
        .map(|s| s.signal)
        .collect();
    assert_eq!(
        later,
        ["node.moved", "decision.needed"],
        "only what came after"
    );
}
