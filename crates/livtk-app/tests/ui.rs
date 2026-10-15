//! The window with real key events, headless: what the keys do, and what they must not do.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use gpui_kit::TestAppContext;
use livtk_app::model::{Model, View};
use livtk_app::ui::{LivtkApp, bind_keys};
use livtk_core::{Actor, Command, Kind, State, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

/// A project with two planned tasks and a proposed lesson.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    for t in ["first", "second"] {
        let e = ws
            .execute(
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
        let id = e[0].op.subjects().0.to_string();
        ws.execute(
            Command::Move {
                node: id,
                to: State::Planned,
            },
            &CLAUDE,
        )
        .unwrap();
    }
    ws.execute(
        Command::Create {
            kind: Kind::Lesson,
            title: "a rule".into(),
            body: String::new(),
            priority: None,
            links: vec![],
        },
        &CLAUDE,
    )
    .unwrap();
    dir
}

fn state_of(dir: &std::path::Path, key: &str) -> State {
    Workspace::open(dir).unwrap().node(key).unwrap().state
}

#[gpui_kit::test]
fn keys_move_the_selection_and_decide(cx: &mut TestAppContext) {
    let dir = project();
    cx.update(|cx| {
        gpui_kit::init(cx);
        bind_keys(cx);
    });
    let path = dir.path().to_path_buf();
    let (app, cx) =
        cx.add_window_view(|window, cx| LivtkApp::new(Model::open(&[path]), window, cx));
    cx.run_until_parked();

    cx.simulate_keystrokes("j");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().selected().unwrap().key, "LIV-2")
    });
    cx.simulate_keystrokes("a");
    assert_eq!(
        state_of(dir.path(), "LIV-2"),
        State::Acked,
        "a acknowledges the selected plan"
    );
    assert_eq!(state_of(dir.path(), "LIV-1"), State::Planned);

    cx.simulate_keystrokes("g t");
    app.read_with(cx, |a, _| assert_eq!(a.model().view, View::Tasks));
    cx.simulate_keystrokes("g l");
    app.read_with(cx, |a, _| assert_eq!(a.model().view, View::Lessons));
    cx.simulate_keystrokes("s");
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(
        ws.always_rules().unwrap().len(),
        1,
        "s ratifies as a standing rule"
    );
}

#[gpui_kit::test]
fn typing_in_the_note_field_never_triggers_a_command(cx: &mut TestAppContext) {
    let dir = project();
    cx.update(|cx| {
        gpui_kit::init(cx);
        bind_keys(cx);
    });
    let path = dir.path().to_path_buf();
    let (app, cx) =
        cx.add_window_view(|window, cx| LivtkApp::new(Model::open(&[path]), window, cx));
    cx.run_until_parked();

    cx.simulate_keystrokes("c");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert!(a.model().detail.is_some(), "c opens the panel for a note")
    });
    cx.simulate_input("a quick jot");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    assert_eq!(
        state_of(dir.path(), "LIV-1"),
        State::Planned,
        "typing `a` acknowledged nothing"
    );
    let history = Workspace::open(dir.path())
        .unwrap()
        .history(Some("LIV-1"))
        .unwrap();
    let last = history.last().unwrap();
    assert!(
        matches!(&last.op, livtk_core::event::Op::NodeNote { data, .. } if data.text == "a quick jot"),
        "{last:?}"
    );
    assert_eq!(last.by, "human");
}
