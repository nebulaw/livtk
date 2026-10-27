//! What a session is called: its task, what it did, and who, from the log and its runs.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use livtk_core::event::{RunData, RunMark};
use livtk_core::run::Stage;
use livtk_core::sessions::{Doing, Naming};
use livtk_core::{Actor, Command, Kind, State, Workspace};

fn project() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    for t in ["first", "second", "third"] {
        ws.execute(
            Command::Create {
                kind: Kind::Task,
                title: t.into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &Actor::Human,
        )
        .unwrap();
    }
    (dir, ws)
}

fn claude(agent: Option<&str>) -> Actor {
    Actor::Claude {
        agent: agent.map(str::to_owned),
    }
}

/// A run on `key` in session `session`, by `agent`, at `stage`: its first note.
fn run(ws: &mut Workspace, key: &str, session: &str, agent: Option<&str>, stage: Stage) {
    ws.execute(
        Command::RunNote {
            node: key.into(),
            text: "Started.".into(),
            run: RunData {
                session: session.into(),
                agent: agent.map(str::to_owned),
                stage,
                mark: RunMark::Started,
            },
        },
        &claude(None),
    )
    .unwrap();
}

fn name(ws: &Workspace, session: &str) -> String {
    ws.session_name(session, None, None)
        .unwrap()
        .text(&Naming::default())
}

#[test]
fn a_run_is_named_by_its_task_and_stage_and_its_agent_when_not_livtks_own() {
    let (_dir, mut ws) = project();
    run(
        &mut ws,
        "LIV-1",
        "s-build",
        Some("livtk:implementer"),
        Stage::Build,
    );
    run(
        &mut ws,
        "LIV-1",
        "s-review",
        Some("livtk:refuter"),
        Stage::Review,
    );
    run(
        &mut ws,
        "LIV-2",
        "s-own",
        Some("project:strict-reviewer"),
        Stage::Review,
    );
    run(&mut ws, "LIV-3", "s-plan", None, Stage::Plan);
    assert_eq!(name(&ws, "s-build"), "LIV-1 · Build");
    assert_eq!(name(&ws, "s-review"), "LIV-1 · Review");
    assert_eq!(name(&ws, "s-own"), "LIV-2 · Review · strict-reviewer");
    assert_eq!(name(&ws, "s-plan"), "LIV-3 · Plan");
    // livtk's own agents are left out of the name, but still found by.
    let review = ws.session_name("s-review", None, None).unwrap();
    assert_eq!(review.agent.as_deref(), Some("refuter"));
    assert!(review.matches(&Naming::default(), "refuter"));
}

#[test]
fn a_chat_session_is_named_by_the_task_it_worked_on_most_and_what_its_moves_show() {
    let (_dir, mut ws) = project();
    ws.set_session(Some("s-chat".into()));
    // It planned LIV-2 and noted on LIV-3.
    ws.execute(
        Command::Edit {
            node: "LIV-2".into(),
            title: None,
            body: Some("The plan.".into()),
        },
        &claude(None),
    )
    .unwrap();
    ws.execute(
        Command::Move {
            node: "LIV-2".into(),
            to: State::Planned,
        },
        &claude(None),
    )
    .unwrap();
    ws.execute(
        Command::Note {
            node: "LIV-3".into(),
            text: "seen".into(),
        },
        &claude(None),
    )
    .unwrap();
    let n = ws.session_name("s-chat", None, Some("plan these")).unwrap();
    assert_eq!(n.text(&Naming::default()), "LIV-2 +1 · Plan");
    assert_eq!(
        (n.task.as_deref(), n.more, n.doing),
        (Some("LIV-2"), 1, Doing::Plan)
    );
    // One that only talked and noted: Chat.
    ws.set_session(Some("s-talk".into()));
    ws.execute(
        Command::Note {
            node: "LIV-1".into(),
            text: "a thought".into(),
        },
        &claude(None),
    )
    .unwrap();
    assert_eq!(name(&ws, "s-talk"), "LIV-1 · Chat");
    // A refuter's notes: a review, its agent livtk's own for it.
    ws.set_session(Some("s-refute".into()));
    ws.execute(
        Command::Note {
            node: "LIV-1".into(),
            text: "review: UPHELD".into(),
        },
        &claude(Some("livtk:refuter")),
    )
    .unwrap();
    assert_eq!(name(&ws, "s-refute"), "LIV-1 · Review");
    // A session of its own whose subagent reviewed: the session's work names it, not the
    // subagent's.
    ws.set_session(Some("s-main".into()));
    ws.execute(
        Command::Move {
            node: "LIV-1".into(),
            to: State::Planned,
        },
        &claude(None),
    )
    .unwrap();
    ws.execute(
        Command::Note {
            node: "LIV-1".into(),
            text: "review: UPHELD".into(),
        },
        &claude(Some("livtk:refuter")),
    )
    .unwrap();
    let main = ws.session_name("s-main", None, None).unwrap();
    assert_eq!((main.doing, main.agent), (Doing::Plan, None));
}

#[test]
fn a_name_the_human_gave_is_kept_and_a_session_that_touched_nothing_keeps_its_prompt() {
    let (_dir, mut ws) = project();
    run(&mut ws, "LIV-1", "s-build", None, Stage::Build);
    let given = ws
        .session_name("s-build", Some("  The importer, day two "), None)
        .unwrap();
    assert_eq!(given.text(&Naming::default()), "The importer, day two");
    assert_eq!(
        given.task.as_deref(),
        Some("LIV-1"),
        "its parts still there to filter by"
    );
    let idle = ws
        .session_name("s-none", None, Some("\n  how do hooks work?\nmore"))
        .unwrap();
    assert_eq!(idle.text(&Naming::default()), "how do hooks work?");
    // Only lessons and decisions: no task, so its prompt names it.
    ws.set_session(Some("s-memory".into()));
    for kind in [Kind::Lesson, Kind::Decision] {
        ws.execute(
            Command::Create {
                kind,
                title: "a rule".into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &claude(None),
        )
        .unwrap();
    }
    assert_eq!(
        ws.session_name("s-memory", None, Some("remember this"))
            .unwrap()
            .text(&Naming::default()),
        "remember this"
    );
    assert_eq!(
        ws.session_name("01a2b3c4-dead", None, None)
            .unwrap()
            .text(&Naming::default()),
        "Session 01a2b3c4"
    );
}

#[test]
fn a_name_is_found_by_its_task_its_stage_and_its_agent_and_reads_as_configured() {
    let (_dir, mut ws) = project();
    run(
        &mut ws,
        "LIV-2",
        "s-own",
        Some("project:strict-reviewer"),
        Stage::Review,
    );
    let n = ws.session_name("s-own", None, None).unwrap();
    let naming = Naming::default();
    for q in ["LIV-2", "liv-2", "review", "strict-reviewer", "", "strict"] {
        assert!(n.matches(&naming, q), "{q}");
    }
    for q in ["LIV-1", "build", "refuter"] {
        assert!(!n.matches(&naming, q), "{q}");
    }
    let plain = Naming {
        separator: " / ".into(),
        agent: false,
    };
    assert_eq!(n.text(&plain), "LIV-2 / Review");
}
