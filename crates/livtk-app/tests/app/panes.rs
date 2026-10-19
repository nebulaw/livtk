//! Panes without a window: splitting, closing and focusing them, a view in each with its own
//! state, each view in one pane at most, and every pane kept up to date.

use std::path::Path;

use livtk_app::model::{Axis, Model, Split, View};
use livtk_core::{Actor, Command, Kind, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

/// A project with three tasks and a finding.
fn project(dir: &Path) {
    let mut ws = Workspace::init(dir, Some("LIV")).unwrap();
    for (kind, title) in [
        (Kind::Task, "One"),
        (Kind::Task, "Two"),
        (Kind::Task, "Three"),
        (Kind::Finding, "Found"),
    ] {
        ws.execute(
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
    }
}

fn open(dir: &Path) -> Model {
    let mut m = Model::open(&[dir.to_path_buf()]);
    m.set_models_dir(None);
    m
}

fn views(m: &Model) -> Vec<Option<View>> {
    m.split
        .ids()
        .iter()
        .map(|id| {
            let p = &m.panes()[m.pane_index(*id).unwrap()];
            (!p.placeholder).then_some(p.view)
        })
        .collect()
}

#[test]
fn a_split_makes_a_focused_placeholder_beside_and_closing_gives_its_room_back() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut m = open(dir.path());
    m.set_view(View::Terminal);
    m.split_pane(Axis::Row);
    assert_eq!(views(&m), [Some(View::Terminal), None]);
    assert!(m.panes()[m.focused()].placeholder, "the new one, focused");
    // Its view chosen, the placeholder is a pane like any other.
    m.set_view(View::Tasks);
    assert_eq!(views(&m), [Some(View::Terminal), Some(View::Tasks)]);
    assert_eq!(m.rows().len(), 3);
    // Split again the same way: a third beside, the shares halved, not nested.
    m.split_pane(Axis::Row);
    let Split::Split { axis, parts } = &m.split else {
        panic!("a split")
    };
    assert_eq!(*axis, Axis::Row);
    let shares: Vec<f32> = parts.iter().map(|(_, f)| *f).collect();
    assert_eq!(shares, [0.5, 0.25, 0.25]);
    // Closed: the room goes back to the rest, the one before it focused.
    m.close_pane();
    assert_eq!(views(&m), [Some(View::Terminal), Some(View::Tasks)]);
    assert_eq!(m.view(), View::Tasks);
    let Split::Split { parts, .. } = &m.split else {
        panic!("a split")
    };
    let total: f32 = parts.iter().map(|(_, f)| *f).sum();
    assert!((total - 1.).abs() < 1e-4, "{total}");
    // Down, then back to one: the last pane never closes.
    m.split_pane(Axis::Column);
    assert!(
        matches!(&m.split, Split::Split { parts, .. } if matches!(&parts[1].0, Split::Split { axis: Axis::Column, .. }))
    );
    m.close_pane();
    m.close_pane();
    m.close_pane();
    assert_eq!(m.split, Split::Pane(m.panes()[0].id));
    assert_eq!(m.panes().len(), 1);
}

#[test]
fn each_pane_keeps_its_own_selection_and_a_view_shows_in_one_pane_at_most() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut m = open(dir.path());
    m.set_view(View::Tasks);
    m.select_by(2);
    let task = m.selected().unwrap().key.clone();
    m.split_pane(Axis::Row);
    m.set_view(View::Findings);
    assert_eq!(m.selected().unwrap().title, "Found");
    // Back to the tasks' pane: its selection is where it was.
    m.focus_next_pane(1);
    assert_eq!(m.view(), View::Tasks);
    assert_eq!(m.selected().unwrap().key, task);
    // Asking for the findings here focuses their pane instead of showing them twice.
    m.set_view(View::Findings);
    assert_eq!(m.view(), View::Findings);
    assert_eq!(views(&m), [Some(View::Tasks), Some(View::Findings)]);
    // Acting on another pane for a while leaves the focus where it is.
    let shown = m.with_pane(0, |m| m.rows().len());
    assert_eq!(shown, 3);
    assert_eq!(m.view(), View::Findings);
}

#[test]
fn a_change_from_elsewhere_reaches_every_pane() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut m = open(dir.path());
    m.set_view(View::Tasks);
    m.split_pane(Axis::Row);
    m.set_view(View::Findings);
    let mut ws = Workspace::open(dir.path()).unwrap();
    ws.execute(
        Command::Create {
            kind: Kind::Task,
            title: "Four".into(),
            body: String::new(),
            priority: None,
            links: vec![],
        },
        &CLAUDE,
    )
    .unwrap();
    drop(ws);
    assert!(m.tick());
    assert_eq!(
        m.with_pane(0, |m| m.rows().len()),
        4,
        "the tasks' pane, unfocused"
    );
}

#[test]
fn the_panes_come_back_after_a_restart_as_they_were_left() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut m = open(dir.path());
    m.set_view(View::Terminal);
    m.split_pane(Axis::Row);
    m.set_view(View::Tasks);
    m.toggle_layout();
    m.split_pane(Axis::Column);
    let file = tempfile::tempdir().unwrap();
    let path = file.path().join("livtk/app.toml");
    m.saved().save(&path).unwrap();
    let mut back = open(dir.path());
    back.restore(&livtk_app::model::Saved::load(&path));
    assert_eq!(back.split, m.split, "laid out the same");
    assert_eq!(
        views(&back),
        [Some(View::Terminal), Some(View::Tasks), None]
    );
    assert!(back.placeholder(), "the placeholder, focused as it was");
    back.focus_next_pane(-1);
    assert_eq!(back.view(), View::Tasks);
    assert_eq!(back.layout(), livtk_app::model::Layout::Board);
    assert_eq!(back.rows().len(), 3, "and read");
    // A new pane takes an id none has.
    back.split_pane(Axis::Row);
    let mut ids = back.split.ids();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 4);
}

#[test]
fn a_view_in_none_of_the_splits_panes_shows_alone_and_the_split_comes_back() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut m = open(dir.path());
    m.set_view(View::Terminal);
    m.split_pane(Axis::Row);
    m.set_view(View::Tasks);
    let split = m.split.clone();
    // The docs are in neither pane: shown alone, the split kept as it was.
    m.set_view(View::Docs);
    assert!(m.solo());
    assert_eq!(m.view(), View::Docs);
    assert_eq!(m.shown_split().ids().len(), 1);
    assert_eq!(m.split, split, "the split untouched");
    assert_eq!(views(&m), [Some(View::Terminal), Some(View::Tasks)]);
    // Another view not in it replaces the one alone; one in it brings the split back, on it.
    m.set_view(View::Findings);
    assert!(m.solo());
    assert_eq!(m.view(), View::Findings);
    m.set_view(View::Terminal);
    assert!(!m.solo());
    assert_eq!(m.view(), View::Terminal);
    assert_eq!(m.shown_split(), split);
    // The next pane's key, or closing, from a view alone: the split again, where it was left.
    m.set_view(View::Graph);
    m.focus_next_pane(1);
    assert!(!m.solo());
    assert_eq!(m.view(), View::Terminal);
    m.set_view(View::Graph);
    m.close_pane();
    assert!(!m.solo());
    assert_eq!(m.panes().len(), 2);
    // Splitting a view shown alone takes it into the split, beside where it was left.
    m.set_view(View::Docs);
    m.split_pane(Axis::Row);
    assert!(!m.solo());
    assert_eq!(
        views(&m),
        [Some(View::Terminal), Some(View::Docs), Some(View::Tasks)]
    );
    assert_eq!(m.view(), View::Docs);
    // A placeholder still takes the view asked for.
    m.split_pane(Axis::Column);
    m.set_view(View::Lessons);
    assert!(!m.solo());
    assert_eq!(m.view(), View::Lessons);
}

#[test]
fn unsplitting_a_pane_keeps_the_focus_and_a_view_alone_where_they_are() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut m = open(dir.path());
    m.set_view(View::Terminal);
    m.split_pane(Axis::Row);
    m.set_view(View::Tasks);
    m.split_pane(Axis::Row);
    m.set_view(View::Findings);
    let tasks = m.split.ids()[1];
    assert_eq!(m.split_views().len(), 3);
    // Out goes the tasks' pane; the findings keep the keys.
    m.unsplit(tasks);
    assert_eq!(views(&m), [Some(View::Terminal), Some(View::Findings)]);
    assert_eq!(m.view(), View::Findings);
    // Unsplit to one pane under a view shown alone: that view is the one view.
    m.set_view(View::Docs);
    assert!(m.solo());
    let findings = m.split.ids()[1];
    m.unsplit(findings);
    assert!(!m.solo());
    assert_eq!(m.view(), View::Docs);
    assert!(m.split_views().is_empty(), "one pane: no split to group");
    assert_eq!(m.panes().len(), 1);
}
