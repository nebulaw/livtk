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

/// How long a test waits for a turn to start or a run to get somewhere. A stub turn is a shell
/// script, quick alone, but it can wait seconds for the machine while other builds and tests
/// run: the wait only bounds a hang, it never paces a passing test.
const WAIT: Duration = Duration::from_secs(30);

/// A stub `claude`: each turn takes the next word from `.stub-turns` beside it (the project's
/// folder, wherever the turn works), writes its arguments to `.stub-args` there, and answers as
/// the word says.
const STUB: &str = r#"#!/bin/sh
here=$(dirname "$0")
f="$here/.stub-turns"
what=$(head -n 1 "$f")
tail -n +2 "$f" > "$f.tmp"; mv "$f.tmp" "$f"
printf '%s\n' "$*" >> "$here/.stub-args"
echo '{"type":"system","subtype":"init","session_id":"stub"}'
case "$what" in
  ask) echo '{"type":"result","subtype":"success","is_error":false,"result":"Which colour should it be?","permission_denials":[]}' ;;
  deny) echo '{"type":"result","subtype":"success","is_error":false,"result":"I need to run the tests.","permission_denials":[{"tool_name":"Bash","tool_use_id":"t1","tool_input":{"command":"cargo test && cargo clippy"}}]}' ;;
  done) echo '{"type":"result","subtype":"success","is_error":false,"result":"Built, and in review.","permission_denials":[]}' ;;
  push) echo '{"type":"result","subtype":"success","is_error":false,"result":"Pushing it.","permission_denials":[{"tool_name":"Bash","tool_use_id":"t1","tool_input":{"command":"git push origin main"}}]}' ;;
  slow) trap 'echo "{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"is_error\":true,\"result\":\"\"}"; exit 0' INT
        while :; do sleep 0.05; done ;;
  path) printf '%s' "$PATH" > "$here/.stub-path"
        echo '{"type":"result","subtype":"success","is_error":false,"result":"Done.","permission_denials":[]}' ;;
  commit) pwd > built.txt && git add built.txt && git commit -qm "build it"
          echo '{"type":"result","subtype":"success","is_error":false,"result":"Built.","permission_denials":[]}' ;;
  dirty) pwd > left.txt
         echo '{"type":"result","subtype":"success","is_error":false,"result":"Left it.","permission_denials":[]}' ;;
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
            let _ = run::kill_turn(&live);
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
        assert!(start.elapsed() < WAIT, "waited for {what}: {r:?}");
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
            start.elapsed() < WAIT,
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
        runner.answer(&mut ws, &key, true, false, &CLAUDE).is_err(),
        "nor through the runner"
    );
    runner
        .answer(&mut ws, &key, true, false, &Actor::Human)
        .unwrap();
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
        assert!(start.elapsed() < WAIT);
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

/// A turn runs with the tools it needs on its PATH, whatever PATH started it: claude's folder
/// and the usual install folders first.
#[test]
fn a_turn_finds_its_tools_on_its_path() {
    let (_dir, mut ws, runner, key) = setup(&["path"]);
    let _runs = KillRuns(ws.dir().to_path_buf());
    runner
        .start(&mut ws, &key, None, None, &[], &Actor::Human)
        .unwrap();
    let file = root(&ws).join(".stub-path");
    let start = Instant::now();
    while !file.exists() {
        assert!(start.elapsed() < WAIT, "the turn never ran");
        std::thread::sleep(Duration::from_millis(20));
    }
    let path = std::fs::read_to_string(file).unwrap();
    let dirs: Vec<&str> = path.split(':').collect();
    let stub_dir = root(&ws);
    assert!(
        dirs.contains(&stub_dir.to_str().unwrap()),
        "claude's folder: {path}"
    );
    for usual in [
        "/.cargo/bin",
        "/.local/bin",
        "/opt/homebrew/bin",
        "/usr/local/bin",
    ] {
        assert!(dirs.iter().any(|d| d.ends_with(usual)), "{usual}: {path}");
    }
}

/// `git args` in `dir`, which must work.
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `setup`, as a git repository with everything committed, and more acknowledged tasks given
/// to an agent: `extra` of them, their keys returned after the first's.
fn git_setup(turns: &[&str], extra: usize) -> (tempfile::TempDir, Workspace, Runner, Vec<String>) {
    let (dir, mut ws, runner, key) = setup(turns);
    let mut keys = vec![key];
    for i in 0..extra {
        let e = ws
            .execute(
                Command::Create {
                    kind: Kind::Task,
                    title: format!("more work {i}"),
                    body: String::new(),
                    priority: None,
                    links: vec![],
                },
                &CLAUDE,
            )
            .unwrap();
        keys.push(ws.node(&e[0].op.subjects().0.to_string()).unwrap().key);
    }
    for k in &keys {
        if ws.node(k).unwrap().state != State::Acked {
            ws.execute(
                Command::Move {
                    node: k.clone(),
                    to: State::Planned,
                },
                &CLAUDE,
            )
            .unwrap();
            ws.execute(
                Command::Move {
                    node: k.clone(),
                    to: State::Acked,
                },
                &Actor::Human,
            )
            .unwrap();
        }
        ws.execute(
            Command::SetAgent {
                node: k.clone(),
                agent: "livtk:implementer".into(),
            },
            &Actor::Human,
        )
        .unwrap();
    }
    let root = root(&ws);
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "t@t"]);
    git(&root, &["config", "user.name", "t"]);
    std::fs::write(root.join(".gitignore"), ".stub-*\nclaude-stub\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "init"]);
    (dir, ws, runner, keys)
}

fn auto(ws: &mut Workspace, on: bool, at_once: u8) {
    ws.set_runs(&livtk_core::Runs {
        auto: on,
        builds_at_once: at_once,
        worktree: true,
        ..livtk_core::Runs::default()
    })
    .unwrap();
}

/// Off (the default), nothing starts on its own. On, acknowledged tasks with an agent start
/// building, as many as the project builds at once, the rest waiting their turn in order; each
/// build works in its own worktree, on its own branch.
#[test]
fn builds_start_on_their_own_up_to_the_limit_each_in_its_worktree() {
    let (_dir, mut ws, runner, keys) = git_setup(&["slow", "slow", "slow"], 2);
    let _runs = KillRuns(ws.dir().to_path_buf());
    run::settle(&mut ws, &runner).unwrap();
    assert!(ws.runs().unwrap().is_empty(), "off: nothing starts");
    assert!(run::queue(&ws).unwrap().is_empty(), "and nothing queues");
    auto(&mut ws, true, 2);
    run::settle(&mut ws, &runner).unwrap();
    let runs = ws.runs().unwrap();
    assert_eq!(runs.len(), 2, "two at once: {runs:?}");
    let queued: Vec<String> = run::queue(&ws)
        .unwrap()
        .into_iter()
        .map(|t| t.key)
        .collect();
    assert_eq!(queued.len(), 1, "the third waits its turn");
    assert!(!runs.iter().any(|r| r.key == queued[0]));
    for r in &runs {
        let live = run::live(ws.dir(), &r.session).unwrap();
        assert_eq!(
            live.cwd,
            run::worktree(ws.dir(), &r.key).unwrap(),
            "in its worktree"
        );
        assert!(live.cwd.join(".livtk/config.toml").exists());
    }
    // Settled again, nothing more starts while two go.
    run::settle(&mut ws, &runner).unwrap();
    assert_eq!(ws.runs().unwrap().len(), 2);
    assert_eq!(keys.len(), 3);
}

/// A build's branch comes into the checkout when its task is done, fast-forward, its worktree
/// gone; one that cannot says why, once.
#[test]
fn a_done_build_is_merged_or_says_why_not() {
    let (_dir, mut ws, runner, keys) = git_setup(&["commit", "commit"], 1);
    let _runs = KillRuns(ws.dir().to_path_buf());
    let root = root(&ws);
    for k in &keys {
        runner
            .start(&mut ws, k, None, None, &[], &Actor::Human)
            .unwrap();
        until(&mut ws, &runner, k, "the build", |r| {
            r.ended().is_some() || r.needs_you()
        });
    }
    // The first: done, and merged.
    let first = &keys[0];
    for to in [State::Doing, State::Review] {
        ws.execute(
            Command::Move {
                node: first.clone(),
                to,
            },
            &CLAUDE,
        )
        .unwrap();
    }
    ws.execute(
        Command::Move {
            node: first.clone(),
            to: State::Done,
        },
        &Actor::Human,
    )
    .unwrap();
    run::settle(&mut ws, &runner).unwrap();
    assert!(root.join("built.txt").exists(), "merged into the checkout");
    assert!(
        run::worktree(ws.dir(), first).is_none(),
        "its worktree gone"
    );
    let said = |ws: &Workspace, k: &str, what: &str| {
        ws.history(Some(k))
            .unwrap()
            .iter()
            .filter(|e| matches!(&e.op, livtk_core::event::Op::NodeNote { data, .. } if data.text.starts_with(what)))
            .count()
    };
    assert_eq!(said(&ws, first, "Merged livtk/"), 1);
    // The second also made built.txt, from the old HEAD: no fast-forward now.
    let second = &keys[1];
    for to in [State::Doing, State::Review] {
        ws.execute(
            Command::Move {
                node: second.clone(),
                to,
            },
            &CLAUDE,
        )
        .unwrap();
    }
    ws.execute(
        Command::Move {
            node: second.clone(),
            to: State::Done,
        },
        &Actor::Human,
    )
    .unwrap();
    run::settle(&mut ws, &runner).unwrap();
    run::settle(&mut ws, &runner).unwrap();
    assert_eq!(
        said(&ws, second, "Could not merge livtk/"),
        1,
        "said once: {:#?}",
        ws.history(Some(second))
            .unwrap()
            .iter()
            .filter_map(|e| match &e.op {
                livtk_core::event::Op::NodeNote { data, .. } => Some(data.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    assert!(
        run::worktree(ws.dir(), second).is_some(),
        "kept to bring in by hand"
    );
}

/// With the project starting runs on its own, a task coming into review starts its review.
#[test]
fn a_review_starts_on_its_own() {
    let (_dir, mut ws, runner, keys) = git_setup(&["ask"], 0);
    let _runs = KillRuns(ws.dir().to_path_buf());
    let k = &keys[0];
    // Built by hand first (so it is not queued), then in review.
    for to in [State::Doing, State::Review] {
        ws.execute(
            Command::Move {
                node: k.clone(),
                to,
            },
            &CLAUDE,
        )
        .unwrap();
    }
    auto(&mut ws, true, 2);
    run::settle(&mut ws, &runner).unwrap();
    let r = ws.runs().unwrap().pop().expect("a review");
    assert_eq!(r.stage, Stage::Review);
    assert_eq!(
        r.agent.as_deref(),
        Some("livtk:refuter"),
        "as the reviewer, not the builder"
    );
}

/// A build that left work uncommitted is not called merged: said once, its worktree kept.
#[test]
fn a_build_with_uncommitted_work_is_not_merged() {
    let (_dir, mut ws, runner, keys) = git_setup(&["dirty"], 0);
    let _runs = KillRuns(ws.dir().to_path_buf());
    let k = &keys[0];
    runner
        .start(&mut ws, k, None, None, &[], &Actor::Human)
        .unwrap();
    until(&mut ws, &runner, k, "the build", |r| {
        r.ended().is_some() || r.needs_you()
    });
    for to in [State::Doing, State::Review] {
        ws.execute(
            Command::Move {
                node: k.clone(),
                to,
            },
            &CLAUDE,
        )
        .unwrap();
    }
    ws.execute(
        Command::Move {
            node: k.clone(),
            to: State::Done,
        },
        &Actor::Human,
    )
    .unwrap();
    for _ in 0..3 {
        run::settle(&mut ws, &runner).unwrap();
    }
    let notes: Vec<String> = ws
        .history(Some(k))
        .unwrap()
        .iter()
        .filter_map(|e| match &e.op {
            livtk_core::event::Op::NodeNote { data, .. } => Some(data.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        notes.iter().filter(|t| t.starts_with("Not merged")).count(),
        1,
        "{notes:#?}"
    );
    assert!(!notes.iter().any(|t| t.starts_with("Merged")), "{notes:#?}");
    assert!(
        notes.iter().any(|t| t.contains("left.txt")),
        "names what it left"
    );
    assert!(run::worktree(ws.dir(), k).is_some(), "kept");
}

/// A build whose worktree cannot be made ends its run, saying why: it neither waits its turn
/// for ever nor holds a build's place.
#[test]
fn a_worktree_that_cannot_be_made_ends_the_run() {
    let (_dir, mut ws, runner, keys) = git_setup(&["silent"], 0);
    let _runs = KillRuns(ws.dir().to_path_buf());
    let k = &keys[0];
    runner
        .start(&mut ws, k, None, None, &[], &Actor::Human)
        .unwrap();
    until(&mut ws, &runner, k, "the first build", |r| {
        r.ended().is_some()
    });
    // Its worktree removed by hand, still registered with git: it cannot be made again.
    let tree = run::worktree(ws.dir(), k).unwrap();
    std::fs::remove_dir_all(&tree).unwrap();
    let err = runner
        .start(&mut ws, k, None, None, &[], &Actor::Human)
        .unwrap_err()
        .to_string();
    let last = ws.runs().unwrap().pop().unwrap();
    assert_eq!(last.ended(), Some(Ending::Failed), "{err}");
    assert!(
        last.text.starts_with("Its worktree could not be made"),
        "{}",
        last.text
    );
    auto(&mut ws, true, 1);
    assert!(run::queue(&ws).unwrap().is_empty(), "not waiting its turn");
}

/// A run is refused what is never allowed outright: said on the task, answered at once, and it
/// goes on without, never waiting for the human. Its turns are given the never-rules, and its
/// stage's whole job to do without asking.
#[test]
fn what_is_never_allowed_is_refused_not_asked() {
    let (_dir, mut ws, runner, key) = setup(&["push", "done"]);
    let _kill = KillRuns(ws.dir().to_path_buf());
    runner
        .start(&mut ws, &key, None, None, &[], &Actor::Human)
        .unwrap();
    let first = turn_args(&root(&ws), 1);
    assert!(
        first.contains("--disallowedTools") && first.contains("Bash(git push:*)"),
        "{first}"
    );
    assert!(
        first.contains("Bash(cargo:*)") && first.contains("Bash(git commit:*)"),
        "{first}"
    );
    // It goes on by itself to its next turn, which asks its own question: the refusal never
    // waited for the human.
    until(&mut ws, &runner, &key, "the next turn's question", |r| {
        r.needs_you() && r.asks_to_run().is_empty()
    });
    let resumed = turn_args(&root(&ws), 2);
    assert!(
        resumed.contains("Not allowed: Bash(git push origin main)"),
        "{resumed}"
    );
    let notes = ws.history(Some(&key)).unwrap();
    assert!(
        notes.iter().any(
            |e| matches!(&e.op, livtk_core::event::Op::NodeNote { data, .. }
            if data.text.contains("Not allowed, ever: Bash(git push origin main)"))
        ),
        "said on the task"
    );
}

/// Allowed always: the kind of command is the project's runs of that stage's from now on, in
/// its config, and the next run is given it.
#[test]
fn allowed_always_is_kept_for_the_stage() {
    let (_dir, mut ws, runner, key) = setup(&["deny", "done"]);
    let _kill = KillRuns(ws.dir().to_path_buf());
    runner
        .start(&mut ws, &key, None, None, &[], &Actor::Human)
        .unwrap();
    until(&mut ws, &runner, &key, "the ask", run::Run::needs_you);
    runner
        .answer(&mut ws, &key, true, true, &Actor::Human)
        .unwrap();
    let ws2 = Workspace::open(&root(&ws)).unwrap();
    let own = ws2
        .config()
        .runs
        .allow
        .get("build")
        .cloned()
        .unwrap_or_default();
    assert!(own.contains(&"Bash(cargo test:*)".to_owned()), "{own:?}");
    assert!(own.contains(&"Bash(cargo clippy:*)".to_owned()), "{own:?}");
    let perms = livtk_core::permits::permits(Stage::Build, &ws2.config().runs.rules());
    assert!(perms.allow.contains(&"Bash(cargo test:*)".to_owned()));
}

/// A task says whether its builds work in their own worktree; else the project does.
#[test]
fn a_task_says_where_its_builds_work() {
    let (_dir, mut ws, _runner, key) = setup(&["done"]);
    assert!(run::wants_worktree(&ws, &key), "as the project: its own");
    ws.execute(
        Command::SetWorktree {
            node: key.clone(),
            worktree: Some(false),
        },
        &Actor::Human,
    )
    .unwrap();
    assert!(
        !run::wants_worktree(&ws, &key),
        "in the checkout, as it says"
    );
    ws.execute(
        Command::SetWorktree {
            node: key.clone(),
            worktree: None,
        },
        &Actor::Human,
    )
    .unwrap();
    assert!(run::wants_worktree(&ws, &key), "back to the project's");
}

#[test]
fn a_here_document_is_one_command() {
    assert_eq!(
        run::simple_commands("python3 - <<'EOF'\nimport json\nfor e in ev: print(e)\nEOF\nls"),
        ["python3 - <<'EOF'", "ls"]
    );
    assert_eq!(
        run::simple_commands("cat <<EOF | wc -l\na; b\nEOF"),
        ["cat <<EOF", "wc -l"]
    );
    // A `<<` in quotes or arithmetic opens nothing: the next line is a command.
    assert_eq!(
        run::simple_commands("echo \"a <<b\"\ngit push"),
        ["echo \"a <<b\"", "git push"]
    );
    assert_eq!(
        run::simple_commands("echo $((1<<2))\ngit push"),
        ["echo $((1<<2))", "git push"]
    );
    // An ending word with a dash, quoted, or after a backslash; two on one line, in turn.
    assert_eq!(
        run::simple_commands("cat <<\"END-X\"\nEND\nmore\nEND-X\nls"),
        ["cat <<\"END-X\"", "ls"]
    );
    assert_eq!(
        run::simple_commands("cat <<\\EOF\nrm -rf x\nEOF\nls"),
        ["cat <<\\EOF", "ls"]
    );
    assert_eq!(
        run::simple_commands("cat <<A && cat <<B\none\nA\ntwo\nB\ngit push"),
        ["cat <<A", "cat <<B", "git push"]
    );
    // A here-string is no here-document.
    assert_eq!(
        run::simple_commands("grep x <<< \"y\"\nls"),
        ["grep x <<< \"y\"", "ls"]
    );
}

/// A turn's pid left in its record after the turn ended may name another process's group by
/// now: killing the turn leaves that process alone.
#[test]
fn killing_a_turn_never_kills_a_process_that_took_its_pid() {
    use std::os::unix::process::CommandExt;
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    let live = run::Live {
        session: "01a1-a-turn-long-gone".into(),
        key: "LIV-1".into(),
        agent: None,
        stage: Stage::Build,
        cwd: PathBuf::from("/"),
        allowed: vec![],
        read: 0,
        pid: Some(other.id()),
        stopping: None,
        stop_asked: None,
    };
    assert!(
        !run::kill_turn(&live).unwrap(),
        "not that turn's: nothing sent"
    );
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        other.try_wait().unwrap().is_none(),
        "the other process lives on"
    );
    other.kill().unwrap();
    let _ = other.wait();
}
