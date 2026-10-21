//! The activity feed: the log newest first, and what the human has not seen yet.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::time::{Duration, Instant};

use livtk_core::event::{NodeData, NoteData, Op};
use livtk_core::model::Priority;
use livtk_core::{Actor, Command, Event, Kind, Rel, State, UNSEEN_CAP, Workspace};

const HUMAN: Actor = Actor::Human;
const CLAUDE: Actor = Actor::Claude { agent: None };

fn create(ws: &mut Workspace, title: &str, by: &Actor) -> String {
    let e = ws
        .execute(
            Command::Create {
                kind: Kind::Task,
                title: title.into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            by,
        )
        .unwrap();
    ws.node(&e[0].op.subjects().0.to_string()).unwrap().key
}

fn workspace() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    ws.set_models_dir(None);
    (dir, ws)
}

#[test]
fn the_feed_is_the_log_newest_first_a_page_at_a_time() {
    let (_dir, mut ws) = workspace();
    let a = create(&mut ws, "first", &CLAUDE);
    let b = create(&mut ws, "second", &CLAUDE);
    ws.execute(
        Command::Link {
            src: b.clone(),
            rel: Rel::DependsOn,
            dst: a.clone(),
        },
        &CLAUDE,
    )
    .unwrap();
    ws.execute(
        Command::Note {
            node: a.clone(),
            text: "a note".into(),
        },
        &HUMAN,
    )
    .unwrap();

    let all = ws.activity(None, 10).unwrap();
    assert_eq!(all.len(), 4);
    assert!(
        matches!(all[0].event.op, Op::NodeNote { .. }),
        "newest first"
    );
    assert_eq!(all[0].node.as_ref().unwrap().key, a);
    assert!(matches!(all[1].event.op, Op::EdgeAdd { .. }));
    assert_eq!(
        all[1].node.as_ref().unwrap().key,
        b,
        "a link is about its source"
    );
    assert_eq!(
        all[1].other.as_ref().unwrap().key,
        a,
        "and names its other end"
    );
    assert_eq!(all[3].node.as_ref().unwrap().title, "first");

    let first = ws.activity(None, 2).unwrap();
    let rest = ws.activity(Some(first[1].event.id), 10).unwrap();
    let ids: Vec<_> = first.iter().chain(&rest).map(|x| x.event.id).collect();
    let want: Vec<_> = all.iter().map(|x| x.event.id).collect();
    assert_eq!(ids, want, "pages join up with nothing lost or repeated");
    assert!(ws.activity(Some(all[3].event.id), 10).unwrap().is_empty());
}

#[test]
fn what_is_unseen_is_what_others_did_since_the_mark_and_a_rebuild_keeps_it() {
    let (dir, mut ws) = workspace();
    create(&mut ws, "by claude", &CLAUDE);
    create(&mut ws, "by the human", &HUMAN);
    assert_eq!(ws.activity_seen().unwrap(), None);
    assert_eq!(
        ws.unseen_activity().unwrap(),
        1,
        "the human's own work is never news"
    );

    let newest = ws.activity(None, 1).unwrap()[0].event.id;
    ws.set_activity_seen(newest).unwrap();
    assert_eq!(ws.unseen_activity().unwrap(), 0);
    create(&mut ws, "more from claude", &CLAUDE);
    create(
        &mut ws,
        "and again",
        &Actor::Claude {
            agent: Some("implementer".into()),
        },
    );
    assert_eq!(
        ws.unseen_activity().unwrap(),
        2,
        "agents count as others too"
    );

    ws.rebuild().unwrap();
    assert_eq!(
        ws.activity_seen().unwrap(),
        Some(newest),
        "a rebuild keeps the mark"
    );
    let reopened = Workspace::open(dir.path()).unwrap();
    assert_eq!(reopened.unseen_activity().unwrap(), 2);
}

/// `cargo test --release -p livtk-core --test activity -- --ignored --nocapture`
#[test]
#[ignore = "a benchmark: run in release with --ignored"]
fn a_page_of_100k_events_and_the_unseen_count_stay_in_budget() {
    let (dir, ws) = workspace();
    drop(ws);
    let mut lines = String::new();
    let mut last = None;
    let mut nodes = Vec::new();
    let claude = Actor::Claude { agent: None };
    let mut push = |op: Op, last: &mut Option<uuid::Uuid>| {
        let e = Event::new(*last, &claude, op);
        *last = Some(e.id);
        lines.push_str(&e.to_line().unwrap());
        lines.push('\n');
    };
    for i in 1..=1_000u32 {
        let node = uuid::Uuid::now_v7();
        nodes.push(node);
        push(
            Op::NodeCreate {
                node,
                data: NodeData {
                    kind: Kind::Task,
                    key: format!("LIV-{i}"),
                    title: format!("task {i}"),
                    body: String::new(),
                    state: State::Backlog,
                    priority: Priority::DEFAULT,
                    props: serde_json::Map::new(),
                },
            },
            &mut last,
        );
    }
    for i in 0..99_000usize {
        push(
            Op::NodeNote {
                node: nodes[i % nodes.len()],
                data: NoteData {
                    text: format!("progress note {i}"),
                    health: None,
                },
            },
            &mut last,
        );
    }
    std::fs::write(dir.path().join(".livtk/events.jsonl"), lines).unwrap();
    let mut ws = Workspace::open(dir.path()).unwrap();
    ws.rebuild().unwrap();

    let best = |f: &mut dyn FnMut()| {
        (0..15)
            .map(|_| {
                let t = Instant::now();
                f();
                t.elapsed()
            })
            .min()
            .unwrap()
    };
    let page = best(&mut || assert_eq!(ws.activity(None, 200).unwrap().len(), 200));
    let older = ws.activity(None, 50_000).unwrap()[49_999].event.id;
    let deep = best(&mut || assert_eq!(ws.activity(Some(older), 200).unwrap().len(), 200));
    let unseen = best(&mut || assert_eq!(ws.unseen_activity().unwrap(), UNSEEN_CAP));
    eprintln!(
        "100k events: newest page {page:?}, a page 50k deep {deep:?}, unseen count {unseen:?}"
    );
    assert!(
        page < Duration::from_millis(10),
        "page budget 10 ms: {page:?}"
    );
    assert!(
        deep < Duration::from_millis(10),
        "page budget 10 ms: {deep:?}"
    );
    assert!(
        unseen < Duration::from_millis(1),
        "count budget 1 ms: {unseen:?}"
    );
}

#[test]
fn edges_lists_every_link_once() {
    let (_dir, mut ws) = workspace();
    let a = create(&mut ws, "first", &CLAUDE);
    let b = create(&mut ws, "second", &CLAUDE);
    ws.execute(
        Command::Link {
            src: b.clone(),
            rel: Rel::DependsOn,
            dst: a.clone(),
        },
        &CLAUDE,
    )
    .unwrap();
    let ids = |k: &str| ws.node(k).unwrap().id;
    assert_eq!(ws.edges().unwrap(), [(ids(&b), Rel::DependsOn, ids(&a))]);
    assert_eq!(
        ws.snapshot().unwrap().edges,
        ws.edges().unwrap(),
        "the snapshot agrees"
    );
}
