//! The activity feed without a window: what it shows, what is new, and acting from it.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::path::Path;

use jiff::civil::date;
use livtk_app::model::{FeedRow, Model, View, created_as, day_label, describe};
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
fn a_key_renumbered_by_a_merge_shows_the_key_it_was_created_with() {
    let (ours, theirs) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut ws = Workspace::init(ours.path(), Some("LIV")).unwrap();
    create(&mut ws, "ours");
    drop(ws);
    let mut other = Workspace::init(theirs.path(), Some("LIV")).unwrap();
    create(&mut other, "theirs");
    drop(other);
    // A merge of two branches that both created LIV-1: both lines kept, theirs the later.
    let log = |d: &Path| d.join(".livtk/events.jsonl");
    let mut merged = std::fs::read_to_string(log(ours.path())).unwrap();
    merged.push_str(&std::fs::read_to_string(log(theirs.path())).unwrap());
    std::fs::write(log(ours.path()), merged).unwrap();

    let mut m = Model::open(&[ours.path().to_path_buf()]);
    m.set_models_dir(None);
    m.set_view(View::Activity);
    let theirs = m
        .activity
        .iter()
        .find(|a| a.node.as_ref().is_some_and(|n| n.title == "theirs"))
        .unwrap();
    assert_eq!(theirs.node.as_ref().unwrap().key, "LIV-2", "renumbered");
    assert_eq!(
        describe(theirs),
        "created as LIV-1",
        "the feed says what it was"
    );
    assert_eq!(
        created_as(&theirs.event, "LIV-2"),
        Some("LIV-1"),
        "and so does the panel"
    );
    let ours = m
        .activity
        .iter()
        .find(|a| a.node.as_ref().is_some_and(|n| n.title == "ours"))
        .unwrap();
    assert_eq!(
        describe(ours),
        "created",
        "a key that stayed says nothing more"
    );
    assert_eq!(created_as(&ours.event, "LIV-1"), None);
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

#[test]
fn each_event_reads_as_a_few_words() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    let a = create(&mut ws, "first");
    let b = create(&mut ws, "second");
    plan(&mut ws, &a);
    ws.execute(
        Command::SetPriority {
            node: a.clone(),
            priority: 1,
        },
        &CLAUDE,
    )
    .unwrap();
    ws.execute(
        Command::Link {
            src: b.clone(),
            rel: livtk_core::Rel::DependsOn,
            dst: a.clone(),
        },
        &CLAUDE,
    )
    .unwrap();
    ws.execute(
        Command::Note {
            node: a,
            text: "winit panics on resize\nmore below".into(),
        },
        &CLAUDE,
    )
    .unwrap();
    let words: Vec<String> = ws
        .activity(None, 10)
        .unwrap()
        .iter()
        .map(describe)
        .collect();
    assert_eq!(
        words,
        [
            "noted “winit panics on resize…”",
            "linked: depends on LIV-1",
            "set priority to urgent",
            "moved to Planned",
            "created",
            "created",
        ]
    );
}

#[test]
fn a_notes_line_breaks_are_kept_and_its_code_left_alone() {
    use livtk_app::model::note_markdown;
    assert_eq!(note_markdown("a quick\njot"), "a quick  \njot");
    assert_eq!(
        note_markdown("one\n\ntwo"),
        "one\n\ntwo",
        "a paragraph stays one"
    );
    assert_eq!(
        note_markdown("see\n```rust\nlet a = 1;\nlet b = 2;\n```\ndone"),
        "see  \n```rust\nlet a = 1;\nlet b = 2;\n```  \ndone",
        "lines inside code are code"
    );
}

#[test]
fn tab_indents_the_lines_a_selection_touches_and_shift_tab_outdents_them() {
    use livtk_app::model::reindent;
    let apply = |text: &str, sel: std::ops::Range<usize>, out: bool| {
        let (range, new, after) = reindent(text, sel, out);
        let mut t = text.to_owned();
        t.replace_range(range, &new);
        (t, after)
    };
    // The cursor's line, wherever the cursor is in it.
    assert_eq!(apply("- a\n- b", 6..6, false), ("- a\n  - b".into(), 8..8));
    assert_eq!(apply("- a\n  - b", 8..8, true), ("- a\n- b".into(), 6..6));
    // Every line selected, and the selection kept over them.
    let (t, after) = apply("x\ny\nz", 2..5, false);
    assert_eq!(t, "x\n  y\n  z");
    assert_eq!(&t[after], "y\n  z");
    // Outdenting takes up to the indent, never text.
    assert_eq!(apply(" a", 0..0, true).0, "a");
    assert_eq!(apply("a", 0..0, true).0, "a");
    // A list item goes under the text of the item above: a sub-item, whatever its marker.
    assert_eq!(apply("1. a\n1. b", 7..7, false).0, "1. a\n   1. b");
    assert_eq!(apply("10. a\n- b", 8..8, false).0, "10. a\n    - b");
    assert_eq!(
        apply("- a\n  - b\n  - c", 13..13, false).0,
        "- a\n  - b\n    - c",
        "one level deeper each time"
    );
    assert_eq!(
        apply("- a\n  - b\n    - c", 15..15, true).0,
        "- a\n  - b\n  - c",
        "and back one level"
    );
}

mod grid {
    use jiff::civil::{Date, date};
    use livtk_app::model::contour::Pick;
    use livtk_app::model::grid::{Grid, What, Who};
    use livtk_core::DayEvents;

    fn row(day: &str, human: bool, op: &str, state: Option<&str>, count: u64) -> DayEvents {
        DayEvents {
            day: day.into(),
            human,
            op: op.into(),
            state: state.map(Into::into),
            count,
        }
    }

    fn grid() -> Grid {
        let mut g = Grid::default();
        g.set_days(&[
            row("2026-10-14", false, "node.create", None, 2),
            row("2026-10-15", true, "node.set", Some("acked"), 1),
            row("2026-10-15", false, "node.note", None, 3),
            row("2026-10-16", false, "node.set", Some("done"), 8),
            row("2026-10-16", true, "node.set", Some("ratified"), 2),
            row("2025-03-01", false, "node.create", None, 1),
        ]);
        g
    }

    const TODAY: Date = date(2026, 10, 16);

    fn day(g: &Grid, d: Date) -> (u64, u8) {
        g.drawn(TODAY)
            .weeks
            .iter()
            .flatten()
            .find(|c| c.day == d && !c.future)
            .map(|c| (c.count, c.level))
            .unwrap_or((0, 0))
    }

    #[test]
    fn days_count_by_who_and_what_in_whole_weeks_from_monday() {
        let mut g = grid();
        let drawn = g.drawn(TODAY);
        assert_eq!(
            drawn.weeks[0][0].day,
            date(2025, 10, 13),
            "the Monday a year back"
        );
        let last = drawn.weeks.last().unwrap();
        assert!(
            last.iter().all(|c| c.future == (c.day > TODAY)),
            "the rest of this week is not yet"
        );
        // Wednesday 2026-10-14 sits in row 2 (Monday is row 0).
        assert_eq!(drawn.at(date(2026, 10, 14)).map(|(_, r)| r), Some(2));
        assert_eq!(day(&g, date(2026, 10, 15)).0, 4, "everyone's");
        g.who = Who::Human;
        assert_eq!(day(&g, date(2026, 10, 15)).0, 1, "the human's only");
        g.who = Who::All;
        g.what = What::Decisions;
        assert_eq!(day(&g, date(2026, 10, 16)).0, 2, "the ratification");
        g.what = What::Done;
        assert_eq!(day(&g, date(2026, 10, 16)).0, 8);
    }

    #[test]
    fn levels_follow_the_ranges_own_busy_days() {
        let g = grid();
        assert_eq!(day(&g, date(2026, 10, 13)).1, 0, "nothing");
        assert!(day(&g, date(2026, 10, 14)).1 < day(&g, date(2026, 10, 16)).1);
        assert_eq!(day(&g, date(2026, 10, 16)).1, 5, "the busiest");
        assert_eq!(day(&g, date(2025, 3, 1)), (0, 0), "older than a year");
    }

    #[test]
    fn months_run_whole_and_name_their_first_whole_week() {
        let drawn = grid().drawn(TODAY);
        let nov = drawn
            .months
            .iter()
            .find(|m| m.first == date(2025, 11, 1))
            .unwrap();
        assert_eq!(nov.last, date(2025, 11, 30));
        // Saturday 1 Nov is in week 2; its first whole week is week 3.
        assert_eq!(nov.label_week, 3);
        assert_eq!(
            drawn.months[0].label_week, 0,
            "the first month however short"
        );
    }

    #[test]
    fn a_click_anchors_a_second_closes_and_a_third_starts_again() {
        let mut p = Pick::default();
        let picked = |p: &Pick, a: Date, b: Date, anchored| {
            assert_eq!(
                *p,
                Pick {
                    span: Some((a, b)),
                    anchored
                }
            )
        };
        p.click(date(2026, 10, 16));
        picked(&p, date(2026, 10, 16), date(2026, 10, 16), true);
        p.click(date(2026, 10, 14));
        picked(&p, date(2026, 10, 14), date(2026, 10, 16), false);
        p.click(date(2026, 10, 1));
        picked(&p, date(2026, 10, 1), date(2026, 10, 1), true);
        p.take(date(2026, 9, 1), date(2026, 9, 30));
        picked(&p, date(2026, 9, 1), date(2026, 9, 30), false);
        assert!(p.clear());
        assert_eq!(p, Pick::default());
        assert!(!p.clear(), "nothing was");
    }
}

/// The grid counts in any view, as the bottom bar shows it in any; a day picked outside the
/// Activity view filters the feed once it is opened.
#[test]
fn the_grid_counts_in_any_view_and_a_pick_waits_for_the_feed() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    ws.execute(
        Command::Create {
            kind: Kind::Task,
            title: "today's".into(),
            body: String::new(),
            priority: None,
            links: vec![],
        },
        &CLAUDE,
    )
    .unwrap();
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Tasks);
    let today = livtk_app::model::grid::today();
    let drawn = m.grid.drawn(today);
    let cell = drawn.weeks.iter().flatten().find(|c| c.day == today);
    assert!(
        cell.unwrap().count >= 1,
        "counted outside the Activity view"
    );
    m.grid_pick(today.yesterday().unwrap());
    m.set_view(View::Activity);
    assert!(m.activity.is_empty(), "the feed opens on the day picked");
}

/// A day picked in the grid shows that day's events only; unpicked, all of them again.
#[test]
fn a_day_picked_in_the_grid_filters_the_feed() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    ws.execute(
        Command::Create {
            kind: Kind::Task,
            title: "today's".into(),
            body: String::new(),
            priority: None,
            links: vec![],
        },
        &CLAUDE,
    )
    .unwrap();
    drop(ws);
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Activity);
    let today = livtk_app::model::grid::today();
    assert!(!m.activity.is_empty());
    m.grid_pick(today.yesterday().unwrap());
    assert!(m.activity.is_empty(), "nothing happened yesterday");
    m.grid_pick(today);
    assert_eq!(m.activity.len(), 1, "yesterday to today");
    m.grid_unpick();
    assert_eq!(m.activity.len(), 1);
    let drawn = m.grid.drawn(today);
    let cell = drawn.weeks.iter().flatten().find(|c| c.day == today);
    assert!(cell.unwrap().count >= 1, "today counted");
}
