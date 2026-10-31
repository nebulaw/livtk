//! Projects in the app: the view, a project's panel, and tasks grouped by project.

use livtk_app::model::{GroupKey, Grouping, Layout, Model, Ordering, View};
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
    let (g, updates, flow) = d.project.as_ref().unwrap();
    assert_eq!((g.done, g.total), (1, 3));
    assert_eq!(updates.len(), 1);
    // Its flow: started, so a lead time; a time for each state its tasks have been in.
    assert!(flow.lead.is_some());
    let states: Vec<State> = flow.states.iter().map(|t| t.state).collect();
    assert_eq!(
        states,
        [
            State::Backlog,
            State::Planned,
            State::Acked,
            State::Doing,
            State::Review
        ]
    );
    assert_eq!(flow.states[4].tasks, 2, "two tasks reached review");
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

/// What each view's list offers, from one description of its kind.
#[test]
fn each_list_offers_what_its_kind_has() {
    use livtk_app::model::making::Making;
    let offers = |v: View| {
        let s = v.listing_spec().unwrap();
        (s.making, s.board, s.priority, s.by_project, s.nests, s.noun)
    };
    assert_eq!(
        offers(View::Tasks),
        (Some(Making::Task), true, true, true, true, "task")
    );
    assert_eq!(
        offers(View::Projects),
        (Some(Making::Project), true, false, false, true, "project")
    );
    for (v, noun) in [
        (View::Lessons, "lesson"),
        (View::Decisions, "decision"),
        (View::Findings, "finding"),
    ] {
        assert_eq!(offers(v), (None, false, false, false, false, noun), "{v:?}");
    }
    for v in [View::Inbox, View::Activity, View::Graph, View::Docs] {
        assert!(v.listing_spec().is_none(), "{v:?} lists no one kind");
    }
    assert!(View::Projects.has_board() && View::Tasks.has_board());
    assert!(!View::Lessons.has_board());
    assert_eq!(
        View::Projects.listing_spec().unwrap().nest_label(),
        "Nest sub-projects"
    );
    assert_eq!(
        View::Tasks.listing_spec().unwrap().nest_label(),
        "Nest sub-tasks"
    );
}

/// An empty list says how to go on in its own words: a project in Projects, a task only in
/// Tasks, and nothing to make where nothing can be made.
#[test]
fn an_empty_list_names_what_it_makes() {
    let text = |v: View| v.listing_spec().unwrap().empty_list_text();
    assert_eq!(
        text(View::Projects),
        "Clear the filter, press S D to show dismissed work, or P for a new project."
    );
    assert_eq!(
        text(View::Tasks),
        "Clear the filter, press S D to show dismissed work, or N for a new task."
    );
    for v in [
        View::Projects,
        View::Lessons,
        View::Decisions,
        View::Findings,
    ] {
        assert!(!text(v).contains("task"), "{v:?}: {}", text(v));
    }
    assert_eq!(
        text(View::Lessons),
        "Clear the filter, or press S D to show dismissed work."
    );
    assert!(
        View::Projects
            .listing_spec()
            .unwrap()
            .empty_board_text()
            .ends_with("or P for a new project."),
    );
}

/// Projects have no priority: not grouped, ordered or filtered by it, nor grouped by project;
/// tasks still are.
#[test]
fn projects_are_not_grouped_or_ordered_by_priority() {
    let dir = tempfile::tempdir().unwrap();
    let _ws = setup(dir.path());
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Projects);
    assert_eq!(
        m.display().ordering,
        Ordering::Updated,
        "the latest first, not by priority"
    );
    for g in [Grouping::Priority, Grouping::Project] {
        m.set_grouping(g);
        assert_eq!(m.display().grouping, Grouping::State, "{g:?} refused");
    }
    m.set_ordering(Ordering::Priority);
    assert_eq!(m.display().ordering, Ordering::Updated, "refused");
    m.set_ordering(Ordering::Created);
    assert_eq!(m.display().ordering, Ordering::Created, "others taken");

    m.set_view(View::Tasks);
    assert_eq!(m.display().ordering, Ordering::Priority);
    for g in [Grouping::Priority, Grouping::Project] {
        m.set_grouping(g);
        assert_eq!(m.display().grouping, g);
    }
}

/// Projects on a board: a column per project state in its own order, and a drop on a column
/// moves the project there.
#[test]
fn the_projects_board_moves_a_project_by_a_drop() {
    let dir = tempfile::tempdir().unwrap();
    let _ws = setup(dir.path());
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Projects);
    m.set_layout(Layout::Board);
    assert_eq!(m.layout(), Layout::Board);
    let columns = |m: &Model| -> Vec<(GroupKey, Vec<String>)> {
        m.columns()
            .into_iter()
            .map(|(k, rows)| (k, rows.iter().map(|r| r.key.clone()).collect()))
            .collect()
    };
    assert_eq!(
        columns(&m),
        [
            (GroupKey::State(State::Active), vec!["PRJ-1".to_owned()]),
            (GroupKey::State(State::Proposed), vec!["PRJ-2".to_owned()]),
        ]
    );
    m.toggle_empty();
    let keys: Vec<GroupKey> = columns(&m).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        keys,
        [
            GroupKey::State(State::Active),
            GroupKey::State(State::Proposed),
            GroupKey::State(State::Paused),
            GroupKey::State(State::Done),
        ],
        "every live state, as the projects' own order has them"
    );
    let id = m.rows().iter().find(|r| r.key == "PRJ-1").unwrap().id;
    assert!(m.can_drop(id, GroupKey::State(State::Paused)));
    m.drop_on(id, GroupKey::State(State::Paused));
    let paused = columns(&m)
        .into_iter()
        .find(|(k, _)| *k == GroupKey::State(State::Paused))
        .unwrap()
        .1;
    assert_eq!(paused, ["PRJ-1"], "the drop paused it");
}

/// Projects nest under the projects they are part of, as sub-tasks under their parent.
#[test]
fn sub_projects_nest_under_their_project() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = setup(dir.path());
    let sub = create(
        &mut ws,
        Kind::Project,
        "A part of the import",
        Some("PRJ-2"),
    );
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_view(View::Projects);
    let keys: Vec<&str> = m.rows().iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, ["PRJ-1", "PRJ-2", sub.as_str()], "under its parent");
    let id = m.rows()[2].id;
    assert_eq!(m.depth().get(&id), Some(&1));
}
