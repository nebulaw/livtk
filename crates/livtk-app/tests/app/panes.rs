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
