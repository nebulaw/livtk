//! Behaviour of a workspace seen from the outside: commands in, state and log out.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::fs;
use std::path::{Path, PathBuf};

use livtk_core::event::{Event, NodePatch, Op};
use livtk_core::{Actor, Command, Error, Kind, Rel, State, Workspace};
use proptest::prelude::*;

const HUMAN: Actor = Actor::Human;
const CLAUDE: Actor = Actor::Claude { agent: None };

fn init(dir: &Path) -> Workspace {
    Workspace::init(dir, Some("LIV")).unwrap()
}

fn log_path(dir: &Path) -> PathBuf {
    dir.join(".livtk/events.jsonl")
}

fn task(ws: &mut Workspace, title: &str) -> String {
    task_after(ws, title, &[])
}

fn task_after(ws: &mut Workspace, title: &str, deps: &[&str]) -> String {
    let events = ws
        .execute(
            Command::Create {
                kind: Kind::Task,
                title: title.into(),
                body: String::new(),
                priority: None,
                links: deps
                    .iter()
                    .map(|d| (Rel::DependsOn, (*d).to_owned()))
                    .collect(),
            },
            &CLAUDE,
        )
        .unwrap();
    match &events[0].op {
        Op::NodeCreate { data, .. } => data.key.clone(),
        other => panic!("{other:?}"),
    }
}

fn mv(ws: &mut Workspace, key: &str, to: State, by: &Actor) -> Result<Vec<Event>, Error> {
    ws.execute(
        Command::Move {
            node: key.into(),
            to,
        },
        by,
    )
}

/// Walk a task from backlog to acked, the human acknowledging.
fn ack(ws: &mut Workspace, key: &str) {
    mv(ws, key, State::Planned, &CLAUDE).unwrap();
    mv(ws, key, State::Acked, &HUMAN).unwrap();
}

fn state(ws: &Workspace, key: &str) -> State {
    ws.node(key).unwrap().state
}

// ---- the human gate ---------------------------------------------------------------------------

/// Add `[gates]` to the project's config and open it again.
fn with_gates(dir: &Path, entries: &str) -> Result<Workspace, Error> {
    let path = dir.join(".livtk/config.toml");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str(&format!("\n[gates]\nhuman = [{entries}]\n"));
    fs::write(&path, text).unwrap();
    Workspace::open(dir)
}

#[test]
fn a_project_can_keep_more_moves_for_the_human() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let (a, b) = (task(&mut ws, "a"), task(&mut ws, "b"));
    for k in [&a, &b] {
        ack(&mut ws, k);
        mv(&mut ws, k, State::Doing, &CLAUDE).unwrap();
        mv(&mut ws, k, State::Review, &CLAUDE).unwrap();
    }
    let c = task(&mut ws, "c");
    drop(ws);
    let mut ws = with_gates(dir.path(), r#""task: review -> done", "task: -> canceled""#).unwrap();
    assert!(ws.human_only(Kind::Task, State::Review, State::Done));
    assert!(
        ws.human_only(Kind::Task, State::Backlog, State::Canceled),
        "from any state"
    );
    assert!(!ws.human_only(Kind::Task, State::Doing, State::Review));

    let refused = mv(&mut ws, &a, State::Done, &CLAUDE).unwrap_err();
    assert!(
        matches!(&refused, Error::Refused(m) if m.contains("[gates]") && m.contains("livtk move LIV-1 done")),
        "{refused}"
    );
    assert!(matches!(
        mv(&mut ws, &c, State::Canceled, &CLAUDE),
        Err(Error::Refused(_))
    ));
    mv(&mut ws, &a, State::Done, &HUMAN).unwrap();
    mv(&mut ws, &b, State::Doing, &CLAUDE).unwrap();
    mv(&mut ws, &b, State::Review, &CLAUDE).unwrap();

    let inbox: Vec<String> = ws.inbox().unwrap().into_iter().map(|s| s.key).collect();
    assert_eq!(
        inbox,
        [b],
        "what waits on a gate from a state is in the inbox; a gate from anywhere adds nothing"
    );
    assert!(
        mv(&mut ws, &c, State::Planned, &CLAUDE).is_ok(),
        "other moves stay Claude's"
    );
    assert!(
        matches!(
            mv(&mut ws, &c, State::Acked, &CLAUDE),
            Err(Error::Refused(_))
        ),
        "and the built-in gates stay"
    );
}

#[test]
fn a_gate_that_names_no_legal_move_is_a_config_error() {
    for (entry, says) in [
        (r#""task review done""#, "write it as"),
        (r#""story: review -> done""#, "no kind `story`"),
        (
            r#""task: review -> ratified""#,
            "a task has no state `ratified`",
        ),
        (
            r#""task: backlog -> done""#,
            "cannot move from backlog to done",
        ),
        (
            r#""finding: -> recorded""#,
            "no finding ever moves to recorded",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path());
        let err = with_gates(dir.path(), entry).err().unwrap();
        assert!(
            matches!(&err, Error::Config { .. }) && err.to_string().contains(says),
            "{entry}: {err}"
        );
    }
}

#[test]
fn only_a_human_acknowledges() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let t = task(&mut ws, "write the plan");
    mv(&mut ws, &t, State::Planned, &CLAUDE).unwrap();

    let err = mv(&mut ws, &t, State::Acked, &CLAUDE).unwrap_err();
    assert!(matches!(err, Error::Refused(_)), "{err}");
    assert!(err.to_string().contains("only a human"), "{err}");
    assert_eq!(state(&ws, &t), State::Planned);

    let sub = Actor::Claude {
        agent: Some("refuter".into()),
    };
    assert!(mv(&mut ws, &t, State::Acked, &sub).is_err());

    mv(&mut ws, &t, State::Acked, &HUMAN).unwrap();
    assert_eq!(state(&ws, &t), State::Acked);
}

#[test]
fn claude_may_do_everything_else_including_done() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let t = task(&mut ws, "ship it");
    ack(&mut ws, &t);
    for to in [
        State::Doing,
        State::Review,
        State::Done,
        State::Doing,
        State::Parked,
    ] {
        mv(&mut ws, &t, to, &CLAUDE).unwrap();
    }
}

#[test]
fn a_changed_plan_needs_a_new_acknowledgement() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let t = task(&mut ws, "plan");
    ack(&mut ws, &t);

    let same = ws
        .execute(
            Command::Edit {
                node: t.clone(),
                title: Some("  plan ".into()),
                body: None,
            },
            &CLAUDE,
        )
        .unwrap();
    assert!(
        same.is_empty(),
        "an edit that changes nothing writes nothing"
    );
    assert_eq!(state(&ws, &t), State::Acked);

    ws.execute(
        Command::Edit {
            node: t.clone(),
            title: None,
            body: Some("now with more steps".into()),
        },
        &CLAUDE,
    )
    .unwrap();
    assert_eq!(state(&ws, &t), State::Planned);
}

#[test]
fn a_note_records_history_without_touching_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let t = task(&mut ws, "plan");
    ack(&mut ws, &t);
    let before = ws.node(&t).unwrap();
    let note = |text: &str| Command::Note {
        node: t.clone(),
        text: text.into(),
    };
    let events = ws
        .execute(note("  tried winit first; it panics on resize  "), &CLAUDE)
        .unwrap();
    assert_eq!(events[0].v, 2, "notes need format 2");
    let after = ws.node(&t).unwrap();
    assert_eq!(after.state, State::Acked, "a note is not a change of plan");
    assert_eq!((after.title, after.body), (before.title, before.body));
    match &ws.history(Some(&t)).unwrap().last().unwrap().op {
        Op::NodeNote { data, .. } => {
            assert_eq!(data.text, "tried winit first; it panics on resize")
        }
        other => panic!("{other:?}"),
    }
    assert!(
        ws.execute(note("   "), &CLAUDE).is_err(),
        "an empty note is refused"
    );
    let live = ws.snapshot().unwrap();
    ws.rebuild().unwrap();
    assert_eq!(ws.snapshot().unwrap(), live);
}

#[test]
fn illegal_moves_say_where_a_node_can_go() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let t = task(&mut ws, "x");
    let err = mv(&mut ws, &t, State::Done, &HUMAN).unwrap_err();
    assert_eq!(
        err.to_string(),
        "LIV-1 is backlog; from there it can move to: planned, parked, canceled"
    );
    let err = mv(&mut ws, &t, State::Backlog, &HUMAN).unwrap_err();
    assert_eq!(err.to_string(), "LIV-1 is already backlog");
}

// ---- dependencies -----------------------------------------------------------------------------

#[test]
fn work_cannot_start_before_its_dependencies_close() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let a = task(&mut ws, "a");
    let b = task(&mut ws, "b");
    let c = task_after(&mut ws, "c", &[&a, &b]);
    ack(&mut ws, &c);

    let err = mv(&mut ws, &c, State::Doing, &CLAUDE).unwrap_err();
    assert_eq!(
        err.to_string(),
        "LIV-3 waits on LIV-1 (backlog), LIV-2 (backlog); finish or cancel those first"
    );
    assert!(ws.node(&c).unwrap().blocked);
    assert_eq!(ws.next().unwrap(), None);

    ack(&mut ws, &a);
    for to in [State::Doing, State::Review, State::Done] {
        mv(&mut ws, &a, to, &CLAUDE).unwrap();
    }
    mv(&mut ws, &b, State::Canceled, &CLAUDE).unwrap();

    assert!(!ws.node(&c).unwrap().blocked);
    assert_eq!(ws.next().unwrap().map(|n| n.key), Some(c.clone()));
    mv(&mut ws, &c, State::Doing, &CLAUDE).unwrap();
}

#[test]
fn cycles_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let a = task(&mut ws, "a");
    let b = task_after(&mut ws, "b", &[&a]);
    let c = task_after(&mut ws, "c", &[&b]);
    let link = |src: &str, dst: &str| Command::Link {
        src: src.into(),
        rel: Rel::DependsOn,
        dst: dst.into(),
    };
    let err = ws.execute(link(&a, &c), &CLAUDE).unwrap_err();
    assert!(matches!(err, Error::Refused(_)), "{err}");
    assert!(ws.execute(link(&a, &a), &CLAUDE).is_err());
    assert!(
        ws.execute(link(&c, &b), &CLAUDE).unwrap().is_empty(),
        "an existing link is a no-op"
    );
    let links = ws.links(&b).unwrap();
    assert_eq!(links.depends_on.len(), 1);
    assert_eq!(links.blocks.len(), 1);
}

#[test]
fn next_prefers_priority_then_what_unblocks_most() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let low = task(&mut ws, "low");
    let hub = task(&mut ws, "hub");
    let urgent = task(&mut ws, "urgent");
    task_after(&mut ws, "after hub 1", &[&hub]);
    task_after(&mut ws, "after hub 2", &[&hub]);
    for t in [&low, &hub, &urgent] {
        ack(&mut ws, t);
    }
    ws.execute(
        Command::SetPriority {
            node: low.clone(),
            priority: 4,
        },
        &CLAUDE,
    )
    .unwrap();
    ws.execute(
        Command::SetPriority {
            node: urgent.clone(),
            priority: 1,
        },
        &CLAUDE,
    )
    .unwrap();
    assert_eq!(ws.next().unwrap().map(|n| n.key), Some(urgent.clone()));
    mv(&mut ws, &urgent, State::Doing, &CLAUDE).unwrap();
    assert_eq!(ws.next().unwrap().map(|n| n.key), Some(hub));
}

#[test]
fn references_accept_keys_numbers_and_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let t = task(&mut ws, "x");
    let id = ws.node(&t).unwrap().id;
    for r in ["LIV-1", "liv-1", "1", " 1 ", &id.to_string()] {
        assert_eq!(ws.node(r).unwrap().id, id, "{r}");
    }
    assert!(matches!(ws.node("LIV-2"), Err(Error::UnknownNode(_))));
}

// ---- durability -------------------------------------------------------------------------------

#[test]
fn reopening_shows_the_same_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let t = task(&mut ws, "persist me");
    ack(&mut ws, &t);
    let before = ws.snapshot().unwrap();
    drop(ws);
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.snapshot().unwrap(), before);
}

#[test]
fn a_reader_sees_changes_another_writer_already_applied() {
    // The app and the CLI share one store: the CLI applies its own writes, so the app's refresh
    // must still report them.
    let dir = tempfile::tempdir().unwrap();
    let mut writer = init(dir.path());
    let mut reader = Workspace::open(dir.path()).unwrap();
    assert!(!reader.refresh().unwrap(), "nothing new yet");
    task(&mut writer, "from elsewhere");
    assert!(
        reader.refresh().unwrap(),
        "the writer's change is news to the reader"
    );
    assert!(!reader.refresh().unwrap(), "and only once");
    assert_eq!(reader.tasks(&[]).unwrap().len(), 1);
    task(&mut reader, "its own");
    assert!(!reader.refresh().unwrap(), "its own writes count as seen");
    assert!(writer.refresh().unwrap(), "but are news to the other");
}

#[test]
fn rebuild_matches_incremental_apply() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let a = task(&mut ws, "a");
    let b = task_after(&mut ws, "b", &[&a]);
    ack(&mut ws, &a);
    ws.execute(
        Command::Unlink {
            src: b.clone(),
            rel: Rel::DependsOn,
            dst: a.clone(),
        },
        &CLAUDE,
    )
    .unwrap();
    let incremental = ws.snapshot().unwrap();
    ws.rebuild().unwrap();
    assert_eq!(ws.snapshot().unwrap(), incremental);
    fs::remove_file(dir.path().join(".livtk/livtk.db")).unwrap();
    let fresh = Workspace::open(dir.path()).unwrap();
    assert_eq!(fresh.snapshot().unwrap(), incremental);
}

#[test]
fn an_event_logged_but_never_applied_is_picked_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let t = task(&mut ws, "crash test");
    let id = ws.node(&t).unwrap().id;
    let last = ws.history(None).unwrap().last().map(|e| e.id);
    // The writer crashed after the log append and before the store update.
    let e = Event::new(
        last,
        &CLAUDE,
        Op::NodeSet {
            node: id,
            data: NodePatch {
                title: Some("survived".into()),
                ..NodePatch::default()
            },
        },
    );
    let mut log = fs::read_to_string(log_path(dir.path())).unwrap();
    log.push_str(&e.to_line().unwrap());
    log.push('\n');
    fs::write(log_path(dir.path()), log).unwrap();

    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.node(&t).unwrap().title, "survived");
}

#[test]
fn an_unfinished_last_line_is_dropped_on_the_next_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    task(&mut ws, "a");
    let mut log = fs::read(log_path(dir.path())).unwrap();
    log.extend_from_slice(br#"{"v":1,"id":"01a1"#);
    fs::write(log_path(dir.path()), &log).unwrap();

    let mut ws = Workspace::open(dir.path()).unwrap();
    task(&mut ws, "b");
    let warnings = ws.take_warnings();
    assert!(
        warnings.iter().any(|w| w.contains("unfinished last line")),
        "{warnings:?}"
    );
    assert_eq!(ws.tasks(&[]).unwrap().len(), 2);
    let text = fs::read_to_string(log_path(dir.path())).unwrap();
    assert!(
        text.lines()
            .all(|l| l.starts_with(r#"{"v":1"#) && l.ends_with('}'))
    );
}

#[test]
fn going_back_in_git_history_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    task(&mut ws, "a");
    let checkpoint = fs::read(log_path(dir.path())).unwrap();
    task(&mut ws, "b");
    assert_eq!(ws.tasks(&[]).unwrap().len(), 2);

    fs::write(log_path(dir.path()), &checkpoint).unwrap();
    assert!(ws.refresh().unwrap());
    let keys: Vec<String> = ws.tasks(&[]).unwrap().into_iter().map(|n| n.key).collect();
    assert_eq!(keys, ["LIV-1"]);
    assert!(!ws.refresh().unwrap(), "nothing changed since");
}

#[test]
fn stale_says_what_refresh_would_catch_up_without_doing_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    task(&mut ws, "a");
    let checkpoint = fs::read(log_path(dir.path())).unwrap();
    task(&mut ws, "b");
    let full = fs::read(log_path(dir.path())).unwrap();
    assert!(!ws.stale().unwrap(), "its own writes are applied");
    let mut other = Workspace::open(dir.path()).unwrap();
    task(&mut other, "c");
    assert!(!ws.stale().unwrap(), "so are another process's");

    fs::write(log_path(dir.path()), &checkpoint).unwrap();
    assert!(ws.stale().unwrap(), "a rewritten log is");
    assert!(ws.stale().unwrap(), "and stays so: nothing was applied");
    assert_eq!(ws.tasks(&[]).unwrap().len(), 3);
    ws.refresh().unwrap();
    assert!(!ws.stale().unwrap(), "until refresh catches up");

    fs::write(log_path(dir.path()), &full).unwrap();
    assert!(ws.stale().unwrap(), "lines appended by git are too");
    ws.refresh().unwrap();
    assert!(!ws.stale().unwrap());
    assert_eq!(ws.tasks(&[]).unwrap().len(), 2);
}

#[test]
fn lines_from_newer_versions_and_noise_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    task(&mut ws, "a");
    let mut log = fs::read_to_string(log_path(dir.path())).unwrap();
    log.push_str("<<<<<<< HEAD\n");
    log.push_str(r#"{"v":9,"id":"01a14146-c798-773d-a550-39d72a612378","op":"node.teleport"}"#);
    log.push('\n');
    fs::write(log_path(dir.path()), log).unwrap();

    let mut ws = Workspace::open(dir.path()).unwrap();
    let warnings = ws.take_warnings();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings.iter().any(|w| w.contains("newer livtk")),
        "{warnings:?}"
    );
    task(&mut ws, "b");
    assert_eq!(ws.tasks(&[]).unwrap().len(), 2);
}

#[test]
fn an_outdated_store_schema_is_rebuilt() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    task(&mut ws, "a");
    let before = ws.snapshot().unwrap();
    drop(ws);
    let db = rusqlite::Connection::open(dir.path().join(".livtk/livtk.db")).unwrap();
    db.pragma_update(None, "user_version", 0).unwrap();
    db.execute_batch("DROP TABLE nodes;").unwrap();
    drop(db);
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.snapshot().unwrap(), before);
}

// ---- merges and replay ------------------------------------------------------------------------

/// A second checkout of the same project: the committed files, no database.
fn clone_project(from: &Path) -> tempfile::TempDir {
    let to = tempfile::tempdir().unwrap();
    fs::create_dir(to.path().join(".livtk")).unwrap();
    for f in [
        "config.toml",
        "events.jsonl",
        ".gitignore",
        ".gitattributes",
    ] {
        fs::copy(
            from.join(".livtk").join(f),
            to.path().join(".livtk").join(f),
        )
        .unwrap();
    }
    to
}

/// What `git merge` with `merge=union` produces: the shared prefix, then both sides' new lines.
fn union(base: &str, ours: &str, theirs: &str) -> String {
    let mut out = base.to_owned();
    out.push_str(&ours[base.len()..]);
    out.push_str(&theirs[base.len()..]);
    out
}

#[test]
fn branches_that_both_create_the_same_key_merge_cleanly() {
    let base_dir = tempfile::tempdir().unwrap();
    let mut base = init(base_dir.path());
    let shared = task(&mut base, "shared");
    let base_log = fs::read_to_string(log_path(base_dir.path())).unwrap();

    let ours_dir = clone_project(base_dir.path());
    let theirs_dir = clone_project(base_dir.path());
    let mut ours = Workspace::open(ours_dir.path()).unwrap();
    let ours_key = task(&mut ours, "ours");
    mv(&mut ours, &shared, State::Planned, &CLAUDE).unwrap();
    let mut theirs = Workspace::open(theirs_dir.path()).unwrap();
    let theirs_key = task(&mut theirs, "theirs");
    mv(&mut theirs, &shared, State::Parked, &CLAUDE).unwrap();
    assert_eq!(ours_key, "LIV-2");
    assert_eq!(theirs_key, "LIV-2");

    let ours_log = fs::read_to_string(log_path(ours_dir.path())).unwrap();
    let theirs_log = fs::read_to_string(log_path(theirs_dir.path())).unwrap();
    let merged_a = union(&base_log, &ours_log, &theirs_log);
    let merged_b = union(&base_log, &theirs_log, &ours_log);

    let mut snapshots = Vec::new();
    for merged in [&merged_a, &merged_b] {
        fs::write(log_path(ours_dir.path()), merged).unwrap();
        ours.refresh().unwrap();
        snapshots.push(ours.snapshot().unwrap());
    }
    assert_eq!(snapshots[0], snapshots[1], "merge order does not matter");

    let titles: Vec<(String, String)> = ours
        .tasks(&[])
        .unwrap()
        .into_iter()
        .map(|n| (n.key, n.title))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(
        titles,
        [
            ("LIV-1".to_owned(), "shared".to_owned()),
            ("LIV-2".to_owned(), "ours".to_owned()),
            ("LIV-3".to_owned(), "theirs".to_owned()),
        ]
    );
    assert_eq!(
        state(&ours, &shared),
        State::Parked,
        "the later change wins"
    );
    assert_eq!(ours.node("LIV-2").unwrap().title, "ours");
    match &ours.history(Some("LIV-3")).unwrap()[0].op {
        Op::NodeCreate { data, .. } => {
            assert_eq!(data.key, "LIV-2", "the first key stays in the log")
        }
        other => panic!("{other:?}"),
    }

    // Life goes on after the merge, and a rebuild agrees with it.
    let next = task(&mut ours, "after the merge");
    assert_eq!(next, "LIV-4");
    let live = ours.snapshot().unwrap();
    ours.rebuild().unwrap();
    assert_eq!(ours.snapshot().unwrap(), live);
}

#[test]
fn older_events_appended_by_a_merge_still_replay_in_time_order() {
    let base_dir = tempfile::tempdir().unwrap();
    let mut base = init(base_dir.path());
    task(&mut base, "shared");
    let base_log = fs::read_to_string(log_path(base_dir.path())).unwrap();

    let theirs_dir = clone_project(base_dir.path());
    let ours_dir = clone_project(base_dir.path());
    // Theirs works first; ours works later and has its own work applied.
    let mut theirs = Workspace::open(theirs_dir.path()).unwrap();
    task(&mut theirs, "theirs, earlier");
    let mut ours = Workspace::open(ours_dir.path()).unwrap();
    task(&mut ours, "ours, later");

    // Pulling theirs appends lines older than everything ours has applied.
    let ours_log = fs::read_to_string(log_path(ours_dir.path())).unwrap();
    let theirs_log = fs::read_to_string(log_path(theirs_dir.path())).unwrap();
    fs::write(
        log_path(ours_dir.path()),
        union(&base_log, &ours_log, &theirs_log),
    )
    .unwrap();
    assert!(ours.refresh().unwrap());

    let fresh = clone_project(ours_dir.path());
    let replayed = Workspace::open(fresh.path()).unwrap();
    assert_eq!(ours.snapshot().unwrap(), replayed.snapshot().unwrap());
    assert_eq!(
        ours.node("LIV-2").unwrap().title,
        "theirs, earlier",
        "the earlier create keeps the key"
    );
    assert_eq!(ours.node("LIV-3").unwrap().title, "ours, later");
}

#[test]
fn eight_writers_at_once_lose_nothing() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    let threads: Vec<_> = (0..8)
        .map(|w| {
            let path = dir.path().to_path_buf();
            std::thread::spawn(move || {
                let mut ws = Workspace::open(&path).unwrap();
                for i in 0..25 {
                    task(&mut ws, &format!("writer {w} task {i}"));
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let ws = Workspace::open(dir.path()).unwrap();
    let tasks = ws.tasks(&[]).unwrap();
    assert_eq!(tasks.len(), 200);
    let mut keys: Vec<String> = tasks.iter().map(|n| n.key.clone()).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), 200, "every key is unique");
    let expected: std::collections::BTreeSet<String> =
        (1..=200).map(|n| format!("LIV-{n}")).collect();
    assert_eq!(
        keys.into_iter().collect::<std::collections::BTreeSet<_>>(),
        expected,
        "no gaps, no renumbering"
    );
    assert_eq!(
        fs::read_to_string(log_path(dir.path()))
            .unwrap()
            .lines()
            .count(),
        200
    );
}

// ---- property: replay is order independent ----------------------------------------------------

/// Drive a workspace with arbitrary but legal commands chosen from `choices`.
fn drive(ws: &mut Workspace, choices: &[(u8, u8, u8)]) {
    for &(what, a, b) in choices {
        let tasks = ws.tasks(&[]).unwrap();
        let pick = |n: u8| {
            tasks
                .get(usize::from(n) % tasks.len().max(1))
                .map(|t| t.key.clone())
        };
        let result = match (what % 5, pick(a), pick(b)) {
            (0, _, _) | (_, None, _) => {
                let deps: Vec<&str> = Vec::new();
                ws.execute(
                    Command::Create {
                        kind: Kind::Task,
                        title: format!("task {a} {b}"),
                        body: String::new(),
                        priority: Some(b % 4 + 1),
                        links: deps
                            .iter()
                            .map(|d| (Rel::DependsOn, (*d).to_owned()))
                            .collect(),
                    },
                    &CLAUDE,
                )
            }
            (1, Some(k), _) => {
                let n = ws.node(&k).unwrap();
                let options = n.kind.next_states(n.state);
                let to = options[usize::from(b) % options.len()];
                mv(ws, &k, to, &HUMAN)
            }
            (2, Some(k), _) => ws.execute(
                Command::Edit {
                    node: k,
                    title: Some(format!("renamed {b}")),
                    body: None,
                },
                &CLAUDE,
            ),
            (3, Some(k), Some(j)) => ws.execute(
                Command::Link {
                    src: k,
                    rel: Rel::DependsOn,
                    dst: j,
                },
                &CLAUDE,
            ),
            (_, Some(k), Some(j)) => ws.execute(
                Command::Unlink {
                    src: k,
                    rel: Rel::DependsOn,
                    dst: j,
                },
                &CLAUDE,
            ),
            (_, Some(_), None) => unreachable!("pick(b) finds a task whenever pick(a) does"),
        };
        // Refusals (a cycle, a blocked start, a missing link) are part of the game.
        match result {
            Ok(_) | Err(Error::Refused(_) | Error::Invalid(_)) => {}
            Err(e) => panic!("{e}"),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    #[test]
    fn replay_ignores_line_order(
        choices in prop::collection::vec(any::<(u8, u8, u8)>(), 1..40),
        order in prop::collection::vec(any::<u32>(), 40..41),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = init(dir.path());
        drive(&mut ws, &choices);
        let expected = ws.snapshot().unwrap();

        let log = fs::read_to_string(log_path(dir.path())).unwrap();
        let mut lines: Vec<&str> = log.lines().collect();
        let mut keyed: Vec<(u32, &str)> = lines
            .drain(..)
            .enumerate()
            .map(|(i, l)| (order[i % order.len()].wrapping_mul(i as u32 + 1), l))
            .collect();
        keyed.sort();
        let shuffled: String = keyed.into_iter().map(|(_, l)| format!("{l}\n")).collect();

        let other = clone_project(dir.path());
        fs::write(log_path(other.path()), shuffled).unwrap();
        let replayed = Workspace::open(other.path()).unwrap();
        prop_assert_eq!(replayed.snapshot().unwrap(), expected);
    }
}

#[test]
fn a_plan_edited_under_way_or_in_review_goes_back_to_planned() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    for to in [State::Doing, State::Review] {
        let t = task(&mut ws, "import the board");
        ack(&mut ws, &t);
        mv(&mut ws, &t, State::Doing, &CLAUDE).unwrap();
        if to == State::Review {
            mv(&mut ws, &t, State::Review, &CLAUDE).unwrap();
        }
        ws.execute(
            Command::Edit {
                node: t.clone(),
                title: None,
                body: Some("Import 139 tasks, not 133.".into()),
            },
            &CLAUDE,
        )
        .unwrap();
        assert_eq!(state(&ws, &t), State::Planned, "from {to}");
        assert!(
            ws.inbox().unwrap().iter().any(|s| s.key == t),
            "waits for the human"
        );
    }
}

// ---- settings written through the core -----------------------------------------------------------

#[test]
fn settings_are_written_checked_and_keep_the_files_comments() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let path = dir.path().join(".livtk/config.toml");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("# the team's own note\n");
    std::fs::write(&path, text).unwrap();
    ws.set_hook("session-start", false).unwrap();
    ws.set_section("session_start", "protocol", false).unwrap();
    ws.set_gates(&["task: review -> done".into()]).unwrap();
    ws.set_docs_dir(Some("handbook")).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("# the team's own note"), "{written}");
    let back = Workspace::open(dir.path()).unwrap();
    let c = back.config();
    assert!(!c.hook_on("session-start"));
    assert!(!c.session_start.has("protocol") && c.session_start.has("tasks"));
    assert_eq!(c.gates.human, ["task: review -> done"]);
    assert_eq!(c.docs.as_deref(), Some("handbook"));
    // A bad gate is refused, and the file left as it was.
    let before = std::fs::read_to_string(&path).unwrap();
    assert!(ws.set_gates(&["task: nowhere -> done".into()]).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    // Everything on again: the tables go, the defaults are back.
    ws.set_hook("session-start", true).unwrap();
    ws.set_section("session_start", "protocol", true).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        !written.contains("disable") && !written.contains("session_start"),
        "{written}"
    );
}

// ---- declared kinds, one process ----------------------------------------------------------------

fn declare(dir: &Path, toml: &str) {
    let path = dir.join(".livtk/config.toml");
    let mut config = fs::read_to_string(&path).unwrap();
    config.push('\n');
    config.push_str(toml);
    fs::write(path, config).unwrap();
}

#[test]
fn two_projects_in_one_process_keep_their_own_kinds() {
    // Names only this test uses: the test binary shares one process.
    let tempdir = || tempfile::tempdir().unwrap();
    let (a, b, c) = (tempdir(), tempdir(), tempdir());
    init(a.path());
    Workspace::init(b.path(), Some("OPS")).unwrap();
    init(c.path());
    declare(
        a.path(),
        "[[kinds]]\nname = \"outage\"\nstates = [\"open\", \"closed\"]\nkey = \"OUT\"\n",
    );
    let mut wa = Workspace::open(a.path()).unwrap();
    // Another project declaring the same name differently is refused, naming the first.
    declare(
        b.path(),
        "[[kinds]]\nname = \"outage\"\nstates = [\"new\", \"triaged\", \"resolved\"]\nkey = \"OPX\"\n",
    );
    let err = Workspace::open(b.path())
        .err()
        .expect("refused")
        .to_string();
    assert!(
        err.contains("outage") && err.contains(&a.path().display().to_string()),
        "{err}"
    );
    // The first project's nodes keep its own key and states.
    let outage = Kind::named("outage").unwrap();
    let events = wa
        .execute(
            Command::Create {
                kind: outage,
                title: "checkout is down".into(),
                body: String::new(),
                priority: None,
                links: Vec::new(),
            },
            &CLAUDE,
        )
        .unwrap();
    match &events[0].op {
        Op::NodeCreate { data, .. } => {
            assert_eq!(data.key, "OUT-1");
            assert_eq!(data.state.as_str(), "open");
        }
        other => panic!("{other:?}"),
    }
    // A project that declares nothing does not take on another's kinds.
    let mut wc = Workspace::open(c.path()).unwrap();
    assert!(!wc.owns(outage));
    let refused = wc.execute(
        Command::Create {
            kind: outage,
            title: "x".into(),
            body: String::new(),
            priority: None,
            links: Vec::new(),
        },
        &CLAUDE,
    );
    assert!(refused.is_err(), "not its kind");
}

// ---- projects -----------------------------------------------------------------------------------

fn create(ws: &mut Workspace, kind: Kind, title: &str, links: &[(Rel, &str)]) -> String {
    let events = ws
        .execute(
            Command::Create {
                kind,
                title: title.into(),
                body: String::new(),
                priority: None,
                links: links.iter().map(|(r, k)| (*r, (*k).to_owned())).collect(),
            },
            &CLAUDE,
        )
        .unwrap();
    match &events[0].op {
        Op::NodeCreate { data, .. } => data.key.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_project_is_the_humans_to_commit_to_and_counts_its_tasks_through_nesting() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let outer = create(&mut ws, Kind::Project, "ship the importer", &[]);
    assert_eq!(outer, "PRJ-1");
    let inner = create(
        &mut ws,
        Kind::Project,
        "parse the board",
        &[(Rel::Parent, &outer)],
    );
    // Claude proposes; only the human commits and calls it done.
    let refused = mv(&mut ws, &outer, State::Active, &CLAUDE);
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    mv(&mut ws, &outer, State::Active, &HUMAN).unwrap();
    // Two tasks in the outer, three in the inner (one canceled, one with a sub-task).
    let a = create(&mut ws, Kind::Task, "a", &[(Rel::Parent, &outer)]);
    let b = create(&mut ws, Kind::Task, "b", &[(Rel::Parent, &outer)]);
    let c = create(&mut ws, Kind::Task, "c", &[(Rel::Parent, &inner)]);
    let d = create(&mut ws, Kind::Task, "d", &[(Rel::Parent, &inner)]);
    let e = create(&mut ws, Kind::Task, "e", &[(Rel::Parent, &c)]);
    // Unrelated work does not count.
    task(&mut ws, "elsewhere");
    for k in [&a, &b, &c] {
        ack(&mut ws, k);
    }
    for k in [&a, &b] {
        mv(&mut ws, k, State::Doing, &CLAUDE).unwrap();
        mv(&mut ws, k, State::Review, &CLAUDE).unwrap();
    }
    mv(&mut ws, &a, State::Done, &HUMAN).unwrap();
    mv(&mut ws, &d, State::Canceled, &CLAUDE).unwrap();
    mv(&mut ws, &e, State::Planned, &CLAUDE).unwrap();
    let now = jiff::Timestamp::now();
    let g = ws.progress(&outer, now).unwrap();
    // a, b, c, e: d is canceled. a is done.
    assert_eq!((g.done, g.total), (1, 4));
    assert_eq!(g.projects, vec![inner.clone()]);
    // e waits for its ack; b, in review, for the human's check.
    assert_eq!(g.waiting.len(), 2, "{:?}", g.waiting);
    assert_eq!(g.waiting[0].state, State::Planned);
    assert_eq!(
        (g.waiting[0].count, g.waiting[0].oldest.as_str()),
        (1, e.as_str())
    );
    assert_eq!(g.waiting[1].state, State::Review);
    assert_eq!(
        (g.waiting[1].count, g.waiting[1].oldest.as_str()),
        (1, b.as_str())
    );
    // Its own count, the inner project's.
    let gi = ws.progress(&inner, now).unwrap();
    assert_eq!((gi.done, gi.total), (0, 2));
    // One project each: a task part of one cannot join another.
    let other = create(&mut ws, Kind::Project, "another", &[]);
    let r = ws.execute(
        Command::Link {
            src: a.clone(),
            rel: Rel::Parent,
            dst: other.clone(),
        },
        &CLAUDE,
    );
    assert!(matches!(r, Err(Error::Refused(_))), "{r:?}");
    // Nor through a sub-task: e is part of c, which is in the inner project.
    let r = ws.execute(
        Command::Link {
            src: e.clone(),
            rel: Rel::Parent,
            dst: other.clone(),
        },
        &CLAUDE,
    );
    assert!(matches!(r, Err(Error::Refused(_))), "{r:?}");
    assert_eq!(ws.progress(&other, now).unwrap().total, 0);
    // Done is the human's too.
    assert!(mv(&mut ws, &outer, State::Done, &CLAUDE).is_err());
    mv(&mut ws, &outer, State::Done, &HUMAN).unwrap();
}

#[test]
fn an_active_project_without_an_update_for_a_week_reads_update_missing() {
    use livtk_core::Health;
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let p = create(&mut ws, Kind::Project, "an outcome", &[]);
    // Proposed: nothing is missing yet.
    let far = jiff::Timestamp::now() + jiff::SignedDuration::from_hours(24 * 30);
    assert!(!ws.progress(&p, far).unwrap().update_missing);
    let at: jiff::Timestamp = mv(&mut ws, &p, State::Active, &HUMAN).unwrap()[0]
        .at
        .parse()
        .unwrap();
    let day = |n: i64| at + jiff::SignedDuration::from_hours(24 * n);
    assert!(!ws.progress(&p, day(6)).unwrap().update_missing);
    assert!(ws.progress(&p, day(8)).unwrap().update_missing);
    let events = ws
        .execute(
            Command::Update {
                node: p.clone(),
                health: Health::AtRisk,
                text: "slower than hoped; next: batch it".into(),
            },
            &CLAUDE,
        )
        .unwrap();
    assert_eq!(events[0].v, 6, "an update is new in format 6");
    let g = ws.progress(&p, day(3)).unwrap();
    assert!(!g.update_missing);
    let u = g.update.unwrap();
    assert_eq!(u.health, Health::AtRisk);
    assert_eq!(u.text, "slower than hoped; next: batch it");
    // An update is a project's; Claude never owns one.
    let t = task(&mut ws, "a task");
    assert!(
        ws.execute(
            Command::Update {
                node: t,
                health: Health::OnTrack,
                text: "x".into()
            },
            &CLAUDE
        )
        .is_err()
    );
    let owner = ws.execute(
        Command::SetProject {
            node: p.clone(),
            target: Some("2026-12-01".into()),
            owner: Some("claude".into()),
        },
        &CLAUDE,
    );
    assert!(matches!(owner, Err(Error::Refused(_))), "{owner:?}");
    ws.execute(
        Command::SetProject {
            node: p.clone(),
            target: Some("2026-12-01".into()),
            owner: Some("karl".into()),
        },
        &CLAUDE,
    )
    .unwrap();
    let g = ws.progress(&p, day(3)).unwrap();
    assert_eq!(g.target.as_deref(), Some("2026-12-01"));
    assert_eq!(g.owner.as_deref(), Some("karl"));
    let bad = ws.execute(
        Command::SetProject {
            node: p.clone(),
            target: Some("next week".into()),
            owner: None,
        },
        &CLAUDE,
    );
    assert!(bad.is_err());
}

// ---- flow -----------------------------------------------------------------------------------------

#[test]
fn flow_numbers_match_a_hand_built_log() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    // Hour by hour, from 2026-01-01T00:00:00Z: ids in time order.
    let at = |h: u32| format!("2026-01-01T{h:02}:00:00.000Z");
    let id = |n: u32| format!("01900000-0000-7000-8000-{n:012x}");
    let node = |n: u32| format!("01800000-0000-7000-8000-{n:012x}");
    let mut lines = Vec::new();
    let mut seq = 0;
    let mut ev = |h: u32, by: &str, op: &str, n: u32, data: String| {
        seq += 1;
        lines.push(format!(
            r#"{{"v":1,"id":"{}","at":"{}","by":"{by}","op":"{op}","node":"{}","data":{data}}}"#,
            id(seq),
            at(h),
            node(n)
        ));
    };
    let create = |kind: &str, key: &str, state: &str| {
        format!(
            r#"{{"kind":"{kind}","key":"{key}","title":"{key}","state":"{state}","priority":3}}"#
        )
    };
    let set = |s: &str| format!(r#"{{"state":"{s}"}}"#);
    // LIV-1: backlog 0-1, planned 1-3, acked 3-4, doing 4-7, review 7-8, done at 8.
    ev(
        0,
        "claude",
        "node.create",
        1,
        create("task", "LIV-1", "backlog"),
    );
    // LIV-2: backlog 0-2, planned 2-? (still planned at 10: 8 hours waiting).
    ev(
        0,
        "claude",
        "node.create",
        2,
        create("task", "LIV-2", "backlog"),
    );
    ev(1, "claude", "node.set", 1, set("planned"));
    ev(2, "claude", "node.set", 2, set("planned"));
    ev(3, "human", "node.set", 1, set("acked"));
    ev(4, "claude", "node.set", 1, set("doing"));
    ev(7, "claude", "node.set", 1, set("review"));
    ev(8, "human", "node.set", 1, set("done"));
    fs::write(log_path(dir.path()), lines.join("\n") + "\n").unwrap();
    let mut ws = Workspace::open(dir.path()).unwrap();
    ws.rebuild().unwrap();
    let now: jiff::Timestamp = at(10).parse().unwrap();
    let f = ws.flow(None, now).unwrap();
    let h = 3600;
    let of = |s: State| f.states.iter().find(|t| t.state == s).cloned().unwrap();
    // Backlog: LIV-1 1h, LIV-2 2h; the median of two is their mean, 1.5h; total 3h.
    assert_eq!(of(State::Backlog).median_secs, 3 * h / 2);
    assert_eq!(
        (of(State::Backlog).tasks, of(State::Backlog).total_secs),
        (2, 3 * h)
    );
    // Planned: LIV-1 2h, LIV-2 8h (still there at 10).
    assert_eq!(of(State::Planned).total_secs, 10 * h);
    assert_eq!(of(State::Doing).median_secs, 3 * h);
    assert_eq!(of(State::Review).total_secs, h);
    assert!(
        f.states.iter().all(|t| t.state != State::Done),
        "done counts no time"
    );
    // The ack queue: LIV-2, 8 hours.
    assert_eq!(f.queues.len(), 1);
    assert_eq!(
        (
            f.queues[0].state,
            f.queues[0].count,
            f.queues[0].oldest.as_str(),
            f.queues[0].oldest_secs
        ),
        (State::Planned, 1, "LIV-2", 8 * h)
    );
    assert_eq!(f.lead, None, "lead time is a project's");
}

#[test]
fn a_projects_flow_counts_its_own_tasks_and_its_lead_time() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = init(dir.path());
    let p = create(&mut ws, Kind::Project, "outcome", &[]);
    let a = create(&mut ws, Kind::Task, "a", &[(Rel::Parent, &p)]);
    task(&mut ws, "elsewhere");
    let now = jiff::Timestamp::now() + jiff::SignedDuration::from_hours(1);
    assert_eq!(
        ws.flow(Some(&p), now).unwrap().lead,
        None,
        "nothing started"
    );
    ack(&mut ws, &a);
    mv(&mut ws, &a, State::Doing, &CLAUDE).unwrap();
    let f = ws.flow(Some(&p), now).unwrap();
    let lead = f.lead.unwrap();
    assert!(lead.finished.is_none());
    assert!(lead.secs >= 3500, "about an hour to now: {}", lead.secs);
    // Only its own task: one in backlog, not two.
    let backlog = f.states.iter().find(|t| t.state == State::Backlog).unwrap();
    assert_eq!(backlog.tasks, 1);
}
