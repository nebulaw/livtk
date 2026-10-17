//! Search at the size the command palette must handle: 10,000 nodes, a query per keystroke.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::path::PathBuf;
use std::time::{Duration, Instant};

use livtk_core::event::{NodeData, Op};
use livtk_core::model::Priority;
use livtk_core::{Actor, Command, Event, Kind, Scope, State, Workspace};

const WORDS: &[&str] = &[
    "window",
    "resize",
    "compositor",
    "panic",
    "driver",
    "render",
    "frame",
    "input",
    "keyboard",
    "layout",
    "shader",
    "texture",
    "buffer",
    "socket",
    "config",
    "plugin",
    "theme",
    "cursor",
    "scroll",
    "animation",
    "timing",
    "surface",
    "output",
    "monitor",
    "client",
    "server",
    "event",
    "focus",
    "popup",
    "tooltip",
    "menu",
    "drag",
    "drop",
    "clipboard",
    "font",
    "glyph",
];

/// A project of `n` tasks with titles and bodies drawn from a small vocabulary, written as a log
/// and rebuilt, as a long-lived project would be.
fn project_of(n: u32) -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    Workspace::init(dir.path(), Some("LIV")).unwrap();
    let mut lines = String::new();
    let mut last = None;
    let word = |i: u32| WORDS[(i as usize) % WORDS.len()];
    for i in 1..=n {
        let title = format!(
            "{} the {} when the {} changes",
            word(i),
            word(i * 7),
            word(i * 13)
        );
        let body = format!(
            "Goal: {} works with {}. Approach: check {} and {} first.",
            word(i * 3),
            word(i * 5),
            word(i * 11),
            word(i * 17)
        );
        let e = Event::new(
            last,
            &Actor::Claude { agent: None },
            Op::NodeCreate {
                node: uuid::Uuid::now_v7(),
                data: NodeData {
                    kind: Kind::Task,
                    key: format!("LIV-{i}"),
                    title,
                    body,
                    state: State::Backlog,
                    priority: Priority::DEFAULT,
                    props: serde_json::Map::new(),
                },
            },
        );
        last = Some(e.id);
        lines.push_str(&e.to_line().unwrap());
        lines.push('\n');
    }
    std::fs::write(dir.path().join(".livtk/events.jsonl"), lines).unwrap();
    let mut ws = Workspace::open(dir.path()).unwrap();
    ws.rebuild().unwrap();
    (dir, ws)
}

/// The median of `runs` searches for each query, typed a letter at a time.
fn per_keystroke(ws: &mut Workspace, queries: &[&str]) -> Duration {
    let mut runs = Vec::new();
    for q in queries {
        for end in 1..=q.len() {
            let t = Instant::now();
            ws.search(&q[..end], Scope::All, 20).unwrap();
            runs.push(t.elapsed());
        }
    }
    runs.sort();
    runs[runs.len() / 2]
}

const TYPED: &[&str] = &["resize the window", "shader panic", "drag and drop menu"];

/// `cargo test --release -p livtk-core --test search -- --ignored --nocapture`
#[test]
#[ignore = "a benchmark: run in release with --ignored"]
fn keyword_search_of_10k_nodes_per_keystroke() {
    let (_dir, mut ws) = project_of(10_000);
    ws.set_models_dir(None);
    let median = per_keystroke(&mut ws, TYPED);
    eprintln!("keywords only, 10k nodes: median {median:?} per keystroke");
}

/// `LIVTK_TEST_MODELS=<dir> cargo test --release -p livtk-core --test search -- --ignored --nocapture`
#[test]
#[ignore = "needs the model: LIVTK_TEST_MODELS=<dir>"]
fn with_model_search_by_meaning_of_10k_nodes_per_keystroke() {
    let models = PathBuf::from(std::env::var("LIVTK_TEST_MODELS").expect("set LIVTK_TEST_MODELS"));
    let (_dir, mut ws) = project_of(10_000);
    ws.set_models_dir(Some(models));
    let t = Instant::now();
    ws.search("warm up", Scope::All, 20).unwrap();
    eprintln!(
        "first search, 10k nodes: {:?} (model load and {} embedded)",
        t.elapsed(),
        ws.embedded()
    );
    let median = per_keystroke(&mut ws, TYPED);
    eprintln!("with the model, 10k nodes: median {median:?} per keystroke");
}

fn create(ws: &mut Workspace, kind: Kind, title: &str) -> String {
    let e = ws
        .execute(
            Command::Create {
                kind,
                title: title.into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &Actor::Claude { agent: None },
        )
        .unwrap();
    ws.node(&e[0].op.subjects().0.to_string()).unwrap().key
}

/// Keywords only: a small project with a key that is also a number in a title.
fn small() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    ws.set_models_dir(None);
    create(&mut ws, Kind::Task, "Resize the window without flicker");
    create(&mut ws, Kind::Task, "Handle 2 monitors");
    create(
        &mut ws,
        Kind::Lesson,
        "Run cargo check before reporting done",
    );
    (dir, ws)
}

fn keys(hits: &[livtk_core::Hit]) -> Vec<&str> {
    hits.iter().map(|h| h.key.as_str()).collect()
}

#[test]
fn find_brings_the_node_whose_key_was_typed_first() {
    let (_dir, mut ws) = small();
    for q in ["LIV-2", "liv-2", "2", " 2 "] {
        let hits = ws.find(q, 10).unwrap();
        assert_eq!(keys(&hits).first(), Some(&"LIV-2"), "{q:?}");
    }
    assert_eq!(keys(&ws.find("LES-1", 10).unwrap()), ["LES-1"]);
    assert!(ws.find("LIV-99", 10).unwrap().is_empty());
}

#[test]
fn find_reads_the_word_being_typed_as_a_prefix() {
    let (_dir, mut ws) = small();
    assert_eq!(keys(&ws.find("resi", 10).unwrap()), ["LIV-1"]);
    assert_eq!(keys(&ws.find("window fli", 10).unwrap()), ["LIV-1"]);
    assert_eq!(keys(&ws.find("cargo ch", 10).unwrap()), ["LES-1"]);
    assert!(
        ws.find("resi ", 10).unwrap().is_empty(),
        "a space ends the word: `resi` is no word of any title"
    );
    assert!(
        ws.search("resi", Scope::All, 10).unwrap().is_empty(),
        "search, unlike find, matches whole words"
    );
    assert!(
        ws.find("r", 10).unwrap().is_empty(),
        "one letter finds nothing"
    );
}
