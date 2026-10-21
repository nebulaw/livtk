//! Projects in the app: the view, a project's panel, and tasks grouped by project.

use livtk_app::model::{GroupKey, Grouping, Model, View};
use livtk_core::{Actor, Command, Health, Kind, Rel, State, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };
const HUMAN: Actor = Actor::Human;

fn create(ws: &mut Workspace, kind: Kind, title: &str, parent: Option<&str>) -> String {
    let e = ws
        .execute(
            Command::Create {
                kind,
                title: title.into(),
                body: String::new(),
                priority: None,
                links: parent
                    .map(|p| (Rel::Parent, p.to_owned()))
                    .into_iter()
                    .collect(),
            },
            &CLAUDE,
        )
        .unwrap();
    match &e[0].op {
        livtk_core::event::Op::NodeCreate { data, .. } => data.key.clone(),
        _ => unreachable!(),
    }
}

fn mv(ws: &mut Workspace, key: &str, to: State, by: &Actor) {
    ws.execute(
        Command::Move {
            node: key.into(),
            to,
        },
        by,
    )
    .unwrap();
}

/// PRJ-1 active with three tasks (one done, one planned, one in review) and an update; PRJ-2
/// proposed and empty; LIV-4 part of no project.
pub(crate) fn setup(dir: &std::path::Path) -> Workspace {
    let mut ws = Workspace::init(dir, Some("LIV")).unwrap();
    ws.set_models_dir(None);
    let p = create(&mut ws, Kind::Project, "Import the board", None);
    create(&mut ws, Kind::Project, "Later outcome", None);
    mv(&mut ws, &p, State::Active, &HUMAN);
    ws.execute(
        Command::SetProject {
            node: p.clone(),
            target: Some("2026-12-01".into()),
            owner: Some("karl".into()),
        },
        &CLAUDE,
    )
    .unwrap();
    let a = create(&mut ws, Kind::Task, "parse cards", Some(&p));
    let b = create(&mut ws, Kind::Task, "write tasks", Some(&p));
    let c = create(&mut ws, Kind::Task, "link them", Some(&p));
    create(&mut ws, Kind::Task, "unrelated", None);
    for k in [&a, &c] {
        mv(&mut ws, k, State::Planned, &CLAUDE);
        mv(&mut ws, k, State::Acked, &HUMAN);
        mv(&mut ws, k, State::Doing, &CLAUDE);
        mv(&mut ws, k, State::Review, &CLAUDE);
    }
    mv(&mut ws, &a, State::Done, &HUMAN);
    mv(&mut ws, &b, State::Planned, &CLAUDE);
    ws.execute(
        Command::Update {
            node: p,
            health: Health::AtRisk,
            text: "parsing is slow; next: batch it".into(),
        },
        &CLAUDE,
    )
    .unwrap();
    ws
}

#[test]
fn the_projects_view_lists_active_ones_first_with_their_progress() {
    let dir = tempfile::tempdir().unwrap();
    let _ws = setup(dir.path());
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Projects);
    let keys: Vec<&str> = m.rows().iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, ["PRJ-1", "PRJ-2"], "active before proposed");
    let id = m.rows()[0].id;
    let g = m.project_progress(id).unwrap();
    assert_eq!((g.done, g.total), (1, 3));
    let waiting: Vec<(State, usize)> = g.waiting.iter().map(|w| (w.state, w.count)).collect();
    assert_eq!(waiting, [(State::Planned, 1), (State::Review, 1)]);
    assert_eq!(g.update.as_ref().unwrap().health, Health::AtRisk);
    assert_eq!(g.target.as_deref(), Some("2026-12-01"));
    assert_eq!(m.too_many_active(), None, "one active is fine");
    // The panel: progress, waits, updates; its tasks by state.
    m.select(id);
    m.open_detail();
    let d = m.detail.as_ref().unwrap();
    let (g, updates) = d.project.as_ref().unwrap();
    assert_eq!((g.done, g.total), (1, 3));
    assert_eq!(updates.len(), 1);
    let groups: Vec<(&str, usize)> = d
        .link_groups
        .iter()
        .map(|l| (l.label, l.briefs.len()))
        .collect();
    assert!(groups.contains(&("Planned", 1)), "{groups:?}");
    assert!(groups.contains(&("In review", 1)), "{groups:?}");
    assert!(groups.contains(&("Done", 1)), "{groups:?}");
}

#[test]
fn more_than_three_active_projects_are_said_plainly() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = setup(dir.path());
    for t in ["two", "three", "four"] {
        let k = create(&mut ws, Kind::Project, t, None);
        mv(&mut ws, &k, State::Active, &HUMAN);
    }
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Projects);
    assert_eq!(m.too_many_active(), Some(4));
    m.set_view(View::Tasks);
    assert_eq!(m.too_many_active(), None, "only in the projects view");
}

#[test]
fn tasks_group_by_project_with_the_rest_under_no_project() {
    let dir = tempfile::tempdir().unwrap();
    let _ws = setup(dir.path());
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Tasks);
    m.set_grouping(Grouping::Project);
    let bands: Vec<(GroupKey, usize)> = m.groups().iter().map(|g| (g.key, g.count)).collect();
    // LIV-1 is done (shown, not dismissed); all three of PRJ-1's first, then LIV-4.
    assert_eq!(bands.len(), 2, "{bands:?}");
    assert!(
        matches!(bands[0], (GroupKey::Project(Some(_)), 3)),
        "{bands:?}"
    );
    assert_eq!(bands[1], (GroupKey::Project(None), 1));
    assert_eq!(m.project_names()[0].1.key, "PRJ-1");
}
