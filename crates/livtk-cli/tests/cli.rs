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

/// The keys `show --json` lists under `field`.
fn shown(d: &Path, key: &str, field: &str) -> Vec<String> {
    let v = json(d, &["show", key]);
    let list = match field {
        "depends_on" | "blocks" => &v["links"][field],
        _ => &v[field],
    };
    list.as_array()
        .unwrap()
        .iter()
        .map(|b| b["key"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn any_link_from_the_command_line_read_from_either_end() {
    let dir = project();
    let d = dir.path();
    ok(d, As::Claude, &["new", "the feature"]);
    assert_eq!(
        ok(d, As::Claude, &["new", "a part", "--parent", "LIV-1"]),
        "LIV-2 created: a part\n  is part of LIV-1\n"
    );
    ok(d, As::Claude, &["new", "another part"]);
    assert_eq!(
        ok(d, As::Claude, &["link", "1", "has-part", "3"]),
        "LIV-1 has-part LIV-3: linked\n"
    );
    assert_eq!(shown(d, "1", "sub_tasks"), ["LIV-2", "LIV-3"]);
    assert_eq!(
        shown(d, "3", "part_of"),
        ["LIV-1"],
        "the same link from below"
    );
    assert_eq!(
        ok(d, As::Claude, &["link", "3", "part-of", "1"]),
        "LIV-3 part-of LIV-1: already linked\n"
    );
    let show = ok(d, As::Claude, &["show", "1"]);
    assert!(show.contains("sub-task  LIV-2"), "{show}");

    ok(d, As::Claude, &["link", "2", "blocks", "3"]);
    assert_eq!(shown(d, "3", "depends_on"), ["LIV-2"]);
    ok(d, As::Claude, &["link", "3", "relates", "2"]);
    assert_eq!(
        shown(d, "2", "related"),
        ["LIV-3"],
        "relating shows at both ends"
    );
    assert_eq!(shown(d, "3", "related"), ["LIV-2"]);

    ok(d, As::Claude, &["lesson", "keep parts small"]);
    ok(d, As::Claude, &["link", "LES-1", "learned-from", "LIV-2"]);
    assert_eq!(shown(d, "LES-1", "learned_from"), ["LIV-2"]);
    ok(d, As::Claude, &["lesson", "keep parts very small"]);
    ok(d, As::Claude, &["link", "LES-2", "supersedes", "LES-1"]);
    assert_eq!(shown(d, "LES-2", "supersedes"), ["LES-1"]);

    let wrong = livtk(d, As::Claude, &["link", "LES-1", "part-of", "LIV-1"]);
    assert!(!wrong.status.success());
    assert!(
        String::from_utf8_lossy(&wrong.stderr).contains("cannot have a `parent` link"),
        "{}",
        String::from_utf8_lossy(&wrong.stderr)
    );
    let cycle = livtk(d, As::Claude, &["link", "1", "part-of", "2"]);
    assert_eq!(cycle.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&cycle.stderr).contains("cycle"));
    let unknown = livtk(d, As::Claude, &["link", "1", "owns", "2"]);
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("part-of"),
        "names the words"
    );

    assert_eq!(
        ok(d, As::Claude, &["unlink", "1", "has-part", "2"]),
        "LIV-1 has-part LIV-2: unlinked\n"
    );
    assert_eq!(shown(d, "1", "sub_tasks"), ["LIV-3"]);
    assert_eq!(
        ok(d, As::Claude, &["link", "2", "relates", "3"]),
        "LIV-2 relates LIV-3: already linked\n",
        "relating has no direction"
    );
    ok(d, As::Claude, &["unlink", "2", "relates", "3"]);
    assert!(
        shown(d, "3", "related").is_empty(),
        "so it is removed from either end"
    );
    let gone = livtk(d, As::Claude, &["unlink", "2", "relates", "3"]);
    assert!(!gone.status.success());
}

#[test]
fn a_gate_in_the_config_keeps_a_move_for_the_human() {
    let dir = project();
    let d = dir.path();
    let config = d.join(".livtk/config.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        text + "\n[gates]\nhuman = [\"task: review -> done\"]\n",
    )
    .unwrap();
    ok(d, As::Claude, &["new", "a"]);
    ok(d, As::Claude, &["move", "1", "planned"]);
    ok(d, As::Human, &["ack", "1"]);
    ok(d, As::Claude, &["start", "1"]);
    ok(d, As::Claude, &["move", "1", "review"]);
    assert_eq!(
        json(d, &["inbox"]).as_array().unwrap()[0]["key"],
        "LIV-1",
        "waiting for the human"
    );
    let inbox = ok(d, As::Human, &["inbox"]);
    assert!(inbox.contains("LIV-1  move to done"), "{inbox}");
    let refused = livtk(d, As::Claude, &["done", "1"]);
    assert_eq!(
        refused.status.code(),
        Some(3),
        "a gate refusal exits with 3"
    );
    assert!(String::from_utf8_lossy(&refused.stderr).contains("[gates]"));
    ok(d, As::Human, &["done", "1"]);
    assert_eq!(json(d, &["show", "1"])["node"]["state"], "done");
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

#[test]
fn version_says_which_build_this_is() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_livtk"))
        .arg("--version")
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert!(
        text.starts_with(&format!("livtk {} (build ", livtk_build::VERSION)),
        "{text}"
    );
    assert!(text.contains(livtk_build::COMMIT), "{text}");
}

/// `events` prints signals as JSON lines; `--since` reads on after a cursor; `--follow` sees a
/// write from another process as it comes.
#[test]
fn events_read_on_from_a_cursor_and_follow_writes_from_elsewhere() {
    use std::io::{BufRead, BufReader};
    let dir = project();
    ok(dir.path(), As::Claude, &["new", "first"]);
    let lines = ok(dir.path(), As::Claude, &["events"]);
    let first: Vec<Value> = lines
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(first[0]["signal"], "node.created");
    let cursor = first.last().unwrap()["event"].as_str().unwrap().to_owned();
    assert_eq!(
        ok(dir.path(), As::Claude, &["events", "--since", &cursor]),
        "",
        "nothing after the cursor yet"
    );

    let mut follow = Command::new(env!("CARGO_BIN_EXE_livtk"))
        .current_dir(dir.path())
        .args([
            "events",
            "--follow",
            "--since",
            &cursor,
            "--signal",
            "decision.needed",
        ])
        .env("LIVTK_MODELS_DIR", "/nonexistent/livtk-models")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    ok(dir.path(), As::Claude, &["move", "LIV-1", "planned"]);
    let (tx, rx) = std::sync::mpsc::channel();
    let out = follow.stdout.take().unwrap();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(out).read_line(&mut line);
        let _ = tx.send(line);
    });
    let line = rx.recv_timeout(Duration::from_secs(10)).unwrap_or_default();
    let _ = follow.kill();
    let _ = follow.wait();
    let signal: Value = serde_json::from_str(&line).expect("a signal while following");
    assert_eq!(signal["signal"], "decision.needed");
    assert_eq!(signal["key"], "LIV-1");
}

/// A command that writes starts the project's hooks apart and returns at once; the hook hears
/// the signal.
#[cfg(unix)]
#[test]
fn a_write_from_the_cli_runs_the_projects_hooks_apart() {
    let dir = project();
    // The hook comes after the first write: the write that first wakes it is heard too.
    ok(dir.path(), As::Claude, &["new", "first"]);
    let config = dir.path().join(".livtk/config.toml");
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str(
        "\n[[on]]\nsignal = \"decision.needed\"\nrun = [\"sh\", \"-c\", \"cat >> heard\"]\n",
    );
    std::fs::write(&config, text).unwrap();
    ok(dir.path(), As::Claude, &["move", "LIV-1", "planned"]);
    let heard = dir.path().join("heard");
    let until = Instant::now() + Duration::from_secs(10);
    while !std::fs::read_to_string(&heard).is_ok_and(|t| t.contains("decision.needed")) {
        assert!(
            Instant::now() < until,
            "the hook ran: {:?}",
            std::fs::read_to_string(&heard)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let line: Value = serde_json::from_str(
        std::fs::read_to_string(&heard)
            .unwrap()
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(line["key"], "LIV-1");
}

/// A doc's path is read from the folder the command runs in, as a shell's paths are: from a
/// subfolder, `README.md` is the one there, not the project's.
#[test]
fn doc_paths_are_read_from_the_folder_livtk_runs_in() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        livtk(dir.path(), As::Human, &["init", "--prefix", "DOC"])
            .status
            .success()
    );
    std::fs::create_dir_all(dir.path().join("docs/guide")).unwrap();
    std::fs::write(dir.path().join("README.md"), "# Root\n").unwrap();
    std::fs::write(dir.path().join("docs/guide/setup.md"), "# Setup\n").unwrap();
    assert!(
        livtk(dir.path(), As::Human, &["new", "Write it"])
            .status
            .success()
    );
    let guide = dir.path().join("docs/guide");
    let out = livtk(&guide, As::Human, &["doc", "link", "DOC-1", "setup.md"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "DOC-1 docs/guide/setup.md: linked"
    );
    let out = livtk(&guide, As::Human, &["doc", "link", "DOC-1", "README.md"]);
    assert!(
        !out.status.success(),
        "no README.md here, though the project has one"
    );
    let out = livtk(&guide, As::Human, &["doc", "links", "setup.md"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("DOC-1"));
}

/// Starting a task names the docs it is linked to, to read first.
#[test]
fn starting_a_task_names_its_docs() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        livtk(dir.path(), As::Human, &["init", "--prefix", "DOC"])
            .status
            .success()
    );
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/design.md"), "# The design\n").unwrap();
    for (who, args) in [
        (As::Claude, vec!["new", "Build it"]),
        (As::Claude, vec!["doc", "link", "DOC-1", "docs/design.md"]),
        (As::Claude, vec!["move", "DOC-1", "planned"]),
        (As::Human, vec!["ack", "DOC-1"]),
    ] {
        assert!(livtk(dir.path(), who, &args).status.success(), "{args:?}");
    }
    let out = livtk(dir.path(), As::Claude, &["start", "DOC-1"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("read: docs/design.md  The design"), "{text}");
}

// ---- readable by machines, and a markdown mirror ---------------------------------------------------

#[test]
fn json_rows_carry_the_fields_asked_for_and_show_carries_everything() {
    let dir = project();
    let d = dir.path();
    ok(
        d,
        As::Claude,
        &["new", "import the board", "--body", "Match on board_id."],
    );
    ok(d, As::Claude, &["note", "LIV-1", "tried matching by title"]);
    // The default row is as it was.
    let rows = json(d, &["list"]);
    assert!(rows[0].get("body").is_none() && rows[0].get("props").is_none());
    assert!(rows[0].get("key").is_some());
    // Only the fields asked for, a node's own too.
    let rows = json(d, &["list", "--fields", "key,state,body,props"]);
    let row = rows[0].as_object().unwrap();
    let mut keys: Vec<&str> = row.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["body", "key", "props", "state"]);
    assert_eq!(row["body"], "Match on board_id.");
    let bad = livtk(d, As::Claude, &["--json", "list", "--fields", "key,nope"]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("nope"));
    // Show: body, props, links and notes in one object.
    let one = json(d, &["show", "LIV-1"]);
    assert_eq!(one["node"]["body"], "Match on board_id.");
    assert!(one["node"]["props"].is_object());
    assert!(one["links"].is_object());
    assert_eq!(one["notes"][0]["text"], "tried matching by title");
}

#[test]
fn export_mirrors_every_node_as_markdown_and_check_says_when_it_is_stale() {
    let dir = project();
    let d = dir.path();
    ok(
        d,
        As::Claude,
        &[
            "new",
            "import the board",
            "--body",
            "Only in the body: zebra-42.",
        ],
    );
    ok(
        d,
        As::Claude,
        &["lesson", "Read before editing", "--why", "lost text"],
    );
    ok(d, As::Human, &["export"]);
    let out = d.join(".livtk/export");
    let task = std::fs::read_to_string(out.join("tasks/LIV-1.md")).unwrap();
    assert!(task.starts_with("---\nkey: \"LIV-1\""), "{task}");
    assert!(task.contains("zebra-42"), "the body, greppable");
    assert!(out.join("lessons").read_dir().unwrap().count() == 1);
    assert!(
        std::fs::read_to_string(out.join(".gitignore"))
            .unwrap()
            .contains('*'),
        "kept out of git by default"
    );
    // A second run with nothing changed writes nothing.
    let again = ok(d, As::Human, &["export"]);
    assert!(again.contains("(0 file(s) written)"), "{again}");
    assert!(livtk(d, As::Human, &["export", "--check"]).status.success());
    // A change makes it stale; exported again, it is current.
    ok(d, As::Claude, &["note", "LIV-1", "a new note"]);
    assert!(!livtk(d, As::Human, &["export", "--check"]).status.success());
    ok(d, As::Human, &["export"]);
    assert!(livtk(d, As::Human, &["export", "--check"]).status.success());
    // Editing an exported file changes nothing in livtk.
    std::fs::write(out.join("tasks/LIV-1.md"), "---\ntitle: \"hacked\"\n---\n").unwrap();
    assert_eq!(
        json(d, &["show", "LIV-1"])["node"]["title"],
        "import the board"
    );
}

// ---- a project's own kinds -------------------------------------------------------------------------

fn declare(dir: &Path, toml: &str) {
    let path = dir.join(".livtk/config.toml");
    let mut config = std::fs::read_to_string(&path).unwrap();
    config.push('\n');
    config.push_str(toml);
    std::fs::write(path, config).unwrap();
}

const INCIDENT: &str = "[[kinds]]\nname = \"incident\"\nstates = [\"open\", \"mitigated\", \"closed\"]\nhuman = [\"mitigated -> closed\"]\nkey = \"INC\"\n";

#[test]
fn a_declared_kind_is_created_listed_moved_gated_and_found() {
    let dir = project();
    let d = dir.path();
    declare(d, INCIDENT);
    let out = ok(
        d,
        As::Claude,
        &["new", "--kind", "incident", "checkout is down"],
    );
    assert!(out.contains("INC-1"), "its own key: {out}");
    let rows = json(d, &["list", "--kind", "incident"]);
    assert_eq!(rows[0]["key"], "INC-1");
    assert_eq!(rows[0]["state"], "open", "starts at its first state");
    assert_eq!(rows[0]["kind"], "incident");
    // Any move between its states; the ones it keeps for the human are the human's.
    ok(d, As::Claude, &["move", "INC-1", "mitigated"]);
    let refused = livtk(d, As::Claude, &["move", "INC-1", "closed"]);
    assert_eq!(refused.status.code(), Some(3), "the human's move");
    ok(d, As::Human, &["move", "INC-1", "closed"]);
    // Closed is over: out of the active list, in --all.
    assert_eq!(
        json(d, &["list", "--kind", "incident"])
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        json(d, &["list", "--kind", "incident", "--all"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Found by search.
    let hits = json(d, &["search", "checkout"]);
    assert!(
        hits.as_array().unwrap().iter().any(|h| h["key"] == "INC-1"),
        "{hits}"
    );
    // A gate in [gates] can name it too.
    declare(d, "[gates]\nhuman = [\"incident: -> mitigated\"]\n");
    ok(d, As::Human, &["move", "INC-1", "open"]);
    let gated = livtk(d, As::Claude, &["move", "INC-1", "mitigated"]);
    assert_eq!(gated.status.code(), Some(3));
}

#[test]
fn a_project_declaring_nothing_is_as_before_and_a_kind_in_use_cannot_be_dropped() {
    let dir = project();
    let d = dir.path();
    // Nothing declared: the built-in kinds and their keys, as always.
    let out = ok(d, As::Claude, &["new", "a task"]);
    assert!(out.contains("LIV-1"));
    assert!(
        livtk(d, As::Claude, &["new", "--kind", "incident", "x"])
            .status
            .code()
            != Some(0)
    );
    // Declared, used, then taken out of the config: refused, naming the nodes.
    let config = std::fs::read_to_string(d.join(".livtk/config.toml")).unwrap();
    declare(d, INCIDENT);
    ok(
        d,
        As::Claude,
        &["new", "--kind", "incident", "checkout is down"],
    );
    std::fs::write(d.join(".livtk/config.toml"), config).unwrap();
    let out = livtk(d, As::Claude, &["list"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("incident") && err.contains("INC-1"), "{err}");
}

#[test]
fn a_bad_declaration_stops_livtk_with_its_reason() {
    for (toml, says) in [
        (
            "[[kinds]]\nname = \"task\"\nstates = [\"a\"]\nkey = \"TK\"\n",
            "exists already",
        ),
        (
            "[[kinds]]\nname = \"bug\"\nstates = []\nkey = \"BUG\"\n",
            "at least one state",
        ),
        (
            "[[kinds]]\nname = \"bug\"\nstates = [\"open\"]\nhuman = [\"open -> gone\"]\nkey = \"BUG\"\n",
            "not a move",
        ),
        (
            "[[kinds]]\nname = \"bug\"\nstates = [\"open\"]\nkey = \"LES\"\n",
            "another kind",
        ),
    ] {
        let dir = project();
        declare(dir.path(), toml);
        let out = livtk(dir.path(), As::Claude, &["list"]);
        assert!(!out.status.success(), "{toml}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(says), "{toml}: {err}");
    }
}

#[test]
fn a_declared_relation_is_added_shown_from_both_ends_and_removed() {
    let dir = project();
    let d = dir.path();
    declare(
        d,
        "[[relations]]\nname = \"implements\"\ninverse = \"implemented_by\"\n",
    );
    ok(d, As::Claude, &["new", "the importer"]);
    ok(d, As::Claude, &["decision", "Import in one pass"]);
    ok(d, As::Claude, &["link", "LIV-1", "implements", "DEC-1"]);
    let from = ok(d, As::Claude, &["show", "LIV-1"]);
    assert!(from.contains("implements  DEC-1"), "{from}");
    let back = ok(d, As::Claude, &["show", "DEC-1"]);
    assert!(
        back.contains("implemented_by  LIV-1"),
        "the inverse from the other end: {back}"
    );
    // From the other end, by the inverse: the same link.
    ok(
        d,
        As::Claude,
        &["unlink", "DEC-1", "implemented_by", "LIV-1"],
    );
    assert!(
        !ok(d, As::Claude, &["show", "LIV-1"]).contains("implements  DEC-1"),
        "unlinked"
    );
    // An unknown word names the ones there are.
    let out = livtk(d, As::Claude, &["link", "LIV-1", "implementz", "DEC-1"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("implemented_by"));
}

#[test]
fn a_declared_kind_is_over_in_its_own_closed_states_whatever_their_names() {
    let dir = project();
    let d = dir.path();
    declare(
        d,
        "[[kinds]]\nname = \"bug\"\nstates = [\"open\", \"done\", \"verified\"]\nkey = \"BUG\"\n\
         [[kinds]]\nname = \"rfc\"\nstates = [\"draft\", \"accepted\"]\nkey = \"RFC\"\n\
         [[kinds]]\nname = \"pattern\"\nstates = [\"draft\", \"closed\", \"archived\"]\nclosed = [\"archived\"]\nkey = \"PAT\"\n",
    );
    let active = |kind: &str| json(d, &["list", "--kind", kind]).as_array().unwrap().len();
    // `done` is a built-in name, but bug closes at `verified`.
    ok(d, As::Claude, &["new", "--kind", "bug", "crash"]);
    ok(d, As::Claude, &["move", "BUG-1", "done"]);
    assert_eq!(active("bug"), 1, "done is not over for a bug");
    ok(d, As::Claude, &["move", "BUG-1", "verified"]);
    assert_eq!(active("bug"), 0);
    // `accepted` is rfc's closed state.
    ok(d, As::Claude, &["new", "--kind", "rfc", "an rfc"]);
    ok(d, As::Claude, &["move", "RFC-1", "accepted"]);
    assert_eq!(active("rfc"), 0, "accepted is over for an rfc");
    // Another kind closing at a state of that name does not close pattern's.
    declare(
        d,
        "[[kinds]]\nname = \"ticket\"\nstates = [\"new\", \"closed\"]\nkey = \"TKT\"\n",
    );
    ok(d, As::Claude, &["new", "--kind", "pattern", "a pattern"]);
    ok(d, As::Claude, &["move", "PAT-1", "closed"]);
    assert_eq!(active("pattern"), 1, "closed is not over for a pattern");
}

#[test]
fn a_dropped_kind_or_state_is_refused_naming_the_node_with_or_without_the_database() {
    let refused = |d: &Path, says: &[&str]| {
        let out = livtk(d, As::Claude, &["list"]);
        assert!(!out.status.success());
        let err = String::from_utf8_lossy(&out.stderr);
        for s in says {
            assert!(err.contains(s), "{s}: {err}");
        }
    };
    let fresh = |d: &Path| {
        for f in ["livtk.db", "livtk.db-wal", "livtk.db-shm"] {
            let _ = std::fs::remove_file(d.join(".livtk").join(f));
        }
    };
    // A kind, taken out of the config.
    let dir = project();
    let d = dir.path();
    let config = std::fs::read_to_string(d.join(".livtk/config.toml")).unwrap();
    declare(d, INCIDENT);
    ok(
        d,
        As::Claude,
        &["new", "--kind", "incident", "checkout is down"],
    );
    std::fs::write(d.join(".livtk/config.toml"), &config).unwrap();
    refused(d, &["incident", "INC-1"]);
    fresh(d);
    refused(d, &["incident", "INC-1"]);
    // A state, taken out of its kind.
    let dir = project();
    let d = dir.path();
    let config = std::fs::read_to_string(d.join(".livtk/config.toml")).unwrap();
    declare(d, INCIDENT);
    ok(
        d,
        As::Claude,
        &["new", "--kind", "incident", "checkout is down"],
    );
    ok(d, As::Claude, &["move", "INC-1", "mitigated"]);
    std::fs::write(d.join(".livtk/config.toml"), &config).unwrap();
    declare(
        d,
        "[[kinds]]\nname = \"incident\"\nstates = [\"open\", \"closed\"]\nkey = \"INC\"\n",
    );
    refused(d, &["mitigated", "INC-1"]);
    fresh(d);
    refused(d, &["mitigated", "INC-1"]);
    // A state still known by name, here a built-in one: refused on the very first command too.
    let dir = project();
    let d = dir.path();
    let config = std::fs::read_to_string(d.join(".livtk/config.toml")).unwrap();
    declare(
        d,
        "[[kinds]]\nname = \"bug\"\nstates = [\"open\", \"done\", \"verified\"]\nkey = \"BUG\"\n",
    );
    ok(d, As::Claude, &["new", "--kind", "bug", "crash"]);
    ok(d, As::Claude, &["move", "BUG-1", "done"]);
    std::fs::write(d.join(".livtk/config.toml"), &config).unwrap();
    declare(
        d,
        "[[kinds]]\nname = \"bug\"\nstates = [\"open\", \"verified\"]\nkey = \"BUG\"\n",
    );
    fresh(d);
    refused(d, &["done", "BUG-1"]);
}

#[test]
fn declared_relations_are_in_show_json_and_a_bad_kind_is_a_usage_error() {
    let dir = project();
    let d = dir.path();
    // Nothing declared: no such field.
    ok(d, As::Claude, &["new", "the importer"]);
    assert!(json(d, &["show", "LIV-1"]).get("declared").is_none());
    declare(
        d,
        "[[relations]]\nname = \"implements\"\ninverse = \"implemented_by\"\n",
    );
    ok(d, As::Claude, &["decision", "Import in one pass"]);
    ok(d, As::Claude, &["link", "LIV-1", "implements", "DEC-1"]);
    let from = json(d, &["show", "LIV-1"]);
    assert_eq!(from["declared"]["implements"]["to"][0]["key"], "DEC-1");
    let back = json(d, &["show", "DEC-1"]);
    assert_eq!(back["declared"]["implements"]["from"][0]["key"], "LIV-1");
    assert_eq!(back["declared"]["implements"]["inverse"], "implemented_by");
    // A kind or state no one declares: exit 2, as clap's own usage errors.
    let out = livtk(d, As::Claude, &["list", "--kind", "incidnet"]);
    assert_eq!(out.status.code(), Some(2));
    let out = livtk(d, As::Claude, &["move", "LIV-1", "nowhere"]);
    assert_eq!(out.status.code(), Some(2));
}

// ---- projects -----------------------------------------------------------------------------------

#[test]
fn a_project_is_proposed_committed_to_by_the_human_and_counts_what_waits_on_them() {
    let dir = project();
    let d = dir.path();
    // This project gates review -> done for the human, so review waits on them too.
    declare(d, "[gates]\nhuman = [\"task: review -> done\"]\n");
    // Rows read as always while there is no project.
    ok(d, As::Claude, &["new", "unrelated"]);
    assert!(json(d, &["list"])[0].get("project").is_none());
    let out = ok(
        d,
        As::Claude,
        &[
            "project",
            "new",
            "Import the board",
            "--body",
            "Bring it in.\n\nDone when: every card is a task.",
            "--target",
            "2026-12-01",
            "--owner",
            "karl",
        ],
    );
    assert!(out.contains("PRJ-1 proposed"), "{out}");
    assert!(
        ok(d, As::Claude, &["inbox"]).contains("PRJ-1"),
        "waits for the human"
    );
    let refused = livtk(d, As::Claude, &["move", "PRJ-1", "active"]);
    assert_eq!(refused.status.code(), Some(3), "the human commits");
    ok(d, As::Human, &["move", "PRJ-1", "active"]);
    for t in ["parse cards", "write tasks", "link them"] {
        ok(d, As::Claude, &["new", t, "--parent", "PRJ-1"]);
    }
    // LIV-2 planned (waits for its ack); LIV-3 in review (waits for done); LIV-4 in backlog.
    ok(d, As::Claude, &["move", "LIV-2", "planned"]);
    ok(d, As::Claude, &["move", "LIV-3", "planned"]);
    ok(d, As::Human, &["ack", "LIV-3"]);
    ok(d, As::Claude, &["start", "LIV-3"]);
    ok(d, As::Claude, &["move", "LIV-3", "review"]);
    let show = json(d, &["project", "show", "PRJ-1"]);
    let g = &show["progress"];
    assert_eq!(
        (g["done"].as_u64(), g["total"].as_u64()),
        (Some(0), Some(3))
    );
    let waiting: Vec<(String, u64, String)> = g["waiting"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            (
                w["state"].as_str().unwrap().to_owned(),
                w["count"].as_u64().unwrap(),
                w["oldest"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(
        waiting.contains(&("planned".into(), 1, "LIV-2".into())),
        "{waiting:?}"
    );
    assert!(
        waiting.contains(&("review".into(), 1, "LIV-3".into())),
        "{waiting:?}"
    );
    assert_eq!(g["target"], "2026-12-01");
    assert_eq!(g["owner"], "karl");
    // A task names its project in lists, text and JSON.
    assert!(ok(d, As::Claude, &["list"]).contains("parse cards  [PRJ-1]"));
    let rows = json(d, &["list"]);
    let row = |k: &str| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["key"] == k)
            .unwrap()
            .clone()
    };
    assert_eq!(row("LIV-2")["project"], "PRJ-1");
    assert!(row("LIV-1").get("project").is_none());
    // An update says how it stands.
    ok(
        d,
        As::Claude,
        &[
            "project",
            "update",
            "PRJ-1",
            "--health",
            "at-risk",
            "parsing is slow; next: batch it",
        ],
    );
    let list = ok(d, As::Claude, &["project", "list"]);
    assert!(
        list.contains("PRJ-1") && list.contains("at risk") && list.contains("0/3"),
        "{list}"
    );
    assert!(
        list.contains("1 planned") && list.contains("1 review"),
        "{list}"
    );
    // Brought to review, a task of an active project asks for its update; started, it names
    // the outcome it serves.
    ok(d, As::Claude, &["move", "LIV-4", "planned"]);
    ok(d, As::Human, &["ack", "LIV-4"]);
    let started = ok(d, As::Claude, &["start", "LIV-4"]);
    assert!(
        started.contains("part of PRJ-1 Import the board: Done when: every card is a task."),
        "{started}"
    );
    let reviewed = ok(d, As::Claude, &["move", "LIV-4", "review"]);
    assert!(reviewed.contains("PRJ-1 may want an update"), "{reviewed}");
    // Done once the human says so.
    ok(d, As::Human, &["done", "LIV-3"]);
    assert_eq!(
        json(d, &["project", "show", "PRJ-1"])["progress"]["done"],
        1
    );
    assert_eq!(
        livtk(d, As::Claude, &["move", "PRJ-1", "done"])
            .status
            .code(),
        Some(3)
    );
}

#[test]
fn more_than_three_active_projects_get_a_plain_line() {
    let dir = project();
    let d = dir.path();
    for i in 1..=4 {
        ok(d, As::Claude, &["project", "new", &format!("outcome {i}")]);
        ok(d, As::Human, &["move", &format!("PRJ-{i}"), "active"]);
        if i == 3 {
            assert!(!ok(d, As::Claude, &["project", "list"]).contains("active at once"));
        }
    }
    let list = ok(d, As::Claude, &["project", "list"]);
    assert!(list.contains("4 projects are active at once"), "{list}");
}

#[test]
fn flow_gives_time_per_state_and_queue_ages_for_all_or_one_project() {
    let dir = project();
    let d = dir.path();
    ok(d, As::Claude, &["project", "new", "outcome"]);
    ok(d, As::Claude, &["new", "in it", "--parent", "PRJ-1"]);
    ok(d, As::Claude, &["new", "elsewhere"]);
    ok(d, As::Claude, &["move", "LIV-1", "planned"]);
    ok(d, As::Claude, &["move", "LIV-2", "planned"]);
    let all = json(d, &["flow"]);
    let backlog = all["states"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["state"] == "backlog")
        .unwrap()
        .clone();
    assert_eq!(backlog["tasks"], 2);
    assert!(backlog["median_secs"].is_i64() && backlog["total_secs"].is_i64());
    assert_eq!(all["queues"][0]["state"], "planned");
    assert_eq!(all["queues"][0]["count"], 2);
    assert!(all["queues"][0]["oldest_secs"].is_i64());
    assert!(all["lead"].is_null());
    // One project: its own task only.
    let one = json(d, &["flow", "--project", "PRJ-1"]);
    assert_eq!(one["queues"][0]["count"], 1);
    assert_eq!(one["queues"][0]["oldest"], "LIV-1");
    assert!(ok(d, As::Claude, &["flow"]).contains("Waiting on the human:"));
    assert!(
        livtk(d, As::Claude, &["flow", "--project", "LIV-1"])
            .status
            .code()
            != Some(0)
    );
}
