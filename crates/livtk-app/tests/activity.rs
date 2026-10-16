//! The activity feed without a window: what it shows, what is new, and acting from it.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::path::Path;

use jiff::civil::date;
use livtk_app::model::{FeedRow, Model, View, day_label};
use livtk_core::event::Op;
use livtk_core::{Actor, Command, Kind, State, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

fn create(ws: &mut Workspace, title: &str) -> String {
    let e = ws
        .execute(
            Command::Create {
                kind: Kind::Task,
                title: title.into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &CLAUDE,
        )
        .unwrap();
    ws.node(&e[0].op.subjects().0.to_string()).unwrap().key
}

fn plan(ws: &mut Workspace, key: &str) {
    ws.execute(
        Command::Move {
            node: key.into(),
            to: State::Planned,
        },
        &CLAUDE,
    )
    .unwrap();
}

/// Claude created two tasks and planned the first: three events.
fn app(dir: &Path) -> Model {
    let mut ws = Workspace::init(dir, Some("LIV")).unwrap();
    let a = create(&mut ws, "first");
    create(&mut ws, "second");
    plan(&mut ws, &a);
    drop(ws);
    let mut m = Model::open(&[dir.to_path_buf()]);
    m.set_models_dir(None);
    m
}

#[test]
fn days_are_headed_as_linear_heads_them() {
    let today = date(2026, 10, 16);
    assert_eq!(day_label(today, today), "Today");
    assert_eq!(day_label(date(2026, 10, 15), today), "Yesterday");
    assert_eq!(day_label(date(2026, 10, 12), today), "Monday");
    assert_eq!(day_label(date(2026, 9, 3), today), "September 3");
    assert_eq!(day_label(date(2025, 12, 31), today), "December 31, 2025");
}

#[test]
fn the_feed_shows_every_event_newest_first_under_its_day() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    assert_eq!(m.projects[0].unseen, 3, "the sidebar counts Claude's work");
    m.set_view(View::Activity);
    assert_eq!(m.activity.len(), 3);
    assert!(
        matches!(m.activity[0].event.op, Op::NodeSet { .. }),
        "the move is newest"
    );
    assert_eq!(m.activity[0].node.as_ref().unwrap().key, "LIV-1");
    assert_eq!(
        m.feed(),
        [
            FeedRow::Day("Today".into()),
            FeedRow::Event(0),
            FeedRow::Event(1),
            FeedRow::Event(2)
        ]
    );
    assert!(m.rows.is_empty(), "the feed lists events, not rows");
}

#[test]
fn what_is_new_stays_marked_for_the_visit_and_the_count_clears() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.set_view(View::Activity);
    assert!(
        m.activity.iter().all(|a| m.is_new(a)),
        "all three are new on this visit"
    );
    assert_eq!(m.projects[0].unseen, 0, "looking at them clears the count");

    m.set_view(View::Tasks);
    m.set_view(View::Activity);
    assert!(
        m.activity.iter().all(|a| !m.is_new(a)),
        "the next visit has nothing new"
    );

    // Claude works while the human is elsewhere.
    m.set_view(View::Tasks);
    let mut ws = Workspace::open(dir.path()).unwrap();
    create(&mut ws, "third");
    assert!(m.tick());
    assert_eq!(m.projects[0].unseen, 1);
    m.set_view(View::Activity);
    let new: Vec<bool> = m.activity.iter().map(|a| m.is_new(a)).collect();
    assert_eq!(new, [true, false, false, false]);
}

#[test]
fn work_arriving_while_the_feed_is_open_lands_on_top_and_flashes() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.set_view(View::Activity);
    m.select_by(2);
    assert_eq!(m.activity_index, 2);
    let highlighted = m.activity[2].event.id;

    let mut ws = Workspace::open(dir.path()).unwrap();
    create(&mut ws, "from claude, live");
    assert!(m.tick());
    assert_eq!(m.activity.len(), 4);
    assert_eq!(
        m.activity[0].node.as_ref().unwrap().title,
        "from claude, live"
    );
    assert!(m.flashed.contains_key(&m.activity[0].event.id));
    assert_eq!(
        m.activity[m.activity_index].event.id, highlighted,
        "the highlight stays on its event"
    );
    assert_eq!(m.projects[0].unseen, 0, "seen as it arrives");
}

#[test]
fn enter_opens_the_node_in_place_and_its_actions_act_on_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.set_view(View::Activity);
    m.open_detail();
    assert_eq!(m.view, View::Activity, "the feed stays");
    assert_eq!(m.detail.as_ref().unwrap().node.key, "LIV-1");

    m.decide();
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(
        ws.node("LIV-1").unwrap().state,
        State::Acked,
        "acknowledged from the feed"
    );
    assert_eq!(
        m.activity[0].event.by, "human",
        "the human's own event lands on top"
    );
    assert!(!m.is_new(&m.activity[0]), "and is never news");
    assert_eq!(
        m.detail.as_ref().unwrap().node.state,
        State::Acked,
        "the panel follows"
    );

    assert_eq!(
        m.activity_index, 1,
        "the highlight stayed on its event below the new one"
    );
    m.select_by(1);
    assert_eq!(
        m.detail.as_ref().unwrap().node.key,
        "LIV-2",
        "moving on opens the next event's node: LIV-2 created"
    );
    m.select_by(5);
    assert_eq!(m.activity_index, 3, "stops at the oldest");
    assert_eq!(m.detail.as_ref().unwrap().node.key, "LIV-1");
}

#[test]
fn older_events_come_a_page_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    for i in 0..250 {
        create(&mut ws, &format!("task {i}"));
    }
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Activity);
    assert_eq!(m.activity.len(), 200);
    assert!(m.activity_more);
    m.activity_older();
    assert_eq!(m.activity.len(), 250);
    assert!(!m.activity_more);
    assert_eq!(m.activity[249].node.as_ref().unwrap().title, "task 0");
}

#[test]
fn the_palette_goes_to_the_feed_and_acts_on_the_node_open_there() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.open_palette();
    m.palette_query("go to activ");
    m.palette_run();
    assert_eq!(m.view, View::Activity);
    m.open_detail();
    m.open_palette();
    m.palette_query("acknowledge");
    m.palette_run();
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.node("LIV-1").unwrap().state, State::Acked);
}
