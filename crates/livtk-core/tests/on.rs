//! Hooks on signals: run once each, from now on, retried until they succeed, one dispatcher at
//! a time.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic
#![cfg(unix)]

use std::path::Path;

use livtk_core::{Actor, Command, Kind, State, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

/// A project whose hook appends each decision it hears to `heard`, but only while `ok` exists.
fn project(dir: &Path) -> Workspace {
    Workspace::init(dir, Some("LIV")).unwrap();
    let config = dir.join(".livtk/config.toml");
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str(
        "\n[[on]]\nsignal = \"decision.made\"\nrun = [\"sh\", \"-c\", \"test -f ok && cat >> heard\"]\n",
    );
    std::fs::write(&config, text).unwrap();
    Workspace::open(dir).unwrap()
}

fn plan(ws: &mut Workspace, title: &str) -> String {
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
    ws.execute(
        Command::Move {
            node: key.clone(),
            to: State::Planned,
        },
        &CLAUDE,
    )
    .unwrap();
    key
}

fn ack(ws: &mut Workspace, key: &str) {
    ws.execute(
        Command::Move {
            node: key.into(),
            to: State::Acked,
        },
        &Actor::Human,
    )
    .unwrap();
}

fn heard(dir: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(dir.join("heard"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn a_new_hook_hears_what_comes_not_the_history() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ok"), "").unwrap();
    let mut ws = project(dir.path());
    let old = plan(&mut ws, "before the hook");
    ack(&mut ws, &old);
    assert_eq!(
        ws.dispatch().unwrap().ran,
        0,
        "first seen: it starts at the end"
    );
    let new = plan(&mut ws, "after");
    ack(&mut ws, &new);
    assert_eq!(ws.dispatch().unwrap().ran, 1);
    let h = heard(dir.path());
    assert_eq!(h.len(), 1);
    assert_eq!(h[0]["signal"], "decision.made");
    assert_eq!(h[0]["key"], new.as_str());
    assert_eq!(ws.dispatch().unwrap().ran, 0, "never twice");
    let ignore = std::fs::read_to_string(dir.path().join(".livtk/.gitignore")).unwrap();
    assert!(
        ignore.contains("on-state.json") && ignore.contains("on.lock"),
        "{ignore}"
    );
}

#[test]
fn a_failing_hook_is_tried_again_and_misses_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    ws.dispatch().unwrap();
    let a = plan(&mut ws, "a");
    ack(&mut ws, &a);
    let done = ws.dispatch().unwrap();
    assert_eq!(done.failed.len(), 1, "no `ok`: it fails");
    std::fs::write(dir.path().join("ok"), "").unwrap();
    let b = plan(&mut ws, "b");
    ack(&mut ws, &b);
    assert_eq!(
        ws.dispatch().unwrap().ran,
        2,
        "the missed one, then the new one"
    );
    let keys: Vec<_> = heard(dir.path())
        .iter()
        .map(|h| h["key"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(keys, [a, b], "in order");
}

#[test]
fn a_second_dispatcher_leaves_the_work_to_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.path().join(".livtk/on.lock"))
        .unwrap();
    held.lock().unwrap();
    assert!(ws.dispatch().unwrap().busy);
    drop(held);
    assert!(!ws.dispatch().unwrap().busy);
}
