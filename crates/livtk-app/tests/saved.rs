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
    m.toggle_show_all();
    m.set_view(View::Lessons);
    m.set_ordering(Ordering::Updated);

    let file = tempfile::tempdir().unwrap();
    let path = file.path().join("livtk/app.toml");
    let mut saved = m.saved();
    saved.window = SavedWindow {
        sidebar_width: Some(280.),
        sidebar_collapsed: true,
        panel_width: Some(540.),
    };
    saved.save(&path).unwrap();

    let back = Saved::load(&path);
    assert_eq!(back, saved, "the file holds it all");
    let mut again = open(&[a.path(), b.path()]);
    again.restore(&back);
    assert_eq!(again.current, 1, "the same project");
    assert_eq!(again.view, View::Lessons, "the same view");
    assert_eq!(again.display().ordering, Ordering::Updated);
    assert_eq!(again.layout, Layout::Board);
    assert!(again.show_all);
    again.set_view(View::Tasks);
    let d = again.display();
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
    assert_eq!(m.view, View::Inbox, "the view stays");
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
