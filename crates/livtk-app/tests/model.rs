//! The app's behaviour, without a window: what the human sees and what their actions do.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::path::Path;

use livtk_app::model::{GroupKey, Grouping, Layout, Model, Ordering, View, choices};
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
    assert!(m.notice.is_none(), "{:?}", m.notice);
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.always_rules().unwrap()[0].key, l);
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
    assert!(m.notice.as_deref().unwrap_or_default().contains("empty"));
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
        m.notice
            .as_deref()
            .unwrap_or_default()
            .contains("waits on LIV-1"),
        "{:?}",
        m.notice
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
    assert!(m.notice.is_some(), "the bad folder is reported");

    m.set_view(View::Lessons);
    assert_eq!(keys(&m), ["LES-1"]);
    m.select_project(1);
    assert_eq!(m.view, View::Lessons);
    assert!(m.rows.is_empty());
    m.set_view(View::Tasks);
    assert_eq!(keys(&m), ["OTH-1"]);
    m.toggle_layout();
    assert_eq!(m.layout, Layout::Board);
    let cols: Vec<(State, usize)> = m.columns().into_iter().map(|(s, r)| (s, r.len())).collect();
    assert_eq!(cols[1], (State::Planned, 1));
    assert_eq!(
        cols.iter().map(|c| c.0).collect::<Vec<_>>(),
        [
            State::Backlog,
            State::Planned,
            State::Acked,
            State::Doing,
            State::Review,
            State::Parked,
            State::Done
        ]
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

#[test]
fn groups_follow_state_order_and_rows_follow_priority() {
    let (_dir, m) = mixed();
    let groups: Vec<(GroupKey, usize)> = m.groups.iter().map(|g| (g.key, g.count)).collect();
    assert_eq!(
        groups,
        [
            (GroupKey::State(State::Planned), 2),
            (GroupKey::State(State::Backlog), 2)
        ]
    );
    assert_eq!(
        keys(&m),
        ["LIV-3", "LIV-2", "LIV-1", "LIV-4"],
        "P2 before P3, P1 before P4"
    );
}

#[test]
fn a_filter_narrows_by_title_or_key_and_priority_chips_combine() {
    let (_dir, mut m) = mixed();
    m.set_filter("PARSER");
    assert_eq!(keys(&m), ["LIV-2", "LIV-4"]);
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
    assert_eq!(m.groups[0].count, 2, "still counted");
    assert!(m.groups[0].collapsed);
    assert_eq!(
        m.selected().map(|r| r.key.as_str()),
        Some("LIV-1"),
        "the selection leaves the hidden rows"
    );
    m.select_by(5);
    assert_eq!(m.selected().map(|r| r.key.as_str()), Some("LIV-4"));
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
        m.notice
            .as_deref()
            .unwrap_or_default()
            .contains("can move to"),
        "{:?}",
        m.notice
    );

    m.drop_on(liv1, GroupKey::State(State::Planned));
    assert!(m.notice.is_none());
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
