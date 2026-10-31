//! What the app remembers between runs, without a window.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::path::Path;

use livtk_app::model::{GroupKey, Grouping, Layout, Model, Ordering, Saved, SavedWindow, View};
use livtk_core::{Actor, Command, Kind, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

fn project(dir: &Path) {
    let mut ws = Workspace::init(dir, Some("LIV")).unwrap();
    for t in ["one", "two"] {
        ws.execute(
            Command::Create {
                kind: Kind::Task,
                title: t.into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &CLAUDE,
        )
        .unwrap();
    }
}

fn open(dirs: &[&Path]) -> Model {
    let mut m = Model::open(&dirs.iter().map(|d| d.to_path_buf()).collect::<Vec<_>>());
    m.set_models_dir(None);
    m
}

#[test]
fn how_each_view_was_shown_comes_back_after_a_restart() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    project(a.path());
    project(b.path());
    let mut m = open(&[a.path(), b.path()]);
    m.select_project(1);
    m.set_view(View::Tasks);
    m.set_filter("on");
    m.toggle_priority(2);
    m.set_grouping(Grouping::Priority);
    m.set_ordering(Ordering::Created);
    m.toggle_collapsed(GroupKey::Priority(3));
    m.toggle_layout();
    m.toggle_dismissed();
    m.toggle_empty();
    m.set_view(View::Lessons);
    m.set_ordering(Ordering::Updated);

    let file = tempfile::tempdir().unwrap();
    let path = file.path().join("livtk/app.toml");
    let mut saved = m.saved();
    saved.window = SavedWindow {
        sidebar_width: Some(280.),
        sidebar_collapsed: true,
        panel_width: Some(540.),
        ..SavedWindow::default()
    };
    saved.save(&path).unwrap();

    let back = Saved::load(&path);
    assert_eq!(back, saved, "the file holds it all");
    let mut again = open(&[a.path(), b.path()]);
    again.restore(&back);
    assert_eq!(again.current, 1, "the same project");
    assert_eq!(again.view(), View::Lessons, "the same view");
    assert_eq!(again.display().ordering, Ordering::Updated);
    assert_eq!(again.layout(), Layout::Board);
    assert!(
        !again.display().show_dismissed && !again.display().show_empty,
        "each view has its own: the lessons were never changed"
    );
    again.set_view(View::Tasks);
    let d = again.display();
    assert!(d.show_dismissed, "dismissed work, as the tasks had it");
    assert!(d.show_empty, "empty groups too");
    assert_eq!(d.filter, "on");
    assert_eq!(d.priorities, [2]);
    assert_eq!(d.grouping, Grouping::Priority);
    assert_eq!(d.ordering, Ordering::Created);
    assert_eq!(
        again.saved().views["tasks"].collapsed,
        ["priority:3"],
        "and the collapsed group"
    );
    assert_eq!(back.window.panel_width, Some(540.), "the window's part too");
}

#[test]
fn a_missing_or_broken_file_starts_the_app_as_new() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(Saved::load(&dir.path().join("none.toml")), Saved::default());
    let broken = dir.path().join("app.toml");
    std::fs::write(&broken, "view = [not toml").unwrap();
    assert_eq!(Saved::load(&broken), Saved::default());
}

#[test]
fn what_no_longer_applies_is_passed_over() {
    let a = tempfile::tempdir().unwrap();
    project(a.path());
    let saved: Saved = toml::from_str(
        r#"
        project = "/nowhere/gone"
        view = "a view that went"
        layout = "board"
        [views.tasks]
        priorities = [2, 9]
        grouping = "sideways"
        collapsed = ["state:doing", "state:nonsense", "all"]
        "#,
    )
    .unwrap();
    let mut m = open(&[a.path()]);
    m.restore(&saved);
    assert_eq!(m.current, 0, "the project that is there");
    assert_eq!(m.view(), View::Inbox, "the view stays");
    m.set_view(View::Tasks);
    let d = m.display();
    assert_eq!(d.priorities, [2], "a priority that cannot be is dropped");
    assert_eq!(
        d.grouping,
        Grouping::State,
        "an unknown grouping keeps the default"
    );
    let mut collapsed = m.saved().views["tasks"].collapsed.clone();
    collapsed.sort();
    assert_eq!(collapsed, ["all", "state:doing"]);
}

#[test]
fn dismissed_work_shown_everywhere_by_an_older_app_is_shown_in_each_view() {
    let a = tempfile::tempdir().unwrap();
    project(a.path());
    let saved: Saved = toml::from_str("show_all = true\n").unwrap();
    let mut m = open(&[a.path()]);
    m.restore(&saved);
    for view in View::ALL {
        m.set_view(view);
        assert_eq!(
            m.display().show_dismissed,
            view.shows_dismissed(),
            "{view:?}"
        );
    }
    let text = toml::to_string(&m.saved()).unwrap();
    assert!(
        !text.contains("show_all"),
        "the old setting is not written again: {text}"
    );
}

#[test]
fn the_states_a_view_was_filtered_to_come_back() {
    let a = tempfile::tempdir().unwrap();
    project(a.path());
    let mut m = open(&[a.path()]);
    m.set_view(View::Tasks);
    m.toggle_state(livtk_core::State::Review);
    m.toggle_state(livtk_core::State::Planned);
    let saved = m.saved();
    assert_eq!(saved.views["tasks"].states, ["planned", "review"]);

    let mut again = open(&[a.path()]);
    again.restore(&saved);
    again.set_view(View::Tasks);
    assert_eq!(
        again.display().states,
        [livtk_core::State::Planned, livtk_core::State::Review]
    );

    // Names that are no state of the kind, or dismissed ones while those are hidden, are dropped.
    let odd: Saved = toml::from_str(
        "[views.tasks]\nstates = [\"doing\", \"ratified\", \"nonsense\", \"canceled\"]\n",
    )
    .unwrap();
    let mut m = open(&[a.path()]);
    m.restore(&odd);
    m.set_view(View::Tasks);
    assert_eq!(m.display().states, [livtk_core::State::Doing]);
}

/// The top bar's menu offers the open project first, then the rest most recently opened first;
/// that order comes back after a restart.
#[test]
fn recent_projects_come_latest_first_and_are_kept() {
    let dirs: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
    for d in &dirs {
        project(d.path());
    }
    let paths: Vec<&Path> = dirs.iter().map(|d| d.path()).collect();
    let mut m = open(&paths);
    assert_eq!(
        m.recent_projects(),
        [0, 1, 2],
        "never opened: as registered"
    );
    m.select_project(2);
    m.select_project(1);
    assert_eq!(
        m.recent_projects(),
        [1, 2, 0],
        "the open one, then the latest"
    );

    let file = tempfile::tempdir().unwrap();
    let path = file.path().join("livtk/app.toml");
    m.saved().save(&path).unwrap();
    let mut back = open(&paths);
    back.restore(&Saved::load(&path));
    assert_eq!(back.current, 1);
    assert_eq!(back.recent_projects(), [1, 2, 0]);
}

/// How docs read comes back after a restart.
#[test]
fn the_reading_settings_are_kept() {
    let a = tempfile::tempdir().unwrap();
    project(a.path());
    let mut m = open(&[a.path()]);
    m.reading = livtk_app::model::Reading {
        size: livtk_app::model::ReadingSize::Small,
        compact: true,
    };
    let file = tempfile::tempdir().unwrap();
    let path = file.path().join("livtk/app.toml");
    m.saved().save(&path).unwrap();
    let mut back = open(&[a.path()]);
    back.restore(&Saved::load(&path));
    assert_eq!(back.reading, m.reading);
}

#[test]
fn the_way_a_detail_opens_comes_back_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    livtk_core::Workspace::init(dir.path(), Some("LIV")).unwrap();
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.detail_mode = livtk_app::model::DetailMode::Center;
    let file = tempfile::tempdir().unwrap();
    let path = file.path().join("livtk/app.toml");
    m.saved().save(&path).unwrap();
    let mut back = Model::open(&[dir.path().to_path_buf()]);
    back.restore(&Saved::load(&path));
    assert_eq!(back.detail_mode, livtk_app::model::DetailMode::Center);
}
