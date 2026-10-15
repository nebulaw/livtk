//! The `livtk` binary, run as a real process: arguments in, output and exit codes out.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use livtk_core::event::{Event, NodeData, Op};
use livtk_core::{Actor, Kind, State};
use serde_json::Value;

#[derive(Clone, Copy)]
enum As {
    Human,
    Claude,
}

fn livtk(dir: &Path, who: As, args: &[&str]) -> Output {
    livtk_with_stdin(dir, who, args, None)
}

fn livtk_with_stdin(dir: &Path, who: As, args: &[&str], stdin: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_livtk"));
    // Never the developer's own models: keyword search only, unless a test says otherwise.
    cmd.current_dir(dir)
        .args(args)
        .env("LIVTK_MODELS_DIR", "/nonexistent/livtk-models")
        .env(
            "LIVTK_CONFIG_DIR",
            std::env::temp_dir().join("livtk-test-config"),
        );
    match who {
        As::Human => cmd.env_remove("CLAUDECODE"),
        As::Claude => cmd.env("CLAUDECODE", "1"),
    };
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}

fn ok(dir: &Path, who: As, args: &[&str]) -> String {
    let out = livtk(dir, who, args);
    assert!(
        out.status.success(),
        "livtk {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn json(dir: &Path, args: &[&str]) -> Value {
    let mut all = vec!["--json"];
    all.extend_from_slice(args);
    serde_json::from_str(&ok(dir, As::Claude, &all)).unwrap()
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    ok(dir.path(), As::Human, &["init", "--prefix", "LIV"]);
    dir
}

#[test]
fn a_task_from_idea_to_done() {
    let dir = project();
    let d = dir.path();
    assert_eq!(
        ok(d, As::Claude, &["new", "first window"]),
        "LIV-1 created: first window\n"
    );
    ok(d, As::Claude, &["move", "LIV-1", "planned"]);

    let refused = livtk(d, As::Claude, &["ack", "1"]);
    assert_eq!(
        refused.status.code(),
        Some(3),
        "a gate refusal exits with 3"
    );
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("only a human can acknowledge LIV-1")
    );

    ok(d, As::Human, &["ack", "1"]);
    assert_eq!(json(d, &["next"])["key"], "LIV-1");
    for step in ["start", "move", "done"] {
        let args: &[&str] = if step == "move" {
            &["move", "1", "review"]
        } else {
            &[step, "1"]
        };
        ok(d, As::Claude, args);
    }
    assert_eq!(json(d, &["show", "1"])["node"]["state"], "done");
    assert_eq!(
        json(d, &["list"]).as_array().unwrap().len(),
        0,
        "done tasks are hidden"
    );
    assert_eq!(json(d, &["list", "--all"]).as_array().unwrap().len(), 1);

    let history = json(d, &["log", "1"]);
    let by: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["by"].as_str().unwrap())
        .collect();
    assert_eq!(
        by,
        ["claude", "claude", "human", "claude", "claude", "claude"]
    );
}

#[test]
fn a_subagent_signs_its_events_and_a_human_needs_no_label() {
    let dir = project();
    ok(
        dir.path(),
        As::Claude,
        &["--agent", "refuter", "new", "from a subagent"],
    );
    ok(dir.path(), As::Claude, &["new", "from the main session"]);
    let by: Vec<Value> = json(dir.path(), &["log"])
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["by"].clone())
        .collect();
    assert_eq!(by, ["claude:refuter", "claude"]);

    let out = livtk(dir.path(), As::Human, &["--agent", "refuter", "list"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("need no label"));
}

#[test]
fn a_body_can_come_from_stdin() {
    let dir = project();
    let plan = "Steps:\n1. one\n2. two\n";
    let out = livtk_with_stdin(
        dir.path(),
        As::Claude,
        &["new", "with a plan", "--body", "-"],
        Some(plan),
    );
    assert!(out.status.success());
    assert_eq!(
        json(dir.path(), &["show", "1"])["node"]["body"],
        plan.trim_end()
    );
}

#[test]
fn dependencies_from_the_command_line() {
    let dir = project();
    let d = dir.path();
    ok(d, As::Claude, &["new", "a"]);
    ok(d, As::Claude, &["new", "b", "--after", "LIV-1"]);
    let cycle = livtk(d, As::Claude, &["dep", "add", "1", "--on", "2"]);
    assert_eq!(cycle.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&cycle.stderr).contains("cycle"));
    assert_eq!(
        ok(d, As::Claude, &["dep", "rm", "2", "--on", "1"]),
        "LIV-2 no longer waits for LIV-1\n"
    );
    assert_eq!(
        ok(d, As::Claude, &["dep", "add", "1", "--on", "2"]),
        "LIV-1 now waits for LIV-2\n"
    );
    assert_eq!(
        ok(d, As::Claude, &["dep", "add", "1", "--on", "2"]),
        "LIV-1 already waits for LIV-2\n"
    );
}

#[test]
fn errors_are_one_line_with_a_hint() {
    let dir = tempfile::tempdir().unwrap();
    let out = livtk(dir.path(), As::Claude, &["list"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "livtk: no .livtk directory here or in any parent; run `livtk init` in the project root\n"
    );
    let dir = project();
    let out = livtk(dir.path(), As::Claude, &["show", "LIV-9"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no node `LIV-9`"));
}

#[test]
fn eight_processes_at_once_lose_nothing() {
    let dir = project();
    let children: Vec<_> = (0..8)
        .map(|w| {
            let path = dir.path().to_path_buf();
            std::thread::spawn(move || {
                for i in 0..25 {
                    ok(
                        &path,
                        As::Claude,
                        &["new", &format!("process {w} task {i}")],
                    );
                }
            })
        })
        .collect();
    for c in children {
        c.join().unwrap();
    }
    let tasks = json(dir.path(), &["list"]);
    let mut keys: Vec<&str> = tasks
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys.len(), 200);
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), 200, "every key is unique");
    let log = std::fs::read_to_string(dir.path().join(".livtk/events.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 200);
}

#[test]
fn memory_from_proposal_to_rule() {
    let dir = project();
    let d = dir.path();
    let out = ok(
        d,
        As::Claude,
        &[
            "lesson",
            "Run cargo check before reporting a task done",
            "--why",
            "a broken build was reported done",
        ],
    );
    assert_eq!(
        out,
        "LES-1 proposed: Run cargo check before reporting a task done\n  it binds once the human runs `livtk ratify LES-1`\n"
    );
    let refused = livtk(d, As::Claude, &["ratify", "LES-1"]);
    assert_eq!(refused.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("only a human can ratify"));

    let inbox = json(d, &["inbox"]);
    assert_eq!(inbox[0]["key"], "LES-1");
    assert_eq!(
        ok(d, As::Human, &["ratify", "LES-1", "--always"]),
        "LES-1: proposed -> ratified\nLES-1: a standing rule, shown at every session start\n"
    );
    assert!(ok(d, As::Claude, &["show", "LES-1"]).contains("ratified · lesson · standing rule"));
    assert_eq!(ok(d, As::Claude, &["inbox"]), "Nothing waits for you.\n");

    ok(
        d,
        As::Claude,
        &[
            "lesson",
            "Run cargo clippy too before reporting done",
            "--supersedes",
            "LES-1",
        ],
    );
    assert_eq!(
        ok(d, As::Human, &["ratify", "LES-2"]),
        "LES-2: proposed -> ratified\nLES-2: recalled when relevant\nLES-1 -> retired (replaced by LES-2)\n"
    );
    let active = json(d, &["list", "--kind", "lesson"]);
    assert_eq!(
        active.as_array().unwrap().len(),
        1,
        "the retired rule is over"
    );

    ok(
        d,
        As::Claude,
        &[
            "decision",
            "SQLite for the store",
            "--body",
            "one file, no daemon",
        ],
    );
    ok(
        d,
        As::Claude,
        &[
            "finding",
            "winit panics on resize under NVIDIA",
            "--from",
            "LES-2",
        ],
    );
    assert_eq!(
        json(
            d,
            &[
                "recall",
                "should the clippy check run before reporting done"
            ]
        )[0]["key"],
        "LES-2"
    );
    let hits = json(d, &["search", "sqlite"]);
    assert_eq!(hits[0]["key"], "DEC-1");
    assert_eq!(
        hits[0]["cosine"],
        Value::Null,
        "keywords only without a model"
    );
}

#[test]
fn embed_status_without_a_model() {
    let dir = project();
    let status = json(dir.path(), &["embed", "status"]);
    assert_eq!(status["model"], "potion-retrieval-32M");
    assert_eq!(status["installed"], false);
    assert!(ok(dir.path(), As::Claude, &["embed", "status"]).contains("search uses keywords only"));
}

/// `cargo test --release -p livtk-cli -- --ignored`: the speed budget from the plan.
#[test]
#[ignore = "a benchmark: run in release with --ignored"]
fn list_of_10k_tasks_stays_under_10ms_warm() {
    let dir = project();
    let mut lines = String::new();
    let mut last = None;
    for n in 1..=10_000u32 {
        let e = Event::new(
            last,
            &Actor::Claude { agent: None },
            Op::NodeCreate {
                node: uuid::Uuid::now_v7(),
                data: NodeData {
                    kind: Kind::Task,
                    key: format!("LIV-{n}"),
                    title: format!("task number {n} with a title of ordinary length"),
                    body: String::new(),
                    state: if n % 3 == 0 {
                        State::Done
                    } else {
                        State::Backlog
                    },
                    priority: livtk_core::model::Priority::DEFAULT,
                    props: serde_json::Map::new(),
                },
            },
        );
        last = Some(e.id);
        lines.push_str(&e.to_line().unwrap());
        lines.push('\n');
    }
    std::fs::write(dir.path().join(".livtk/events.jsonl"), lines).unwrap();

    let start = Instant::now();
    ok(dir.path(), As::Claude, &["rebuild"]);
    let rebuild = start.elapsed();

    let mut runs: Vec<Duration> = (0..15)
        .map(|_| {
            let t = Instant::now();
            let out = livtk(dir.path(), As::Claude, &["list"]);
            assert!(out.status.success());
            t.elapsed()
        })
        .collect();
    runs.sort();
    let median = runs[runs.len() / 2];
    let json_best = (0..15)
        .map(|_| {
            let t = Instant::now();
            ok(dir.path(), As::Claude, &["--json", "list"]);
            t.elapsed()
        })
        .min()
        .unwrap();
    let show = (0..15)
        .map(|_| {
            let t = Instant::now();
            ok(dir.path(), As::Claude, &["show", "LIV-5000"]);
            t.elapsed()
        })
        .min()
        .unwrap();
    eprintln!(
        "10k tasks: rebuild {rebuild:?}, list median {median:?}, json list best {json_best:?}, show best {show:?}"
    );
    assert!(median < Duration::from_millis(10), "list took {median:?}");
}
