//! The app's behaviour, without a window: what the human sees and what their actions do.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::path::Path;
use std::time::{Duration, Instant};

use livtk_app::model::{
    GroupKey, Grouping, Layout, Model, NOTICE_FOR, Ordering, RETRY_FIRST, RETRY_MAX, View, choices,
};
use livtk_core::{Actor, Command, Kind, Rel, State, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

fn project(dir: &Path) -> Workspace {
    let mut ws = Workspace::init(dir, Some("LIV")).unwrap();
    ws.set_models_dir(None);
    ws
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
            &CLAUDE,
        )
        .unwrap();
    match &e[0].op {
        livtk_core::event::Op::NodeCreate { data, .. } => data.key.clone(),
        _ => unreachable!(),
    }
}

fn planned(ws: &mut Workspace, title: &str) -> String {
    let k = create(ws, Kind::Task, title);
    ws.execute(
        Command::Move {
            node: k.clone(),
            to: State::Planned,
        },
        &CLAUDE,
    )
    .unwrap();
    k
}

fn keys(m: &Model) -> Vec<String> {
    m.rows.iter().map(|r| r.key.clone()).collect()
}

#[test]
fn the_inbox_shows_what_waits_and_one_key_decides_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    let a = planned(&mut ws, "first plan");
    let b = planned(&mut ws, "second plan");
    let l = create(&mut ws, Kind::Lesson, "a rule");
    create(&mut ws, Kind::Task, "still in backlog");
    drop(ws);

    let mut m = Model::open(&[dir.path().to_path_buf()]);
    assert_eq!(m.view, View::Inbox);
    assert_eq!(keys(&m), [a.clone(), b.clone(), l.clone()]);
    assert_eq!(m.projects[0].inbox, 3);
    assert_eq!(m.selected().map(|r| r.key.as_str()), Some(a.as_str()));

    m.decide();
    assert_eq!(
        keys(&m),
        [b.clone(), l.clone()],
        "acknowledged work leaves the inbox"
    );
    assert_eq!(m.leaving.len(), 1, "and fades out");
    assert_eq!(
        m.selected().map(|r| r.key.as_str()),
        Some(b.as_str()),
        "the next row takes its place"
    );
    assert_eq!(m.projects[0].inbox, 2);

    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.node(&a).unwrap().state, State::Acked);
    assert_eq!(ws.history(Some(&a)).unwrap().last().unwrap().by, "human");
}

#[test]
fn choices_offer_only_legal_moves_with_decisions_first() {
    let labels = |k, s| {
        choices(k, s)
            .into_iter()
            .map(|c| c.label)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        labels(Kind::Task, State::Planned),
        ["Acknowledge", "Back to backlog", "Park", "Cancel"]
    );
    assert_eq!(
        labels(Kind::Lesson, State::Proposed),
        ["Ratify", "Ratify as standing rule", "Reject"]
    );
    assert_eq!(
        labels(Kind::Decision, State::Proposed),
        ["Accept", "Reject"]
    );
    assert_eq!(labels(Kind::Task, State::Done), ["Back to doing"]);
    assert!(labels(Kind::Finding, State::Recorded).is_empty());
    for c in choices(Kind::Task, State::Planned) {
        assert_eq!(c.decides, c.to == State::Acked);
    }
}

#[test]
fn ratifying_as_a_standing_rule_from_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    let l = create(&mut ws, Kind::Lesson, "Never push to the remote");
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    let standing = choices(Kind::Lesson, State::Proposed)
        .into_iter()
        .find(|c| c.always)
        .unwrap();
    m.choose(&standing);
    assert!(m.notice().is_none(), "{:?}", m.notice());
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.always_rules().unwrap()[0].key, l);
}

#[test]
fn a_gate_the_project_adds_is_the_humans_decision_in_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    let k = planned(&mut ws, "ship it");
    ws.execute(
        Command::Move {
            node: k.clone(),
            to: State::Acked,
        },
        &Actor::Human,
    )
    .unwrap();
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
    drop(ws);
    let config = dir.path().join(".livtk/config.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        text + "\n[gates]\nhuman = [\"task: review -> done\"]\n",
    )
    .unwrap();

    let mut m = Model::open(&[dir.path().to_path_buf()]);
    assert_eq!(keys(&m), std::slice::from_ref(&k), "it waits in the inbox");
    let done = m
        .choices(Kind::Task, State::Review)
        .into_iter()
        .find(|c| c.to == State::Done)
        .unwrap();
    assert!(done.decides, "marked as the human's decision");
    assert!(
        !choices(Kind::Task, State::Review)
            .into_iter()
            .any(|c| c.decides),
        "which the built-in gates alone do not make it"
    );
    let flow = livtk_app::model::steps_with(Kind::Task, State::Review, m.gates());
    let step = flow.iter().find(|s| s.state == State::Done).unwrap();
    assert_eq!(
        step.look(),
        livtk_app::model::Look::Decision,
        "drawn as one"
    );
    m.decide();
    assert!(m.notice().is_none(), "{:?}", m.notice());
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.node(&k).unwrap().state, State::Done, "and `a` takes it");
}

#[test]
fn changes_from_elsewhere_arrive_on_the_next_tick_and_flash() {
    let dir = tempfile::tempdir().unwrap();
    let mut claude = project(dir.path());
    let a = planned(&mut claude, "already there");
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    assert!(!m.tick(), "nothing changed");

    let b = planned(&mut claude, "new from Claude");
    assert!(m.tick());
    assert_eq!(keys(&m), [a.clone(), b.clone()]);
    assert!(m.flashed.contains_key(&m.rows[1].id), "the new row flashes");
    assert!(
        !m.flashed.contains_key(&m.rows[0].id),
        "the old one does not"
    );
    assert_eq!(
        m.selected().map(|r| r.key.as_str()),
        Some(a.as_str()),
        "the selection stays put"
    );

    claude
        .execute(
            Command::Edit {
                node: a.clone(),
                title: Some("renamed".into()),
                body: None,
            },
            &CLAUDE,
        )
        .unwrap();
    assert!(m.tick());
    assert!(
        m.flashed.contains_key(&m.rows[0].id),
        "an edited row flashes too"
    );
    m.settle(
        std::time::Instant::now() + std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(1),
    );
    assert!(m.flashed.is_empty());
}

#[test]
fn a_log_rewritten_by_git_is_caught_up_off_the_ticking_thread() {
    let dir = tempfile::tempdir().unwrap();
    let mut claude = project(dir.path());
    let a = planned(&mut claude, "first");
    let log = dir.path().join(".livtk/events.jsonl");
    let checkpoint = std::fs::read(&log).unwrap();
    let b = planned(&mut claude, "second");
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    assert_eq!(keys(&m), [a.clone(), b.clone()]);

    // A checkout back to before the second task.
    std::fs::write(&log, &checkpoint).unwrap();
    assert!(
        m.tick(),
        "the project is now catching up, which the sidebar shows"
    );
    assert!(m.projects[0].catching_up);
    assert_eq!(
        keys(&m),
        [a.clone(), b.clone()],
        "shown as it was: the tick replayed nothing"
    );
    let jobs = m.take_catch_ups();
    assert_eq!(jobs.len(), 1);
    assert!(!m.tick());
    assert!(m.take_catch_ups().is_empty(), "one at a time per project");

    let job = jobs[0].clone();
    let result = std::thread::spawn(move || job.run()).join().unwrap();
    assert_eq!(result, Ok(()));
    m.caught_up(&jobs[0], result);
    assert!(!m.projects[0].catching_up);
    assert!(
        m.tick(),
        "the next tick finds the store current and changed"
    );
    assert_eq!(keys(&m), [a]);
    assert!(m.take_catch_ups().is_empty());

    m.caught_up(&jobs[0], Err("the disk is full".into()));
    assert!(
        m.notice()
            .map(|n| n.text.as_str())
            .is_some_and(|n| n.contains("the disk is full")),
        "a failure is told: {:?}",
        m.notice()
    );
}

#[test]
fn the_selection_moves_and_stops_at_the_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    for t in ["a", "b", "c"] {
        planned(&mut ws, t);
    }
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.select_by(-1);
    assert_eq!(m.selected_index(), Some(0));
    m.select_by(5);
    assert_eq!(m.selected_index(), Some(2));
    m.select_by(-1);
    assert_eq!(m.selected_index(), Some(1));
}

#[test]
fn the_detail_panel_follows_the_selection_and_shows_notes() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    planned(&mut ws, "a");
    planned(&mut ws, "b");
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.open_detail();
    assert_eq!(m.detail.as_ref().unwrap().node.title, "a");
    assert_eq!(m.detail.as_ref().unwrap().choices[0].label, "Acknowledge");
    m.select_by(1);
    assert_eq!(
        m.detail.as_ref().unwrap().node.title,
        "b",
        "the panel follows"
    );
    m.add_note("looked at it, fine");
    let history = &m.detail.as_ref().unwrap().history;
    assert!(
        matches!(&history.last().unwrap().op, livtk_core::event::Op::NodeNote { data, .. } if data.text == "looked at it, fine")
    );
    m.close_detail();
    assert!(m.detail.is_none());
}

#[test]
fn a_new_task_from_the_app_lands_in_tasks_selected() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    assert!(m.create_task("Draw the first window", "", Some(2)));
    assert_eq!(m.view, View::Tasks);
    let sel = m.selected().unwrap();
    assert_eq!(
        (sel.title.as_str(), sel.priority),
        ("Draw the first window", 2)
    );
    m.set_priority(1);
    assert_eq!(m.selected().unwrap().priority, 1);
    assert!(!m.create_task("   ", "", None), "an empty title is refused");
    assert!(
        m.notice()
            .map(|n| n.text.as_str())
            .unwrap_or_default()
            .contains("empty")
    );
}

#[test]
fn a_refusal_becomes_a_notice_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    let first = create(&mut ws, Kind::Task, "first");
    let second = ws
        .execute(
            Command::Create {
                kind: Kind::Task,
                title: "second".into(),
                body: String::new(),
                priority: None,
                links: vec![(Rel::DependsOn, first)],
            },
            &CLAUDE,
        )
        .unwrap();
    let _ = second;
    ws.execute(
        Command::Move {
            node: "LIV-2".into(),
            to: State::Planned,
        },
        &CLAUDE,
    )
    .unwrap();
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.decide();
    m.set_view(View::Tasks);
    let row = m.rows.iter().find(|r| r.key == "LIV-2").unwrap().id;
    m.select(row);
    let start = choices(Kind::Task, State::Acked)
        .into_iter()
        .find(|c| c.to == State::Doing)
        .unwrap();
    m.choose(&start);
    assert!(
        m.notice()
            .map(|n| n.text.as_str())
            .unwrap_or_default()
            .contains("waits on LIV-1"),
        "{:?}",
        m.notice()
    );
}

/// A model of an empty project, for what does not need nodes.
fn empty_model(dir: &Path) -> Model {
    project(dir);
    let mut m = Model::open(&[dir.to_path_buf()]);
    m.set_models_dir(None);
    m
}

const FADE: Duration = Duration::from_millis(240);
const MS: Duration = Duration::from_millis(1);

#[test]
fn a_notice_leaves_on_its_own_after_its_time_and_a_fade() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = empty_model(dir.path());
    m.warn("the disk is full");
    let shown = Instant::now();

    assert!(!m.age_notice(shown + NOTICE_FOR - MS, FADE), "not yet");
    assert!(m.notice().is_some_and(|n| n.leaving.is_none()));

    let up = shown + NOTICE_FOR;
    assert!(m.age_notice(up, FADE), "its time is up: it starts to leave");
    assert_eq!(m.notice().and_then(|n| n.leaving), Some(up));
    assert!(!m.age_notice(up + FADE - MS, FADE), "fading");
    assert!(m.notice().is_some(), "still drawn while it fades");
    assert!(m.age_notice(up + FADE, FADE), "faded: gone");
    assert!(m.notice().is_none());
    assert!(!m.age_notice(up + FADE * 2, FADE), "nothing left to age");
}

#[test]
fn a_notice_under_the_pointer_stays_and_gets_its_whole_time_again_when_let_go() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = empty_model(dir.path());
    m.warn("refused");
    let shown = Instant::now();
    m.hold_notice(true, shown);
    assert!(
        !m.age_notice(shown + NOTICE_FOR * 10, FADE),
        "held, it stays"
    );
    assert!(m.notice().is_some_and(|n| n.leaving.is_none()));

    let let_go = shown + NOTICE_FOR * 10;
    m.hold_notice(false, let_go);
    assert!(
        !m.age_notice(let_go + NOTICE_FOR - MS, FADE),
        "its time starts again when the pointer leaves"
    );
    assert!(m.age_notice(let_go + NOTICE_FOR, FADE));

    // Caught while fading: the pointer brings it back.
    m.hold_notice(true, let_go + NOTICE_FOR + MS);
    assert!(m.notice().is_some_and(|n| n.leaving.is_none()));
    assert!(!m.age_notice(let_go + NOTICE_FOR + FADE * 2, FADE));
    assert!(m.notice().is_some());
}

#[test]
fn a_new_notice_replaces_the_old_and_starts_its_time_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = empty_model(dir.path());
    m.warn("refused");
    let first = m.notice().unwrap().id;
    let up = Instant::now() + NOTICE_FOR;
    assert!(m.age_notice(up, FADE), "the first is leaving");

    m.warn("refused");
    let n = m.notice().unwrap();
    assert!(n.id > first, "the same text again is a new notice");
    assert_eq!(n.text, "refused");
    assert_eq!(n.leaving, None, "it is not leaving");
    assert!(
        !m.age_notice(Instant::now() + NOTICE_FOR - MS, FADE),
        "its time started when it was shown"
    );

    m.hold_notice(true, Instant::now());
    m.warn("another");
    assert!(
        !m.age_notice(Instant::now() + NOTICE_FOR * 10, FADE),
        "it shows where the one it replaced did, under the same pointer, so it is held"
    );
}

#[test]
fn a_notice_closes_and_a_success_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    planned(&mut ws, "a plan");
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.warn("refused");
    m.dismiss_notice();
    assert!(m.notice().is_none(), "closed");

    m.warn("refused");
    m.decide();
    assert!(m.notice().is_none(), "an action that succeeds clears it");
}

/// Make `path` unreadable, or readable again.
#[cfg(unix)]
fn readable(path: &Path, yes: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mode = if yes { 0o644 } else { 0o000 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
#[test]
fn a_failure_that_lasts_is_told_once_not_on_every_tick() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    planned(&mut ws, "a plan");
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    // Its length unchanged, the log is read to see whether it changed, and cannot be.
    let log = dir.path().join(".livtk/events.jsonl");
    readable(&log, false);

    m.tick();
    let told = m.notice().map(|n| n.id).expect("the failure is told");
    m.tick();
    assert_eq!(
        m.notice().map(|n| n.id),
        Some(told),
        "not again: telling it again would start its time again"
    );
    m.dismiss_notice();
    m.tick();
    assert!(m.notice().is_none(), "closed, it stays closed");

    readable(&log, true);
    m.tick();
    assert!(m.notice().is_none());
    readable(&log, false);
    m.tick();
    assert!(
        m.notice().is_some_and(|n| n.id > told),
        "failing again after it passed is told again"
    );
    readable(&log, true);
}

#[test]
fn a_catch_up_that_keeps_failing_is_told_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut claude = project(dir.path());
    planned(&mut claude, "first");
    let log = dir.path().join(".livtk/events.jsonl");
    let checkpoint = std::fs::read(&log).unwrap();
    planned(&mut claude, "second");
    drop(claude);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    // A checkout: the store must catch up, and each try fails alike.
    std::fs::write(&log, &checkpoint).unwrap();
    let fail = |m: &mut Model, why: &str| {
        // Past any pause before the next try.
        m.tick_at(Instant::now() + RETRY_MAX);
        let jobs = m.take_catch_ups();
        assert_eq!(jobs.len(), 1, "the store is still behind: it tries again");
        m.caught_up(&jobs[0], Err(why.into()));
    };

    fail(&mut m, "the disk is full");
    let told = m.notice().map(|n| n.id).expect("the failure is told");
    fail(&mut m, "the disk is full");
    assert_eq!(
        m.notice().map(|n| n.id),
        Some(told),
        "not again on the next try"
    );
    m.dismiss_notice();
    fail(&mut m, "the disk is full");
    assert!(m.notice().is_none(), "closed, it stays closed");

    fail(&mut m, "the lock is held");
    assert!(
        m.notice()
            .is_some_and(|n| n.text.contains("the lock is held")),
        "a different failure is told"
    );
    m.dismiss_notice();
    m.tick_at(Instant::now() + RETRY_MAX);
    let jobs = m.take_catch_ups();
    m.caught_up(&jobs[0], Ok(()));
    fail(&mut m, "the lock is held");
    assert!(
        m.notice().is_some(),
        "failing again after it worked is told again"
    );
}

#[test]
fn a_failed_catch_up_is_tried_again_after_a_pause_that_grows() {
    let dir = tempfile::tempdir().unwrap();
    let mut claude = project(dir.path());
    planned(&mut claude, "first");
    let log = dir.path().join(".livtk/events.jsonl");
    let checkpoint = std::fs::read(&log).unwrap();
    planned(&mut claude, "second");
    drop(claude);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    std::fs::write(&log, &checkpoint).unwrap();
    // Time is the test's own: every try and every failure happens at a moment it names.
    let mut now = Instant::now();
    let starts = |m: &mut Model, at: Instant| {
        m.tick_at(at);
        m.take_catch_ups()
    };
    const TICK: Duration = Duration::from_millis(1);

    let jobs = starts(&mut m, now);
    assert_eq!(jobs.len(), 1, "the first try starts at once");
    let mut pause = RETRY_FIRST;
    for _ in 0..8 {
        m.caught_up_at(&jobs[0], Err("the store is read-only".into()), now);
        assert!(
            starts(&mut m, now).is_empty(),
            "not again at once after a failure"
        );
        assert!(
            starts(&mut m, now + pause - TICK).is_empty(),
            "nor before its pause of {pause:?} is over"
        );
        now += pause;
        assert_eq!(starts(&mut m, now).len(), 1, "but once it is");
        pause = (pause * 2).min(RETRY_MAX);
    }
    assert_eq!(pause, RETRY_MAX, "it grows to its longest and stays there");

    m.caught_up_at(&jobs[0], Ok(()), now);
    assert_eq!(
        starts(&mut m, now).len(),
        1,
        "once one worked, the next one needed starts at once"
    );
    m.caught_up_at(&jobs[0], Err("again".into()), now);
    assert!(starts(&mut m, now + RETRY_FIRST - TICK).is_empty());
    assert_eq!(
        starts(&mut m, now + RETRY_FIRST).len(),
        1,
        "and its pause starts short again"
    );

    // Caught up by another process while a pause runs: found current, the pause is gone.
    m.caught_up_at(&jobs[0], Err("again".into()), now);
    jobs[0].run().unwrap();
    assert!(
        starts(&mut m, now).is_empty(),
        "current: nothing to catch up"
    );
    std::fs::write(&log, b"").unwrap();
    assert_eq!(
        starts(&mut m, now).len(),
        1,
        "so the next rewrite is caught up at once, not after the old pause"
    );
}

#[test]
fn what_claude_puts_in_the_inbox_arrives_once_and_the_humans_own_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut claude = project(dir.path());
    planned(&mut claude, "already waiting");
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    assert!(
        m.take_arrivals().is_empty(),
        "what waited at the start is not news"
    );

    let k = planned(&mut claude, "a new plan");
    m.tick();
    let arrived = m.take_arrivals();
    assert_eq!(
        arrived
            .iter()
            .map(|a| (a.key.as_str(), a.title.as_str()))
            .collect::<Vec<_>>(),
        [(k.as_str(), "a new plan")]
    );
    m.tick();
    assert!(m.take_arrivals().is_empty(), "told once");

    // Two at once are one batch.
    planned(&mut claude, "third");
    create(&mut claude, Kind::Lesson, "a rule");
    m.tick();
    assert_eq!(m.take_arrivals().len(), 2);

    // The human's own, from a terminal, is not news to them.
    let mut human = Workspace::open(dir.path()).unwrap();
    let mine = create(&mut human, Kind::Task, "mine");
    human
        .execute(
            Command::Move {
                node: mine,
                to: State::Planned,
            },
            &Actor::Human,
        )
        .unwrap();
    m.tick();
    assert!(m.take_arrivals().is_empty(), "the human planned it");

    // Opening an arrival goes to the inbox, on it.
    m.set_view(View::Tasks);
    let k2 = planned(&mut claude, "open me");
    m.tick();
    let a = m.take_arrivals().pop().unwrap();
    m.open_arrival(&a);
    assert_eq!(m.view, View::Inbox);
    assert_eq!(m.selected().map(|r| r.key.as_str()), Some(k2.as_str()));
    assert!(m.detail.is_some());
}

#[test]
fn an_arrival_is_whoever_put_it_in_the_inbox_whatever_came_after() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    let mut claude = Workspace::open(dir.path()).unwrap();
    let mut human = Workspace::open(dir.path()).unwrap();
    let note = |ws: &mut Workspace, by: &Actor, key: &str| {
        ws.execute(
            Command::Note {
                node: key.into(),
                text: "a note".into(),
            },
            by,
        )
        .unwrap();
    };

    // The human plans from a terminal; Claude notes it before the next look: not news.
    let mine = create(&mut human, Kind::Task, "mine");
    human
        .execute(
            Command::Move {
                node: mine.clone(),
                to: State::Planned,
            },
            &Actor::Human,
        )
        .unwrap();
    note(&mut claude, &CLAUDE, &mine);
    m.tick();
    assert!(m.take_arrivals().is_empty(), "the human planned it");

    // Claude plans; the human notes it before the next look: news all the same.
    let theirs = planned(&mut claude, "theirs");
    note(&mut human, &Actor::Human, &theirs);
    m.tick();
    let keys: Vec<String> = m.take_arrivals().into_iter().map(|a| a.key).collect();
    assert_eq!(keys, [theirs], "Claude planned it");

    // Claude proposes a lesson learned from the human's task: the lesson arrives, not the task.
    let lesson = create(&mut claude, Kind::Lesson, "a rule");
    claude
        .execute(
            Command::Link {
                src: lesson.clone(),
                rel: Rel::LearnedFrom,
                dst: mine,
            },
            &CLAUDE,
        )
        .unwrap();
    m.tick();
    let keys: Vec<String> = m.take_arrivals().into_iter().map(|a| a.key).collect();
    assert_eq!(keys, [lesson]);
}

#[test]
fn why_no_notifications_come_is_told_once_or_each_time_and_never_over_another_notice() {
    use livtk_app::model::Quiet;
    let dir = tempfile::tempdir().unwrap();
    let mut m = empty_model(dir.path());

    // A notice already showing (a project that would not open) stays, and the missing bundle
    // is told another time.
    m.warn("a project would not open");
    m.tell_quiet(Quiet::NoBundle);
    assert_eq!(m.notice().unwrap().text, "a project would not open");
    m.dismiss_notice();

    m.tell_quiet(Quiet::NoBundle);
    let told = m.notice().map(|n| n.text.clone()).expect("told");
    assert!(told.contains("livtk.app"), "{told}");
    m.dismiss_notice();
    m.tell_quiet(Quiet::NoBundle);
    assert!(m.notice().is_none(), "a missing bundle is told once");

    // And not after a restart: it is remembered.
    let saved = m.saved();
    let mut again = Model::open(&[dir.path().to_path_buf()]);
    again.restore(&saved);
    again.tell_quiet(Quiet::NoBundle);
    assert!(again.notice().is_none(), "not after a restart either");

    // A bundle that would not open is told each time.
    for _ in 0..2 {
        again.tell_quiet(Quiet::OpenFailed);
        assert!(again.notice().is_some());
        again.dismiss_notice();
    }
}

#[test]
fn a_note_goes_to_the_node_open_in_the_panel_even_when_the_selection_moved() {
    let dir = tempfile::tempdir().unwrap();
    let mut claude = project(dir.path());
    let first = planned(&mut claude, "first");
    let second = planned(&mut claude, "second");
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Tasks);
    m.select_by(1);
    m.open_detail();
    assert_eq!(m.detail.as_ref().unwrap().node.key, second);

    // Claude cancels it: it leaves the list and the selection moves on, the panel stays.
    claude
        .execute(
            Command::Move {
                node: second.clone(),
                to: State::Canceled,
            },
            &CLAUDE,
        )
        .unwrap();
    m.tick();
    assert_eq!(m.selected().map(|r| r.key.clone()), Some(first.clone()));
    assert_eq!(m.detail.as_ref().unwrap().node.key, second);

    // The palette says so too: its actions name the node in the panel.
    m.open_palette();
    m.palette_query("add a note");
    let context = match &m.palette.as_ref().unwrap().items[0] {
        livtk_app::model::Item::Action(a) => a.context.clone(),
        livtk_app::model::Item::Node(_) => None,
    };
    assert_eq!(context.as_deref(), Some(second.as_str()));
    m.close_palette();

    m.add_note("written in its panel");
    let ws = Workspace::open(dir.path()).unwrap();
    let last = ws.history(Some(&second)).unwrap().pop().unwrap();
    assert!(
        matches!(&last.op, livtk_core::event::Op::NodeNote { data, .. } if data.text == "written in its panel"),
        "{last:?}"
    );
    assert!(
        !ws.history(Some(&first))
            .unwrap()
            .iter()
            .any(|e| matches!(e.op, livtk_core::event::Op::NodeNote { .. })),
        "not on the row the selection moved to"
    );
}

#[test]
fn views_layouts_projects_and_a_bad_folder() {
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut ws = project(d1.path());
    create(&mut ws, Kind::Lesson, "rule one");
    let mut other = Workspace::init(d2.path(), Some("OTH")).unwrap();
    other.set_models_dir(None);
    planned(&mut other, "elsewhere");
    let missing = d1.path().join("nothing-here");
    let mut m = Model::open(&[d1.path().to_path_buf(), missing, d2.path().to_path_buf()]);
    assert_eq!(m.projects.len(), 2);
    assert!(m.notice().is_some(), "the bad folder is reported");

    m.set_view(View::Lessons);
    assert_eq!(keys(&m), ["LES-1"]);
    m.select_project(1);
    assert_eq!(m.view, View::Lessons);
    assert!(m.rows.is_empty());
    m.set_view(View::Tasks);
    assert_eq!(keys(&m), ["OTH-1"]);
    m.toggle_layout();
    assert_eq!(m.layout, Layout::Board);
    // Every column, empty ones included: their order is the point here.
    m.toggle_empty();
    let cols: Vec<(GroupKey, usize)> = m.columns().into_iter().map(|(s, r)| (s, r.len())).collect();
    assert_eq!(cols[1], (GroupKey::State(State::Planned), 1));
    assert_eq!(
        cols.iter().map(|c| c.0).collect::<Vec<_>>(),
        [
            State::Backlog,
            State::Planned,
            State::Acked,
            State::Doing,
            State::Review,
            State::Done,
            State::Parked
        ]
        .map(GroupKey::State)
    );
}

/// Tasks across states and priorities: LIV-1 P1 backlog, LIV-2 P3 planned "parser",
/// LIV-3 P2 planned "renderer", LIV-4 P4 backlog "parser tests".
fn mixed() -> (tempfile::TempDir, Model) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    for (title, p, plan) in [
        ("draw the window", 1, false),
        ("write the parser", 3, true),
        ("write the renderer", 2, true),
        ("parser tests", 4, false),
    ] {
        let k = create(&mut ws, Kind::Task, title);
        ws.execute(
            Command::SetPriority {
                node: k.clone(),
                priority: p,
            },
            &CLAUDE,
        )
        .ok();
        if plan {
            ws.execute(
                Command::Move {
                    node: k,
                    to: State::Planned,
                },
                &CLAUDE,
            )
            .unwrap();
        }
    }
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Tasks);
    (dir, m)
}

/// The list's bands: each group's key and how many rows it holds.
fn bands(m: &Model) -> Vec<(GroupKey, usize)> {
    m.groups.iter().map(|g| (g.key, g.count)).collect()
}

/// Every group's rows are where it says, and of its group.
fn groups_hold_their_rows(m: &Model) {
    for g in &m.groups {
        let rows = &m.rows[g.start..g.start + g.shown];
        if let GroupKey::State(s) = g.key {
            assert!(rows.iter().all(|r| r.state == s), "{g:?}");
        }
    }
    assert_eq!(
        m.groups.iter().map(|g| g.shown).sum::<usize>(),
        m.rows.len()
    );
}

#[test]
fn empty_groups_list_every_state_in_order_and_dismissed_ones_only_when_shown() {
    use GroupKey::State as S;
    let (dir, mut m) = mixed();
    let mut ws = Workspace::open(dir.path()).unwrap();
    let k = create(&mut ws, Kind::Task, "dropped");
    ws.execute(
        Command::Move {
            node: k,
            to: State::Canceled,
        },
        &CLAUDE,
    )
    .unwrap();
    drop(ws);
    m.tick();
    assert_eq!(
        bands(&m),
        [(S(State::Backlog), 2), (S(State::Planned), 2)],
        "off, only groups with rows, as before"
    );

    m.toggle_empty();
    assert_eq!(
        bands(&m),
        [
            (S(State::Backlog), 2),
            (S(State::Planned), 2),
            (S(State::Acked), 0),
            (S(State::Doing), 0),
            (S(State::Review), 0),
            (S(State::Done), 0),
            (S(State::Parked), 0),
        ],
        "every state of the kind in its own order, empty ones with 0; dismissed left out"
    );
    assert_eq!(m.rows.len(), 4, "the same rows");
    groups_hold_their_rows(&m);

    m.toggle_dismissed();
    assert_eq!(
        bands(&m).last(),
        Some(&(S(State::Canceled), 1)),
        "dismissed states join when dismissed work is shown"
    );
    groups_hold_their_rows(&m);

    m.set_view(View::Lessons);
    let d = m.display();
    assert!(
        !d.show_empty && !d.show_dismissed,
        "each view has its own options"
    );
    assert!(m.groups.is_empty(), "so no lesson's group shows");
    m.toggle_empty();
    assert_eq!(
        bands(&m),
        [(S(State::Proposed), 0), (S(State::Ratified), 0)],
        "a lesson's states, without the dismissed"
    );
    m.set_view(View::Tasks);
    assert!(m.display().show_empty && m.display().show_dismissed);
}

#[test]
fn empty_groups_by_priority_stay_through_a_filter_and_collapse() {
    use GroupKey::Priority as P;
    let (_dir, mut m) = mixed();
    m.set_grouping(Grouping::Priority);
    m.set_filter("parser");
    m.toggle_empty();
    assert_eq!(
        bands(&m),
        [(P(1), 0), (P(2), 0), (P(3), 1), (P(4), 1)],
        "a filter that empties a group leaves it listed"
    );
    m.toggle_collapsed(P(1));
    m.set_filter("");
    let urgent = &m.groups[0];
    assert_eq!(
        (urgent.key, urgent.count, urgent.shown, urgent.collapsed),
        (P(1), 1, 0, true),
        "an empty group collapses as any other, and keeps it once it has rows"
    );
    groups_hold_their_rows(&m);

    m.set_grouping(Grouping::None);
    assert_eq!(
        bands(&m),
        [(GroupKey::All, 4)],
        "ungrouped, there are no empty groups to show"
    );
}

#[test]
fn the_boards_empty_columns_show_only_with_empty_groups() {
    use GroupKey::{Priority as P, State as S};
    let (_dir, mut m) = mixed();
    m.toggle_layout();
    let columns = |m: &Model| {
        m.columns()
            .into_iter()
            .map(|(k, rows)| (k, rows.len()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        columns(&m),
        [(S(State::Backlog), 2), (S(State::Planned), 2)],
        "off, the columns holding cards, in working order"
    );

    m.toggle_empty();
    assert_eq!(
        columns(&m),
        [
            (S(State::Backlog), 2),
            (S(State::Planned), 2),
            (S(State::Acked), 0),
            (S(State::Doing), 0),
            (S(State::Review), 0),
            (S(State::Done), 0),
            (S(State::Parked), 0),
        ],
        "on, every column, in the order the list's groups have"
    );
    m.toggle_dismissed();
    let tail: Vec<GroupKey> = columns(&m)
        .iter()
        .rev()
        .take(3)
        .rev()
        .map(|c| c.0)
        .collect();
    assert_eq!(
        tail,
        [S(State::Done), S(State::Parked), S(State::Canceled)],
        "parked stays after done whether dismissed work is shown or not, canceled last"
    );

    m.toggle_empty();
    m.set_filter("parser");
    assert_eq!(
        columns(&m),
        [(S(State::Backlog), 1), (S(State::Planned), 1)],
        "a filter that empties a column hides it"
    );
    m.set_grouping(Grouping::Priority);
    assert_eq!(columns(&m), [(P(3), 1), (P(4), 1)], "priorities alike");
    m.set_filter("nothing matches this");
    assert!(
        m.columns().is_empty(),
        "and none at all when nothing matches"
    );
}

#[test]
fn the_filter_narrows_by_state_with_priority_and_text_and_shapes_groups_and_columns() {
    use GroupKey::State as S;
    let (_dir, mut m) = mixed();
    m.toggle_state(State::Planned);
    assert_eq!(keys(&m), ["LIV-3", "LIV-2"], "planned work only");
    m.toggle_state(State::Backlog);
    assert_eq!(
        m.display().states,
        [State::Backlog, State::Planned],
        "in the kind's order"
    );
    assert_eq!(m.rows.len(), 4);
    m.toggle_priority(1);
    assert_eq!(keys(&m), ["LIV-1"], "with priorities too");
    m.set_filter("nothing");
    assert!(m.rows.is_empty(), "and with text");
    m.clear_filters();
    let d = m.display();
    assert!(d.filter.is_empty() && d.priorities.is_empty() && d.states.is_empty());
    assert_eq!(m.rows.len(), 4, "clear brings everything back");

    m.toggle_state(State::Planned);
    m.toggle_empty();
    assert_eq!(
        bands(&m),
        [(S(State::Planned), 2)],
        "empty groups are the filter's states only"
    );
    m.toggle_layout();
    assert_eq!(
        m.columns().into_iter().map(|c| c.0).collect::<Vec<_>>(),
        [S(State::Planned)],
        "on the board too"
    );

    m.set_view(View::Lessons);
    assert!(m.display().states.is_empty(), "each view has its own");
}

#[test]
fn hiding_dismissed_work_unchecks_a_dismissed_state() {
    let (_dir, mut m) = mixed();
    m.toggle_dismissed();
    m.toggle_state(State::Canceled);
    m.toggle_state(State::Planned);
    m.toggle_dismissed();
    assert_eq!(
        m.display().states,
        [State::Planned],
        "a hidden state would filter for rows that are not there"
    );
    assert_eq!(m.rows.len(), 2);
}

#[test]
fn groups_follow_state_order_and_rows_follow_priority() {
    let (_dir, m) = mixed();
    let groups: Vec<(GroupKey, usize)> = m.groups.iter().map(|g| (g.key, g.count)).collect();
    assert_eq!(
        groups,
        [
            (GroupKey::State(State::Backlog), 2),
            (GroupKey::State(State::Planned), 2)
        ],
        "the task's states in their own order: backlog before planned"
    );
    assert_eq!(
        keys(&m),
        ["LIV-1", "LIV-4", "LIV-3", "LIV-2"],
        "P1 before P4, P2 before P3"
    );
}

#[test]
fn a_filter_narrows_by_title_or_key_and_priority_chips_combine() {
    let (_dir, mut m) = mixed();
    m.set_filter("PARSER");
    assert_eq!(keys(&m), ["LIV-4", "LIV-2"], "backlog's first");
    m.toggle_priority(4);
    assert_eq!(keys(&m), ["LIV-4"]);
    m.set_filter("");
    m.toggle_priority(1);
    assert_eq!(keys(&m), ["LIV-1", "LIV-4"]);
    m.set_filter("liv-1");
    assert_eq!(keys(&m), ["LIV-1"]);
    m.set_view(View::Inbox);
    assert_eq!(m.display().filter, "", "display options are per view");
    m.set_view(View::Tasks);
    assert_eq!(m.display().filter, "liv-1", "and kept when you come back");
}

#[test]
fn grouping_and_ordering_change_the_arrangement() {
    let (_dir, mut m) = mixed();
    m.set_grouping(Grouping::Priority);
    let groups: Vec<GroupKey> = m.groups.iter().map(|g| g.key).collect();
    assert_eq!(
        groups,
        [
            GroupKey::Priority(1),
            GroupKey::Priority(2),
            GroupKey::Priority(3),
            GroupKey::Priority(4)
        ]
    );
    m.set_grouping(Grouping::None);
    m.set_ordering(Ordering::Created);
    assert_eq!(m.groups.len(), 1);
    assert_eq!(
        keys(&m),
        ["LIV-4", "LIV-3", "LIV-2", "LIV-1"],
        "newest first"
    );
    m.set_ordering(Ordering::Priority);
    assert_eq!(keys(&m), ["LIV-1", "LIV-3", "LIV-2", "LIV-4"]);
}

#[test]
fn a_collapsed_group_keeps_its_count_and_drops_out_of_navigation() {
    let (_dir, mut m) = mixed();
    m.toggle_collapsed(GroupKey::State(State::Planned));
    assert_eq!(keys(&m), ["LIV-1", "LIV-4"]);
    let planned = m
        .groups
        .iter()
        .find(|g| g.key == GroupKey::State(State::Planned))
        .unwrap();
    assert_eq!(planned.count, 2, "still counted");
    assert!(planned.collapsed);
    assert_eq!(
        m.selected().map(|r| r.key.as_str()),
        Some("LIV-1"),
        "the selection leaves the hidden rows"
    );
    m.select_by(1);
    assert_eq!(m.selected().map(|r| r.key.as_str()), Some("LIV-4"));
    m.select_by(5);
    assert_eq!(
        m.band(),
        Some(GroupKey::State(State::Planned)),
        "past the rows, the folded band, never its rows"
    );
    assert_eq!(m.filtered.len(), 4, "the board still shows everything");
    m.toggle_collapsed(GroupKey::State(State::Planned));
    assert_eq!(m.rows.len(), 4);
}

#[test]
fn dropping_moves_state_or_priority_and_refuses_with_the_reason() {
    let (dir, mut m) = mixed();
    let id = |m: &Model, k: &str| m.filtered.iter().find(|r| r.key == k).unwrap().id;
    let liv1 = id(&m, "LIV-1");
    assert!(
        !m.can_drop(liv1, GroupKey::State(State::Doing)),
        "backlog cannot jump to doing"
    );
    assert!(m.can_drop(liv1, GroupKey::State(State::Planned)));
    m.drop_on(liv1, GroupKey::State(State::Doing));
    assert!(
        m.notice()
            .map(|n| n.text.as_str())
            .unwrap_or_default()
            .contains("can move to"),
        "{:?}",
        m.notice()
    );

    m.drop_on(liv1, GroupKey::State(State::Planned));
    assert!(m.notice().is_none());
    let liv2 = id(&m, "LIV-2");
    m.drop_on(liv2, GroupKey::State(State::Acked));
    m.drop_on(liv2, GroupKey::Priority(1));
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.node("LIV-1").unwrap().state, State::Planned);
    let n2 = ws.node("LIV-2").unwrap();
    assert_eq!((n2.state, n2.priority), (State::Acked, 1));
    assert_eq!(
        ws.history(Some("LIV-2")).unwrap().last().unwrap().by,
        "human"
    );
}

#[test]
fn done_work_stays_in_view_and_can_be_reopened_by_dragging() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    let k = planned(&mut ws, "finished work");
    let canceled = create(&mut ws, Kind::Task, "dropped idea");
    ws.execute(
        Command::Move {
            node: canceled,
            to: State::Canceled,
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
    for to in [State::Doing, State::Review, State::Done] {
        ws.execute(
            Command::Move {
                node: k.clone(),
                to,
            },
            &CLAUDE,
        )
        .unwrap();
    }
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Tasks);
    assert_eq!(
        keys(&m),
        std::slice::from_ref(&k),
        "done shows; canceled does not"
    );
    assert_eq!(m.groups[0].key, GroupKey::State(State::Done));
    let done_col = m
        .columns()
        .into_iter()
        .find(|(s, _)| *s == GroupKey::State(State::Done))
        .unwrap();
    assert_eq!(done_col.1.len(), 1, "the board's Done column holds it");

    let id = m.rows[0].id;
    assert!(m.can_drop(id, GroupKey::State(State::Doing)));
    m.drop_on(id, GroupKey::State(State::Doing));
    assert!(m.notice().is_none(), "{:?}", m.notice());
    assert_eq!(
        Workspace::open(dir.path()).unwrap().node(&k).unwrap().state,
        State::Doing
    );

    m.toggle_dismissed();
    assert_eq!(m.rows.len(), 2, "h brings back what was dismissed");
}

#[test]
fn board_columns_follow_the_grouping_and_a_drop_sets_that_property() {
    let (dir, mut m) = mixed();
    m.set_grouping(Grouping::None);
    m.toggle_layout();
    assert_eq!(m.layout, Layout::Board);
    assert_eq!(
        m.display().grouping,
        Grouping::State,
        "a board is never ungrouped"
    );
    m.set_grouping(Grouping::None);
    assert_eq!(m.display().grouping, Grouping::State, "not even when asked");

    m.set_grouping(Grouping::Priority);
    let cols: Vec<(GroupKey, Vec<String>)> = m
        .columns()
        .into_iter()
        .map(|(k, rows)| (k, rows.into_iter().map(|r| r.key.clone()).collect()))
        .collect();
    assert_eq!(
        cols,
        [
            (GroupKey::Priority(1), vec!["LIV-1".to_owned()]),
            (GroupKey::Priority(2), vec!["LIV-3".to_owned()]),
            (GroupKey::Priority(3), vec!["LIV-2".to_owned()]),
            (GroupKey::Priority(4), vec!["LIV-4".to_owned()]),
        ]
    );
    let liv4 = m.filtered.iter().find(|r| r.key == "LIV-4").unwrap().id;
    m.drop_on(liv4, GroupKey::Priority(1));
    let urgent: Vec<&str> = m.columns()[0].1.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(urgent, ["LIV-1", "LIV-4"], "dropped into Urgent");
    assert_eq!(
        Workspace::open(dir.path())
            .unwrap()
            .node("LIV-4")
            .unwrap()
            .priority,
        1
    );

    m.set_ordering(Ordering::Created);
    let urgent: Vec<&str> = m.columns()[0].1.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(
        urgent,
        ["LIV-4", "LIV-1"],
        "the ordering applies inside a column"
    );
}

/// `cargo test --release -p livtk-app --test model -- --ignored --nocapture`: the app's budgets
/// on 10,000 tasks.
#[test]
#[ignore = "a benchmark: run in release with --ignored"]
fn ten_thousand_tasks_stay_fast() {
    use livtk_core::event::{Event, NodeData, Op};
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut lines = String::new();
    let mut last = None;
    for n in 1..=10_000u32 {
        let state = [State::Backlog, State::Planned, State::Doing, State::Done][n as usize % 4];
        let e = Event::new(
            last,
            &CLAUDE,
            Op::NodeCreate {
                node: uuid::Uuid::now_v7(),
                data: NodeData {
                    kind: Kind::Task,
                    key: format!("LIV-{n}"),
                    title: format!("task number {n} with a title of ordinary length"),
                    body: String::new(),
                    state,
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
    Workspace::open(dir.path()).unwrap(); // the store, built once, as it would already exist

    let t = Instant::now();
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Tasks);
    let open = t.elapsed();
    let ticks: Vec<Duration> = (0..50)
        .map(|_| {
            let t = Instant::now();
            assert!(!m.tick());
            t.elapsed()
        })
        .collect();
    let tick = ticks.iter().min().unwrap();
    eprintln!(
        "10k tasks: open and list {open:?}, idle tick {tick:?}, rows {}",
        m.rows.len()
    );
    assert!(open < Duration::from_millis(300), "open took {open:?}");
    assert!(
        *tick < Duration::from_millis(1),
        "an idle tick took {tick:?}"
    );
}

/// What "never stalls" means for the tick that finds a rewritten log: well inside a 16 ms frame,
/// and far below the replay it leaves to the job (0.93 s on the developer's Linux machine, 0.75 to
/// 0.77 s on an Apple silicon Mac). Measured: 84 µs on Linux; 1.1 to 1.4 ms on that Mac, where the
/// file system's first touch of the log just rewritten costs about a millisecond and later ticks
/// take 40 µs, so a 1 ms budget failed there by chance.
const TICK_BUDGET: Duration = Duration::from_millis(5);

/// `cargo test --release -p livtk-app --test model -- --ignored --nocapture`: after git rewrites
/// a log of 100,000 events, the window's tick stays a tick; the replay happens in the job.
#[test]
#[ignore = "a benchmark: run in release with --ignored"]
fn a_rewritten_log_of_100k_events_never_stalls_the_tick() {
    use livtk_core::event::{Event, NodeData, Op};
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut lines = Vec::new();
    let mut last = None;
    for n in 1..=100_000u32 {
        let e = Event::new(
            last,
            &CLAUDE,
            Op::NodeCreate {
                node: uuid::Uuid::now_v7(),
                data: NodeData {
                    kind: Kind::Task,
                    key: format!("LIV-{n}"),
                    title: format!("task number {n} with a title of ordinary length"),
                    body: String::new(),
                    state: State::Backlog,
                    priority: livtk_core::model::Priority::DEFAULT,
                    props: serde_json::Map::new(),
                },
            },
        );
        last = Some(e.id);
        lines.push(e.to_line().unwrap());
    }
    let log = dir.path().join(".livtk/events.jsonl");
    std::fs::write(&log, lines.join("\n") + "\n").unwrap();
    let mut m = Model::open(&[dir.path().to_path_buf()]);

    // As a merge leaves it: the same events, the last two in the other order.
    let n = lines.len();
    lines.swap(n - 1, n - 2);
    std::fs::write(&log, lines.join("\n") + "\n").unwrap();
    let t = Instant::now();
    assert!(m.tick());
    let tick = t.elapsed();
    let jobs = m.take_catch_ups();
    assert_eq!(jobs.len(), 1);
    let t = Instant::now();
    let result = jobs[0].run();
    let replay = t.elapsed();
    assert_eq!(result, Ok(()));
    m.caught_up(&jobs[0], result);
    let t = Instant::now();
    assert!(m.tick());
    let after = t.elapsed();
    eprintln!(
        "100k events rewritten: tick {tick:?}, replay in the job {replay:?}, tick after {after:?}"
    );
    assert!(tick < TICK_BUDGET, "the tick took {tick:?}");
}

/// Whatever the human does that moves the selection (a drag, a filter, a group folded) moves the
/// open panel with it, so the next key acts on the node in view, never one out of sight.
#[test]
fn the_panel_follows_a_drag_a_filter_and_a_fold() {
    let (_dir, mut m) = mixed();
    let id = |m: &Model, k: &str| m.filtered.iter().find(|r| r.key == k).unwrap().id;
    let panel = |m: &Model| m.detail.as_ref().map(|d| d.node.key.clone());
    m.select(id(&m, "LIV-1"));
    m.open_detail();
    m.drop_on(id(&m, "LIV-2"), GroupKey::Priority(1));
    assert_eq!(panel(&m).as_deref(), Some("LIV-2"), "the card dragged");
    assert_eq!(m.acted_on().map(|r| r.key), Some("LIV-2".into()));

    m.set_filter("renderer");
    assert_eq!(
        panel(&m).as_deref(),
        Some("LIV-3"),
        "the first that matches"
    );
    assert_eq!(m.selected().map(|r| r.key.as_str()), Some("LIV-3"));
    m.set_filter("");

    m.select(id(&m, "LIV-2"));
    let at = m.selected_index().unwrap();
    let group = m
        .groups
        .iter()
        .find(|g| (g.start..g.start + g.shown).contains(&at))
        .map(|g| g.key)
        .unwrap();
    m.toggle_collapsed(group);
    let shown = m.selected().map(|r| r.key.clone());
    assert_ne!(shown.as_deref(), Some("LIV-2"), "folded away");
    assert_eq!(panel(&m), shown, "the panel went with the selection");
}

/// A filter cleared brings the panel's node back, and the selection to it, not to the first row;
/// a card dragged on the board moves the panel even when its list group is folded.
#[test]
fn the_panel_keeps_its_node_through_a_filter_and_follows_a_folded_card() {
    let (_dir, mut m) = mixed();
    let id = |m: &Model, k: &str| m.filtered.iter().find(|r| r.key == k).unwrap().id;
    let panel = |m: &Model| m.detail.as_ref().map(|d| d.node.key.clone());
    m.select(id(&m, "LIV-2"));
    m.open_detail();
    m.set_filter("zzzz");
    m.set_filter("");
    assert_eq!(panel(&m).as_deref(), Some("LIV-2"));
    assert_eq!(m.selected().map(|r| r.key.as_str()), Some("LIV-2"));
    m.set_filter("w");
    m.set_filter("wq");
    m.set_filter("w");
    assert_eq!(
        panel(&m).as_deref(),
        Some("LIV-2"),
        "still matching, still open"
    );

    let liv1 = id(&m, "LIV-1");
    let at = m.rows.iter().position(|r| r.id == liv1).unwrap();
    let group = m
        .groups
        .iter()
        .find(|g| (g.start..g.start + g.shown).contains(&at))
        .map(|g| g.key)
        .unwrap();
    m.toggle_collapsed(group);
    m.select(id(&m, "LIV-2"));
    m.toggle_layout();
    m.drop_on(liv1, GroupKey::State(State::Planned));
    assert_eq!(panel(&m).as_deref(), Some("LIV-1"), "the card dragged");
}

/// In a list, j and k stand on each group's band as well as its rows, and Enter on a band
/// folds or unfolds it, the selection staying on it.
#[test]
fn j_and_k_stand_on_bands_and_enter_folds_one() {
    let (_dir, mut m) = mixed();
    assert!(m.groups.len() >= 2, "grouped by state");
    // The first row is selected; up from it, its band.
    m.select_by(-1);
    assert_eq!(m.band(), Some(m.groups[0].key), "the first band");
    assert!(m.selected().is_none());
    m.select_by(1);
    let first = m.rows[m.groups[0].start].key.clone();
    assert_eq!(
        m.selected().map(|r| r.key.clone()),
        Some(first),
        "then its first row"
    );
    for _ in 0..m.groups[0].shown {
        m.select_by(1);
    }
    let second = m.groups[1].key;
    assert_eq!(m.band(), Some(second), "past the last row, the next band");
    m.enter();
    assert!(m.groups[1].collapsed, "Enter folds it");
    assert_eq!(m.band(), Some(second), "and the selection stays on it");
    m.enter();
    assert!(!m.groups[1].collapsed, "and unfolds it");
    // A filter that hides the band's group lets the band go.
    m.set_filter("zzzz");
    m.set_filter("");
    assert_eq!(m.band(), None);
    // Leaving the list leaves its band.
    m.select_by(-99);
    assert!(m.band().is_some());
    m.set_view(View::Activity);
    assert_eq!(m.band(), None);
}

/// On the board, j runs down a column and on into the next, k back up into the previous, and
/// h and l move between columns.
#[test]
fn on_the_board_j_and_k_cross_columns_and_h_and_l_move_between_them() {
    let (_dir, mut m) = mixed();
    m.set_layout(Layout::Board);
    let cols: Vec<Vec<String>> = m
        .columns()
        .into_iter()
        .map(|(_, rows)| rows.iter().map(|r| r.key.clone()).collect::<Vec<_>>())
        .filter(|c| !c.is_empty())
        .collect();
    assert!(cols.len() >= 2 && cols[0].len() >= 2, "{cols:?}");
    let at = |m: &Model| m.selected().map(|r| r.key.clone()).unwrap();
    for _ in 0..10 {
        m.select_by(-1);
    }
    assert_eq!(at(&m), cols[0][0], "k stops at the first card");
    for _ in 1..cols[0].len() {
        m.select_by(1);
    }
    assert_eq!(at(&m), *cols[0].last().unwrap());
    m.select_by(1);
    assert_eq!(at(&m), cols[1][0], "on into the next column");
    m.select_by(-1);
    assert_eq!(
        at(&m),
        *cols[0].last().unwrap(),
        "back up into the previous"
    );
    m.select_column_by(1);
    let i = (cols[0].len() - 1).min(cols[1].len() - 1);
    assert_eq!(
        at(&m),
        cols[1][i],
        "l: the next column, as far down as it goes"
    );
    m.select_column_by(-1);
    assert_eq!(at(&m), cols[0][i], "h: back");
    // A card of a group folded in the list is selected on the board, and acted on.
    m.set_layout(Layout::List);
    let first = m.groups[0].key;
    m.toggle_collapsed(first);
    m.set_layout(Layout::Board);
    for _ in 0..10 {
        m.select_by(-1);
    }
    assert_eq!(at(&m), cols[0][0]);
    assert_eq!(m.acted_on().map(|r| r.key), Some(cols[0][0].clone()));
}

/// A planned parent with a planned sub-task and a backlog one, and a planned task after them.
fn family() -> (tempfile::TempDir, Model) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    let parent = planned(&mut ws, "parent");
    let other = planned(&mut ws, "other");
    let child = planned(&mut ws, "child planned");
    let elsewhere = create(&mut ws, Kind::Task, "child in backlog");
    for c in [&child, &elsewhere] {
        ws.execute(
            Command::Link {
                src: c.clone(),
                rel: Rel::Parent,
                dst: parent.clone(),
            },
            &CLAUDE,
        )
        .unwrap();
    }
    let _ = other;
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Tasks);
    (dir, m)
}

/// A sub-task sits right under its parent when both are in the same group; one in another
/// group stays in its own, named by its parent; folding a parent hides its sub-tasks; nesting
/// can be turned off.
#[test]
fn sub_tasks_nest_under_their_parent_within_a_group() {
    let (_dir, mut m) = family();
    let parent = m.rows.iter().find(|r| r.title == "parent").unwrap().clone();
    let child = m
        .rows
        .iter()
        .find(|r| r.title == "child planned")
        .unwrap()
        .clone();
    let elsewhere = m
        .rows
        .iter()
        .find(|r| r.title == "child in backlog")
        .unwrap()
        .clone();
    let at = |m: &Model, id| m.rows.iter().position(|r| r.id == id).unwrap();
    assert_eq!(
        at(&m, child.id),
        at(&m, parent.id) + 1,
        "right under its parent"
    );
    assert_eq!(m.depth.get(&child.id), Some(&1));
    assert_eq!(m.parent_key(&child), None, "nested: no chip");
    assert_eq!(
        m.depth.get(&elsewhere.id),
        None,
        "another group: not nested"
    );
    assert_eq!(
        m.parent_key(&elsewhere),
        Some(parent.key.as_str()),
        "but named"
    );
    assert_eq!(m.progress.get(&parent.id), Some(&(0, 2)));

    m.toggle_folded(parent.id);
    assert!(!m.rows.iter().any(|r| r.id == child.id), "folded away");
    assert!(m.is_folded(parent.id));
    m.toggle_folded(parent.id);
    assert!(m.rows.iter().any(|r| r.id == child.id));

    // A filter that shows the sub-task but not its parent shows it on its own, named.
    m.set_filter("child planned");
    assert_eq!(keys(&m), std::slice::from_ref(&child.key));
    assert_eq!(m.parent_key(&child), Some(parent.key.as_str()));
    m.set_filter("");

    m.toggle_nest();
    assert_eq!(m.depth.get(&child.id), None, "flat");
    assert_eq!(m.parent_key(&child), Some(parent.key.as_str()));
}
