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

fn open_app<'a>(
    cx: &'a mut TestAppContext,
    dir: &std::path::Path,
) -> (
    gpui_kit::Entity<LivtkApp>,
    &'a mut gpui_kit::VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        bind_keys(cx);
    });
    let path = dir.to_path_buf();
    let (app, cx) =
        cx.add_window_view(|window, cx| LivtkApp::new(Model::open(&[path]), window, cx));
    cx.run_until_parked();
    (app, cx)
}

#[gpui_kit::test]
fn ctrl_slash_shows_the_shortcuts_and_esc_or_ctrl_slash_closes_them(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("ctrl-/");
    app.read_with(cx, |a, _| assert!(a.help_open()));
    cx.simulate_keystrokes("escape");
    app.read_with(cx, |a, _| assert!(!a.help_open(), "Esc closes it"));
    cx.simulate_keystrokes("ctrl-/");
    cx.simulate_keystrokes("ctrl-/");
    app.read_with(cx, |a, _| {
        assert!(!a.help_open(), "and so does Ctrl+/ again")
    });
}

#[gpui_kit::test]
fn a_click_outside_the_new_task_modal_closes_it(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("n");
    app.read_with(cx, |a, _| assert!(a.form_open()));
    // Top-left corner: the backdrop, well outside the centred card.
    cx.simulate_click(
        gpui_kit::point(gpui_kit::px(8.), gpui_kit::px(8.)),
        gpui_kit::Modifiers::default(),
    );
    app.read_with(cx, |a, _| assert!(!a.form_open(), "the backdrop closed it"));
    assert_eq!(
        Workspace::open(dir.path())
            .unwrap()
            .tasks(&[])
            .unwrap()
            .len(),
        2,
        "nothing was created"
    );
}

#[gpui_kit::test]
fn the_filter_narrows_the_list_and_typing_in_it_runs_no_command(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("/");
    cx.simulate_input("a sec");
    cx.run_until_parked();
    assert_eq!(
        state_of(dir.path(), "LIV-1"),
        State::Planned,
        "the `a` typed acknowledged nothing"
    );
    app.read_with(cx, |a, _| assert_eq!(a.model().display().filter, "a sec"));
    cx.simulate_keystrokes("backspace backspace backspace backspace backspace");
    cx.simulate_input("second");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        let keys: Vec<&str> = a.model().rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["LIV-2"], "only the second plan is left in the inbox");
    });
    cx.simulate_keystrokes("enter a");
    assert_eq!(
        state_of(dir.path(), "LIV-2"),
        State::Acked,
        "Enter went back to the list, where `a` decides"
    );
}

#[gpui_kit::test]
fn the_create_button_sits_inside_the_new_task_card(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("n");
    cx.run_until_parked();
    let card = cx.debug_bounds("new-task-card").expect("the card is drawn");
    let button = cx.debug_bounds("create-task").expect("the button is drawn");
    assert!(
        button.right() <= card.right()
            && button.bottom() <= card.bottom()
            && button.left() >= card.left(),
        "the button {button:?} spills out of the card {card:?}"
    );
}
