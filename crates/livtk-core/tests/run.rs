//! Runs of Claude on a task, against a stub `claude` that speaks the stream Claude Code writes:
//! started, asking to run something, allowed, asking a question, answered by a note, done; a
//! stop that keeps the work; a turn that goes without a word.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use livtk_core::event::{Ending, RunMark};
use livtk_core::run::{self, Answer, Runner, Stage};
use livtk_core::{Actor, Command, Kind, State, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

/// A stub `claude`: each turn takes the next word from `.stub-turns` in the folder it runs in,
/// writes its arguments to `.stub-args`, and answers as the word says.
const STUB: &str = r#"#!/bin/sh
f=.stub-turns
what=$(head -n 1 "$f")
tail -n +2 "$f" > "$f.tmp"; mv "$f.tmp" "$f"
printf '%s\n' "$*" >> .stub-args
echo '{"type":"system","subtype":"init","session_id":"stub"}'
case "$what" in
  ask) echo '{"type":"result","subtype":"success","is_error":false,"result":"Which colour should it be?","permission_denials":[]}' ;;
  deny) echo '{"type":"result","subtype":"success","is_error":false,"result":"I need to run the tests.","permission_denials":[{"tool_name":"Bash","tool_use_id":"t1","tool_input":{"command":"cargo test && cargo clippy"}}]}' ;;
  done) echo '{"type":"result","subtype":"success","is_error":false,"result":"Built, and in review.","permission_denials":[]}' ;;
  slow) trap 'echo "{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"is_error\":true,\"result\":\"\"}"; exit 0' INT
        while :; do sleep 0.05; done ;;
  deaf) trap '' INT
        echo '{"type":"system","subtype":"init"}'
        while :; do sleep 0.05; done ;;
  *) exit 1 ;;
esac
"#;

/// Ends every run's turn in the project when dropped: a failing test leaves no stub looping.
struct KillRuns(PathBuf);

impl Drop for KillRuns {
    fn drop(&mut self) {
        for live in run::lives(&self.0) {
            if let Some(pid) = live.pid {
                let _ = std::process::Command::new("kill")
                    .args(["-KILL", &format!("-{pid}")])
                    .status();
            }
        }
    }
}

fn setup(turns: &[&str]) -> (tempfile::TempDir, Workspace, Runner, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut ws = Workspace::init(&root, Some("LIV")).unwrap();
    let e = ws
        .execute(
            Command::Create {
                kind: Kind::Task,
                title: "paint the shed".into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &CLAUDE,
        )
        .unwrap();
    let key = ws.node(&e[0].op.subjects().0.to_string()).unwrap().key;
    for to in [State::Planned, State::Acked] {
        let by = if to == State::Acked {
            Actor::Human
        } else {
            CLAUDE
        };
        ws.execute(
            Command::Move {
                node: key.clone(),
                to,
            },
            &by,
        )
        .unwrap();
    }
    let claude = root.join("claude-stub");
    std::fs::write(&claude, STUB).unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join(".stub-turns"), turns.join("\n") + "\n").unwrap();
    (dir, ws, Runner { claude }, key)
}

/// Settle until `done` holds of the task's run, or fail after a while.
fn until(
    ws: &mut Workspace,
    runner: &Runner,
    key: &str,
    what: &str,
    done: impl Fn(&run::Run) -> bool,
) -> run::Run {
    let start = Instant::now();
    loop {
        run::settle(ws, runner).unwrap();
        let r = ws
            .runs()
            .unwrap()
            .into_iter()
            .rev()
            .find(|r| r.key == key)
            .expect("a run");
        if done(&r) {
            return r;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "waited for {what}: {r:?}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn args(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join(".stub-args"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The arguments of turn `n` (from 1), once that turn has started.
fn turn_args(root: &Path, n: usize) -> String {
    let start = Instant::now();
    loop {
        if let Some(a) = args(root).get(n - 1) {
            return a.clone();
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "turn {n} never started: {:?} {:?}",
            args(root),
            std::fs::read_dir(root.join(".livtk/runs"))
                .unwrap()
                .flatten()
                .map(|e| (
                    e.file_name(),
                    std::fs::read_to_string(e.path()).unwrap_or_default()
                ))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn root(ws: &Workspace) -> PathBuf {
    ws.root().to_path_buf()
}

#[test]
fn a_build_asks_is_allowed_asks_again_is_answered_and_ends_done() {
    let (_dir, mut ws, runner, key) = setup(&["deny", "ask", "done"]);
    let live = runner
        .start(&mut ws, &key, None, None, &[], &Actor::Human)
        .unwrap();
    assert_eq!(live.stage, Stage::Build, "an acknowledged task is built");
    assert_eq!(live.agent.as_deref(), Some("livtk:implementer"));
    let first = &turn_args(&root(&ws), 1);
    assert!(
        first.contains("--session-id") && first.contains("--agent livtk:implementer"),
        "{first}"
    );
    assert!(
        first.contains("--permission-prompts none") && first.contains("acceptEdits"),
        "{first}"
    );
    // A second run on it is refused while this one goes.
    assert!(
        runner
            .start(&mut ws, &key, None, None, &[], &Actor::Human)
            .is_err()
    );

    // Refused two commands: it asks to run each, and the task waits for you in the inbox.
    let r = until(&mut ws, &runner, &key, "the ask", run::Run::needs_you);
    assert_eq!(r.asks_to_run(), ["Bash(cargo test)", "Bash(cargo clippy)"]);
    assert!(
        ws.inbox().unwrap().iter().any(|s| s.key == key),
        "in the inbox"
    );
    // Only the human lets it.
    let mut claude_allows = Workspace::open(&root(&ws)).unwrap();
    assert!(
        claude_allows
            .execute(
                Command::RunNote {
                    node: key.clone(),
                    text: "Allowed".into(),
                    run: livtk_core::event::RunData {
                        session: r.session.clone(),
                        agent: None,
                        stage: Stage::Build,
                        mark: RunMark::Allowed,
                    },
                },
                &CLAUDE,
            )
            .is_err()
    );
    assert!(
        runner.answer(&mut ws, &key, true, &CLAUDE).is_err(),
        "nor through the runner"
    );
    runner.answer(&mut ws, &key, true, &Actor::Human).unwrap();
    let resumed = turn_args(&root(&ws), 2);
    assert!(
        resumed.contains("--resume") && resumed.contains("Bash(cargo test)"),
        "{resumed}"
    );

    // Then a question: answered by your note, which it reads next.
    let r = until(&mut ws, &runner, &key, "the question", |r| {
        r.needs_you() && r.asks_to_run().is_empty()
    });
    assert_eq!(r.text, "Which colour should it be?");
    // The task in review, as the next turn will leave it: then the turn ends the run, done.
    ws.execute(
        Command::Move {
            node: key.clone(),
            to: State::Doing,
        },
        &CLAUDE,
    )
    .unwrap();
    ws.execute(
        Command::Move {
            node: key.clone(),
            to: State::Review,
        },
        &CLAUDE,
    )
    .unwrap();
    ws.execute(
        Command::Note {
            node: key.clone(),
            text: "Blue.".into(),
        },
        &Actor::Human,
    )
    .unwrap();
    assert!(matches!(ws.runs().unwrap()[0].answer, Some(Answer::Note(ref t)) if t == "Blue."));
    run::settle(&mut ws, &runner).unwrap();
    let resumed = turn_args(&root(&ws), 3);
    assert!(
        resumed.contains("-p Blue.") && resumed.contains("--resume"),
        "{resumed}"
    );
    // Settling again while that turn goes starts nothing more.
    run::settle(&mut ws, &runner).unwrap();
    let r = until(&mut ws, &runner, &key, "the end", |r| r.ended().is_some());
    assert_eq!(r.ended(), Some(Ending::Done));
    assert_eq!(r.text, "Built, and in review.");
    assert_eq!(args(&root(&ws)).len(), 3, "one process a turn");
    assert!(!ws.inbox().unwrap().iter().any(|s| s.key == key));

    // The runs are the log's: rebuilt, the same.
    let before = ws.runs().unwrap();
    ws.rebuild().unwrap();
    assert_eq!(ws.runs().unwrap(), before);
    // Nothing of a run's live state is committed.
    let ignore = std::fs::read_to_string(run::runs_dir(ws.dir()).join(".gitignore")).unwrap();
    assert_eq!(ignore.trim(), "*");
}

#[test]
fn a_stop_ends_the_turn_keeps_the_work_and_it_can_continue() {
    let (_dir, mut ws, runner, key) = setup(&["slow", "done"]);
    let _runs = KillRuns(ws.dir().to_path_buf());
    runner
        .start(&mut ws, &key, None, None, &[], &Actor::Human)
        .unwrap();
    let live = run::lives(ws.dir()).pop().unwrap();
    let start = Instant::now();
    while !run::writing(&live) {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(20));
    }
    // Its turn under way (the stub ready for the interrupt).
    turn_args(&root(&ws), 1);
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        run::writing_sessions(ws.dir()).contains(&live.session),
        "writing, beside the log"
    );
    runner
        .stop(&mut ws, &key, Ending::Quit, &Actor::Human)
        .unwrap();
    let r = until(&mut ws, &runner, &key, "the stop", |r| r.ended().is_some());
    assert_eq!(r.ended(), Some(Ending::Quit));
    assert!(!run::writing_sessions(ws.dir()).contains(&live.session));
    // Continued: the same session, resumed.
    runner.resume_stopped(&mut ws, &key, &Actor::Human).unwrap();
    let resumed = turn_args(&root(&ws), 2);
    assert!(
        resumed.contains(&format!("--resume {}", live.session)),
        "{resumed}"
    );
}

#[test]
fn a_turn_that_goes_without_a_word_fails_its_run() {
    let (_dir, mut ws, runner, key) = setup(&["silent"]);
    runner
        .start(&mut ws, &key, None, None, &[], &Actor::Human)
        .unwrap();
    let r = until(&mut ws, &runner, &key, "the failure", |r| {
        r.ended().is_some()
    });
    assert_eq!(r.ended(), Some(Ending::Failed));
}

#[test]
fn stages_follow_the_task_and_agents_suggest_themselves() {
    assert_eq!(
        Stage::for_state(Kind::Task, State::Backlog),
        Some(Stage::Plan)
    );
    assert_eq!(
        Stage::for_state(Kind::Task, State::Acked),
        Some(Stage::Build)
    );
    assert_eq!(
        Stage::for_state(Kind::Task, State::Review),
        Some(Stage::Review)
    );
    assert_eq!(Stage::for_state(Kind::Task, State::Doing), None);
    assert_eq!(Stage::for_state(Kind::Lesson, State::Proposed), None);
    assert_eq!(
        run::suggest(Stage::Review, &[]).as_deref(),
        Some("livtk:refuter")
    );
    let file = |place, name: &str, stage| livtk_core::agents::AgentFile {
        place,
        path: PathBuf::new(),
        agent: livtk_core::agents::Agent {
            name: name.into(),
            stage,
            ..Default::default()
        },
    };
    use livtk_core::agents::Place;
    let files = [
        file(Place::Livtk, "implementer", Some(Stage::Build)),
        file(Place::Project, "builder", Some(Stage::Build)),
    ];
    assert_eq!(
        run::suggest(Stage::Build, &files).as_deref(),
        Some("builder"),
        "the project's first"
    );
    assert_eq!(
        run::suggest(Stage::Build, &files[..1]).as_deref(),
        Some("livtk:implementer")
    );
    assert_eq!(
        run::simple_commands("cargo test && echo 'a && b' | wc -l; ls"),
        ["cargo test", "echo 'a && b'", "wc -l", "ls"]
    );
}

/// A turn deaf to the interrupt (as one started from a shell script's background inherits it)
/// is told, then made, to stop: the run ends stopped all the same.
#[test]
fn a_stop_not_heard_is_made() {
    let (_dir, mut ws, runner, key) = setup(&["deaf"]);
    let _runs = KillRuns(ws.dir().to_path_buf());
    runner
        .start(&mut ws, &key, None, None, &[], &Actor::Human)
        .unwrap();
    turn_args(&root(&ws), 1);
    std::thread::sleep(Duration::from_millis(200));
    runner
        .stop(&mut ws, &key, Ending::Stopped, &Actor::Human)
        .unwrap();
    let r = until(&mut ws, &runner, &key, "the forced stop", |r| {
        r.ended().is_some()
    });
    assert_eq!(r.ended(), Some(Ending::Stopped));
}

/// A task that waits for unfinished work cannot be started to build, and says what it waits
/// for; once that is finished, it can.
#[test]
fn a_blocked_task_is_not_built_until_what_it_waits_for_is_finished() {
    let (_dir, mut ws, runner, key) = setup(&["done"]);
    let _runs = KillRuns(ws.dir().to_path_buf());
    let e = ws
        .execute(
            Command::Create {
                kind: Kind::Task,
                title: "lay the floor".into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &CLAUDE,
        )
        .unwrap();
    let first = ws.node(&e[0].op.subjects().0.to_string()).unwrap().key;
    ws.execute(
        Command::Link {
            src: key.clone(),
            rel: livtk_core::Rel::DependsOn,
            dst: first.clone(),
        },
        &CLAUDE,
    )
    .unwrap();
    let refused = runner
        .start(&mut ws, &key, None, None, &[], &Actor::Human)
        .unwrap_err()
        .to_string();
    assert!(refused.contains(&format!("waits for {first}")), "{refused}");
    // Planning it would not wait.
    assert_eq!(
        run::waits_for(Stage::Plan, true, &[]),
        None,
        "only a build waits"
    );
    ws.execute(
        Command::Move {
            node: first,
            to: State::Canceled,
        },
        &Actor::Human,
    )
    .unwrap();
    runner
        .start(&mut ws, &key, None, None, &[], &Actor::Human)
        .expect("free to build");
}

/// A stopped build whose task has since come to wait for unfinished work is not continued
/// either: every way a build starts asks the same.
#[test]
fn a_stopped_build_is_not_continued_once_its_task_waits() {
    let (_dir, mut ws, runner, key) = setup(&["silent"]);
    let _runs = KillRuns(ws.dir().to_path_buf());
    runner
        .start(&mut ws, &key, None, None, &[], &Actor::Human)
        .unwrap();
    until(&mut ws, &runner, &key, "the turn's end", |r| {
        r.ended().is_some()
    });
    // Ended failed (a silent turn); stopped is what continue takes: stop a fresh one instead.
    let e = ws
        .execute(
            Command::Create {
                kind: Kind::Task,
                title: "lay the floor".into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &CLAUDE,
        )
        .unwrap();
    let first = ws.node(&e[0].op.subjects().0.to_string()).unwrap().key;
    std::fs::write(ws.root().join(".stub-turns"), "slow\n").unwrap();
    runner
        .start(&mut ws, &key, None, None, &[], &Actor::Human)
        .unwrap();
    turn_args(&root(&ws), 2);
    std::thread::sleep(Duration::from_millis(200));
    runner
        .stop(&mut ws, &key, Ending::Stopped, &Actor::Human)
        .unwrap();
    until(&mut ws, &runner, &key, "the stop", |r| r.ended().is_some());
    ws.execute(
        Command::Link {
            src: key.clone(),
            rel: livtk_core::Rel::DependsOn,
            dst: first.clone(),
        },
        &CLAUDE,
    )
    .unwrap();
    let refused = runner
        .resume_stopped(&mut ws, &key, &Actor::Human)
        .unwrap_err()
        .to_string();
    assert!(refused.contains(&format!("waits for {first}")), "{refused}");
}
