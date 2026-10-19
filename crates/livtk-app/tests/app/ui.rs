//! The window with real key events, headless: what the keys do, and what they must not do.
//!
//! These tests find elements by debug selectors, which GPUI records only in debug builds: run
//! them without `--release`. Only the ignored benchmark is meant for release.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use gpui_kit::TestAppContext;
use std::time::{Duration, Instant};

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
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Tasks));
    cx.simulate_keystrokes("g l");
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Lessons));
    cx.simulate_keystrokes("shift-a");
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(
        ws.always_rules().unwrap().len(),
        1,
        "Shift+A ratifies as a standing rule"
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
    // Shift+Enter breaks the line, Tab indents it and the field keeps the keys; Enter adds.
    cx.simulate_input("a quick");
    cx.simulate_keystrokes("shift-enter");
    cx.simulate_input("jot");
    cx.simulate_keystrokes("tab");
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
        matches!(&last.op, livtk_core::event::Op::NodeNote { data, .. } if data.text == "a quick\n  jot"),
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
    open_window(cx, dir)
}

/// Another window on the app `open_app` set up: as if it ran again.
fn open_window<'a>(
    cx: &'a mut TestAppContext,
    dir: &std::path::Path,
) -> (
    gpui_kit::Entity<LivtkApp>,
    &'a mut gpui_kit::VisualTestContext,
) {
    let path = dir.to_path_buf();
    let (app, cx) = cx.add_window_view(|window, cx| {
        // Keywords only, whatever models this machine has.
        let mut model = Model::open(&[path]);
        model.set_models_dir(None);
        LivtkApp::new(model, window, cx)
    });
    cx.run_until_parked();
    (app, cx)
}

#[gpui_kit::test]
fn ctrl_question_mark_shows_the_shortcuts_and_esc_or_it_again_closes_them(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("secondary-?");
    app.read_with(cx, |a, _| assert!(a.help_open()));
    cx.simulate_keystrokes("escape");
    app.read_with(cx, |a, _| assert!(!a.help_open(), "Esc closes it"));
    cx.simulate_keystrokes("secondary-shift-/");
    app.read_with(cx, |a, _| {
        assert!(a.help_open(), "Shift with / is the same key")
    });
    cx.simulate_keystrokes("secondary-?");
    app.read_with(cx, |a, _| {
        assert!(!a.help_open(), "and Ctrl+? again closes it")
    });
}

fn click_on(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
}

#[gpui_kit::test]
fn the_new_task_takes_its_priority_from_a_menu(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("n");
    cx.run_until_parked();
    click_on(cx, "priority-select");
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("priority")));
    click_on(cx, "priority-1");
    app.read_with(cx, |a, _| {
        assert_eq!(a.menu_open(), None);
        assert!(a.form_open(), "choosing kept the form open");
    });
    cx.simulate_input("urgent work");
    click_on(cx, "create-task");
    let ws = Workspace::open(dir.path()).unwrap();
    let created = ws
        .tasks(&[])
        .unwrap()
        .into_iter()
        .find(|t| t.title == "urgent work")
        .expect("created");
    assert_eq!(created.priority, 1);

    // Esc closes the menu before the form.
    cx.simulate_keystrokes("n");
    click_on(cx, "priority-select");
    cx.simulate_keystrokes("escape");
    app.read_with(cx, |a, _| {
        assert_eq!(a.menu_open(), None);
        assert!(a.form_open());
    });
}

#[gpui_kit::test]
fn slash_opens_the_filter_menu_and_esc_closes_it(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("/");
    cx.run_until_parked();
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("filter")));
    assert!(
        cx.debug_bounds("filter-menu").is_some(),
        "the menu is drawn"
    );
    cx.simulate_keystrokes("escape");
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), None));
    cx.simulate_keystrokes("j");
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.model().selected().unwrap().key,
            "LIV-2",
            "the list has the keys again"
        )
    });
}

#[gpui_kit::test]
fn a_click_outside_the_new_task_modal_closes_it(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("n");
    app.read_with(cx, |a, _| assert!(a.form_open()));
    // Away to another app and back by a click on the backdrop: that click only activates.
    cx.deactivate_window();
    let corner = gpui_kit::point(gpui_kit::px(8.), gpui_kit::px(8.));
    cx.simulate_event(gpui_kit::MouseDownEvent {
        position: corner,
        modifiers: gpui_kit::Modifiers::default(),
        button: gpui_kit::MouseButton::Left,
        click_count: 1,
        first_mouse: true,
    });
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert!(a.form_open(), "kept across an app switch")
    });
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
        let keys: Vec<&str> = a.model().rows().iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["LIV-2"], "only the second plan is left in the inbox");
    });
    cx.simulate_keystrokes("enter a");
    assert_eq!(
        state_of(dir.path(), "LIV-2"),
        State::Acked,
        "Enter went back to the list, where `a` decides"
    );
}

fn double_click_on(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    let at = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"))
        .center();
    for click_count in [1, 2] {
        cx.simulate_event(gpui_kit::MouseDownEvent {
            position: at,
            modifiers: gpui_kit::Modifiers::default(),
            button: gpui_kit::MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(gpui_kit::MouseUpEvent {
            position: at,
            modifiers: gpui_kit::Modifiers::default(),
            button: gpui_kit::MouseButton::Left,
            click_count,
        });
    }
    cx.run_until_parked();
}

/// Open the detail panel and let it finish sliding in: clicks land only once it has room.
fn open_detail(cx: &mut gpui_kit::VisualTestContext) {
    cx.simulate_keystrokes("enter");
    settle(cx);
}

/// Let the panel's slide finish. GPUI animations run on the wall clock, its timers on the test
/// executor's, so both have to move.
fn settle(cx: &mut gpui_kit::VisualTestContext) {
    cx.run_until_parked();
    std::thread::sleep(std::time::Duration::from_millis(300));
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn node_of(dir: &std::path::Path, key: &str) -> livtk_core::Node {
    Workspace::open(dir).unwrap().node(key).unwrap()
}

#[gpui_kit::test]
fn a_double_click_edits_the_title_and_esc_cancels(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    open_detail(cx);
    click_on(cx, "detail-title");
    app.read_with(cx, |a, _| {
        assert_eq!(a.editing(), None, "one click does not edit")
    });

    double_click_on(cx, "detail-title");
    app.read_with(cx, |a, _| assert_eq!(a.editing(), Some("title")));
    // Select all: Cmd+A on macOS, Ctrl+A elsewhere.
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("renamed plan");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(node_of(dir.path(), "LIV-1").title, "renamed plan");
    app.read_with(cx, |a, _| {
        assert_eq!(a.editing(), None);
        assert_eq!(
            a.model().detail.as_ref().unwrap().node.title,
            "renamed plan"
        );
    });
    cx.simulate_keystrokes("j");
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.model().selected().unwrap().key,
            "LIV-2",
            "the keys are back on the list"
        )
    });

    cx.simulate_keystrokes("escape");
    open_detail(cx);
    double_click_on(cx, "detail-title");
    cx.simulate_input(" and more");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        node_of(dir.path(), "LIV-2").title,
        "second",
        "Esc saved nothing"
    );
    app.read_with(cx, |a, _| assert_eq!(a.editing(), None));
}

#[gpui_kit::test]
fn the_body_is_edited_in_place_and_saved_with_ctrl_enter(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    open_detail(cx);
    // An empty body offers itself with one click.
    click_on(cx, "detail-body");
    app.read_with(cx, |a, _| assert_eq!(a.editing(), Some("body")));
    cx.simulate_input("goal: ship it");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("check: it runs");
    cx.simulate_keystrokes("secondary-enter");
    cx.run_until_parked();
    assert_eq!(
        node_of(dir.path(), "LIV-1").body,
        "goal: ship it\ncheck: it runs"
    );
    app.read_with(cx, |a, _| assert_eq!(a.editing(), None));

    double_click_on(cx, "detail-body");
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.editing(),
            Some("body"),
            "a body is edited by double-click"
        )
    });
}

fn palette_keys(
    app: &gpui_kit::Entity<LivtkApp>,
    cx: &mut gpui_kit::VisualTestContext,
) -> Vec<String> {
    app.read_with(cx, |a, _| {
        a.model()
            .palette
            .as_ref()
            .map(|p| {
                p.items
                    .iter()
                    .map(|i| match i {
                        livtk_app::model::Item::Action(a) => a.label.clone(),
                        livtk_app::model::Item::Node(n) => n.key.clone(),
                        livtk_app::model::Item::Doc(d) => d.path.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

#[gpui_kit::test]
fn ctrl_k_finds_a_node_and_typing_in_it_runs_no_command(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("secondary-k");
    cx.run_until_parked();
    app.read_with(cx, |a, _| assert!(a.palette_open()));
    assert!(cx.debug_bounds("palette").is_some(), "drawn");

    // Every one of these letters is a shortcut on the list.
    cx.simulate_input("seco");
    cx.run_until_parked();
    assert_eq!(palette_keys(&app, cx), ["LIV-2"]);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert!(!a.palette_open());
        assert_eq!(a.model().selected().unwrap().key, "LIV-2");
        assert_eq!(a.model().detail.as_ref().unwrap().node.key, "LIV-2");
    });
    assert_eq!(
        state_of(dir.path(), "LIV-1"),
        State::Planned,
        "no shortcut ran"
    );
    assert_eq!(state_of(dir.path(), "LIV-2"), State::Planned);
}

#[gpui_kit::test]
fn the_palette_runs_an_action_on_the_selected_node(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("secondary-k");
    cx.simulate_input("ack");
    cx.run_until_parked();
    assert_eq!(
        palette_keys(&app, cx).first().map(String::as_str),
        Some("Acknowledge")
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(state_of(dir.path(), "LIV-1"), State::Acked);
    cx.simulate_keystrokes("j");
    app.read_with(cx, |a, _| {
        assert!(
            a.model().selected().is_some(),
            "the keys are back on the list"
        )
    });
}

#[gpui_kit::test]
fn arrows_move_the_highlight_and_esc_or_ctrl_k_closes(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("secondary-k");
    cx.run_until_parked();
    let highlighted = |app: &gpui_kit::Entity<LivtkApp>, cx: &mut gpui_kit::VisualTestContext| {
        app.read_with(cx, |a, _| a.model().palette.as_ref().unwrap().selected)
    };
    cx.simulate_keystrokes("down down");
    assert_eq!(highlighted(&app, cx), 2);
    cx.simulate_keystrokes("up ctrl-n ctrl-n ctrl-p");
    assert_eq!(highlighted(&app, cx), 2);
    cx.simulate_keystrokes("escape");
    app.read_with(cx, |a, _| assert!(!a.palette_open(), "Esc closes it"));
    cx.simulate_keystrokes("secondary-k");
    cx.simulate_keystrokes("secondary-k");
    app.read_with(cx, |a, _| {
        assert!(!a.palette_open(), "and so does Ctrl+K again")
    });
}

#[gpui_kit::test]
fn a_click_runs_an_item_and_new_task_opens_the_form(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("secondary-k");
    cx.simulate_input("new task");
    cx.run_until_parked();
    assert_eq!(
        palette_keys(&app, cx).first().map(String::as_str),
        Some("New task")
    );
    click_on(cx, "palette-item-0");
    app.read_with(cx, |a, _| {
        assert!(!a.palette_open());
        assert!(a.form_open(), "the view did what the palette asked");
    });
    cx.simulate_keystrokes("secondary-k");
    app.read_with(cx, |a, _| {
        assert!(
            !a.palette_open(),
            "the form keeps the palette out, so nothing typed is lost"
        )
    });
}

#[gpui_kit::test]
fn g_a_opens_the_feed_and_enter_and_a_decide_from_it(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().projects[0].unseen, 5, "Claude's five events")
    });
    cx.simulate_keystrokes("g a");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Activity);
        assert_eq!(a.model().projects[0].unseen, 0, "looking clears the badge");
    });
    assert!(cx.debug_bounds("event-0").is_some(), "the lines are drawn");

    // Newest first: the lesson, then the second plan's move.
    cx.simulate_keystrokes("j");
    open_detail(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Activity, "the feed stays");
        assert_eq!(a.model().detail.as_ref().unwrap().node.key, "LIV-2");
    });
    cx.simulate_keystrokes("a");
    cx.run_until_parked();
    assert_eq!(
        state_of(dir.path(), "LIV-2"),
        State::Acked,
        "a decided the open plan"
    );
}

#[gpui_kit::test]
fn a_click_on_a_line_opens_its_node(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g a");
    cx.run_until_parked();
    click_on(cx, "event-4");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().activity_index, 4);
        assert_eq!(
            a.model().detail.as_ref().unwrap().node.key,
            "LIV-1",
            "the oldest event"
        );
    });
}

#[gpui_kit::test]
fn the_keys_still_work_when_focus_was_lost(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    // Focus can end up nowhere: an input that blurs itself, a panel that closed under it.
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("secondary-?");
    app.read_with(cx, |a, _| {
        assert!(a.help_open(), "Ctrl+? works from anywhere")
    });
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("j");
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.model().selected().unwrap().key,
            "LIV-2",
            "and so do the list's keys"
        )
    });
}

#[gpui_kit::test]
fn closing_the_panel_under_the_note_field_leaves_the_keys_working(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("c");
    settle(cx);
    cx.simulate_input("half a note");
    click_on(cx, "close-detail");
    settle(cx);
    app.read_with(cx, |a, _| assert!(a.model().detail.is_none()));
    cx.simulate_keystrokes("secondary-?");
    app.read_with(cx, |a, _| assert!(a.help_open()));
}

/// Two frames: the first learns the graph's size, the second draws it fitted.
fn draw_graph(cx: &mut gpui_kit::VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }
}

fn camera(
    app: &gpui_kit::Entity<LivtkApp>,
    cx: &mut gpui_kit::VisualTestContext,
) -> livtk_app::model::Camera {
    app.read_with(cx, |a, _| a.model().graph.camera)
}

/// A project whose second plan waits on the first.
fn linked_project() -> tempfile::TempDir {
    let dir = project();
    let mut ws = Workspace::open(dir.path()).unwrap();
    ws.execute(
        Command::Link {
            src: "LIV-2".into(),
            rel: livtk_core::Rel::DependsOn,
            dst: "LIV-1".into(),
        },
        &CLAUDE,
    )
    .unwrap();
    dir
}

#[gpui_kit::test]
fn g_r_opens_the_graph_fitted_and_a_click_opens_a_card(cx: &mut TestAppContext) {
    let dir = linked_project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g r");
    draw_graph(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Graph);
        assert_eq!(a.model().graph.nodes.len(), 3);
    });
    let graph = cx.debug_bounds("graph").expect("drawn");
    let one = cx.debug_bounds("gnode-LIV-1").expect("a card for LIV-1");
    let two = cx.debug_bounds("gnode-LIV-2").expect("a card for LIV-2");
    assert!(
        one.right() < two.left(),
        "LIV-2 waits on LIV-1, so sits right of it"
    );
    for card in [one, two] {
        assert!(
            card.left() >= graph.left() && card.right() <= graph.right(),
            "fitted: {card:?} inside {graph:?}"
        );
    }
    click_on(cx, "gnode-LIV-2");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Graph, "the graph stays");
        assert_eq!(a.model().detail.as_ref().unwrap().node.key, "LIV-2");
    });
}

#[gpui_kit::test]
fn dragging_the_background_pans_and_opens_nothing(cx: &mut TestAppContext) {
    let dir = linked_project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g r");
    draw_graph(cx);
    let before = camera(&app, cx);
    let graph = cx.debug_bounds("graph").unwrap();
    let start = gpui_kit::point(
        graph.left() + gpui_kit::px(10.),
        graph.top() + gpui_kit::px(10.),
    );
    let end = gpui_kit::point(start.x + gpui_kit::px(120.), start.y + gpui_kit::px(40.));
    cx.simulate_mouse_down(
        start,
        gpui_kit::MouseButton::Left,
        gpui_kit::Modifiers::default(),
    );
    cx.simulate_mouse_move(
        end,
        Some(gpui_kit::MouseButton::Left),
        gpui_kit::Modifiers::default(),
    );
    cx.simulate_mouse_up(
        end,
        gpui_kit::MouseButton::Left,
        gpui_kit::Modifiers::default(),
    );
    cx.run_until_parked();
    let after = camera(&app, cx);
    assert!((after.pan_x - before.pan_x - 120.).abs() < 0.01);
    assert!((after.pan_y - before.pan_y - 40.).abs() < 0.01);
    app.read_with(cx, |a, _| {
        assert!(a.model().detail.is_none(), "a drag is not a click")
    });
}

#[gpui_kit::test]
fn scrolling_pans_and_ctrl_scroll_zooms_at_the_pointer(cx: &mut TestAppContext) {
    let dir = linked_project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g r");
    draw_graph(cx);
    let graph = cx.debug_bounds("graph").unwrap();
    let at = gpui_kit::point(
        graph.left() + gpui_kit::px(200.),
        graph.top() + gpui_kit::px(150.),
    );
    let before = camera(&app, cx);
    cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position: at,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
            gpui_kit::px(-30.),
            gpui_kit::px(25.),
        )),
        modifiers: gpui_kit::Modifiers::default(),
        ..Default::default()
    });
    let panned = camera(&app, cx);
    assert!((panned.pan_x - before.pan_x + 30.).abs() < 0.01);
    assert!((panned.pan_y - before.pan_y - 25.).abs() < 0.01);
    assert_eq!(panned.zoom, before.zoom);

    let local = (200., 150.);
    let world = panned.to_world(local.0, local.1);
    cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position: at,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
            gpui_kit::px(0.),
            gpui_kit::px(-100.),
        )),
        modifiers: gpui_kit::Modifiers::control(),
        ..Default::default()
    });
    let zoomed = camera(&app, cx);
    assert!(zoomed.zoom < panned.zoom, "Ctrl+scroll zoomed");
    let (sx, sy) = zoomed.to_screen(world.0, world.1);
    assert!(
        (sx - local.0).abs() < 0.01 && (sy - local.1).abs() < 0.01,
        "at the pointer"
    );
}

#[gpui_kit::test]
fn keys_and_buttons_zoom_and_far_out_cards_become_dots(cx: &mut TestAppContext) {
    let dir = linked_project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g r");
    draw_graph(cx);
    let fitted = camera(&app, cx);
    cx.simulate_keystrokes("+");
    assert!((camera(&app, cx).zoom - fitted.zoom * 1.25).abs() < 1e-4);
    cx.simulate_keystrokes("- -");
    assert!((camera(&app, cx).zoom - fitted.zoom / 1.25).abs() < 1e-4);
    cx.simulate_keystrokes("0");
    assert_eq!(camera(&app, cx), fitted, "0 fits");

    click_on(cx, "zoom-in");
    assert!(camera(&app, cx).zoom > fitted.zoom);
    app.read_with(cx, |a, _| {
        assert!(a.model().detail.is_none(), "a control is not a card")
    });
    click_on(cx, "zoom-fit");
    assert_eq!(camera(&app, cx), fitted);

    for _ in 0..8 {
        click_on(cx, "zoom-out");
    }
    draw_graph(cx);
    assert!(camera(&app, cx).zoom < livtk_app::model::DOTS_BELOW);
    assert!(
        cx.debug_bounds("gnode-LIV-1").is_none(),
        "far out, cards are dots"
    );
    assert!(cx.debug_bounds("graph").is_some());
}

#[gpui_kit::test]
fn the_graph_fills_its_area_and_j_steps_in_reading_order(cx: &mut TestAppContext) {
    let dir = linked_project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g r");
    draw_graph(cx);
    let graph = cx.debug_bounds("graph").unwrap();
    assert!(
        graph.size.height > gpui_kit::px(600.),
        "the full height: {graph:?}"
    );
    assert!(
        graph.size.width > gpui_kit::px(1200.),
        "the full width: {graph:?}"
    );
    cx.simulate_keystrokes("j");
    open_detail(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.model().detail.as_ref().unwrap().node.key,
            "LIV-1",
            "left first"
        )
    });
    cx.simulate_keystrokes("j");
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.model().detail.as_ref().unwrap().node.key,
            "LIV-2",
            "then what waits"
        )
    });
}

/// `cargo test --release -p livtk-app --test ui -- --ignored --nocapture`: a frame of a
/// 1,000-node graph, fitted (far out, dots) and zoomed in (cards), without the GPU.
#[gpui_kit::test]
#[ignore = "a benchmark: run in release with --ignored"]
fn a_frame_of_a_thousand_node_graph_stays_in_budget(cx: &mut TestAppContext) {
    use livtk_core::event::{EdgeData, NodeData, Op};
    let dir = tempfile::tempdir().unwrap();
    Workspace::init(dir.path(), Some("LIV")).unwrap();
    let mut lines = String::new();
    let mut last = None;
    let mut ids = Vec::new();
    let mut push = |op: Op, last: &mut Option<uuid::Uuid>| {
        let e = livtk_core::Event::new(*last, &CLAUDE, op);
        *last = Some(e.id);
        lines.push_str(&e.to_line().unwrap());
        lines.push('\n');
    };
    for i in 1..=1_000u32 {
        let node = uuid::Uuid::now_v7();
        ids.push(node);
        push(
            Op::NodeCreate {
                node,
                data: NodeData {
                    kind: Kind::Task,
                    key: format!("LIV-{i}"),
                    title: format!("task {i} with an ordinary title"),
                    body: String::new(),
                    state: State::Backlog,
                    priority: livtk_core::model::Priority::DEFAULT,
                    props: serde_json::Map::new(),
                },
            },
            &mut last,
        );
    }
    for i in 1..ids.len() {
        push(
            Op::EdgeAdd {
                data: EdgeData {
                    src: ids[i],
                    dst: ids[(i * 7 + 3) % i],
                    rel: livtk_core::Rel::DependsOn,
                },
            },
            &mut last,
        );
    }
    std::fs::write(dir.path().join(".livtk/events.jsonl"), lines).unwrap();
    Workspace::open(dir.path()).unwrap().rebuild().unwrap();

    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g r");
    draw_graph(cx);
    let frame = |cx: &mut gpui_kit::VisualTestContext| {
        (0..10)
            .map(|_| {
                let t = std::time::Instant::now();
                cx.update(|window, _| window.refresh());
                cx.run_until_parked();
                t.elapsed()
            })
            .min()
            .unwrap()
    };
    let fitted = frame(cx);
    app.update(cx, |a, _| a.zoom_for_test(1.));
    let close = frame(cx);
    app.read_with(cx, |a, _| {
        let g = &a.model().graph;
        let (vw, vh) = g.viewport();
        let cam = g.camera;
        let shown = g
            .nodes
            .iter()
            .filter(|n| {
                let (x, y) = cam.to_screen(n.x, n.y);
                x + livtk_app::model::CARD_W * cam.zoom >= 0. && y + 36. >= 0. && x <= vw && y <= vh
            })
            .count();
        eprintln!("at zoom 1: {shown} cards in a {vw}x{vh} view");
    });
    eprintln!("a frame of 1,000 nodes: fitted {fitted:?}, at zoom 1 {close:?}");
    assert!(
        fitted < std::time::Duration::from_millis(16),
        "fitted: {fitted:?}"
    );
    assert!(
        close < std::time::Duration::from_millis(16),
        "zoom 1: {close:?}"
    );
}

/// A selector made at run time, as `debug_bounds` takes a `'static` name.
fn named(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn priority_of(dir: &std::path::Path, key: &str) -> u8 {
    Workspace::open(dir).unwrap().node(key).unwrap().priority
}

fn id_of(dir: &std::path::Path, key: &str) -> uuid::Uuid {
    Workspace::open(dir).unwrap().node(key).unwrap().id
}

#[gpui_kit::test]
fn a_rows_priority_icon_opens_a_menu_that_sets_it(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    cx.run_until_parked();
    let icon = named(format!("priority-of-{}", id_of(dir.path(), "LIV-2")));
    click_on(cx, icon);
    app.read_with(cx, |a, _| {
        assert_eq!(a.menu_open(), Some("row-priority"));
        assert_eq!(
            a.model().selected().unwrap().key,
            "LIV-1",
            "the click did not select the row"
        );
    });
    click_on(cx, "set-priority-1");
    assert_eq!(priority_of(dir.path(), "LIV-2"), 1, "LIV-2 is urgent now");
    assert_eq!(priority_of(dir.path(), "LIV-1"), 3);
    app.read_with(cx, |a, _| {
        assert_eq!(a.menu_open(), None);
        assert!(a.model().detail.is_none(), "nothing opened");
    });
}

#[gpui_kit::test]
fn a_board_cards_priority_and_the_panels_are_set_from_menus_too(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t b");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().layout(), livtk_app::model::Layout::Board)
    });
    click_on(
        cx,
        named(format!("priority-of-{}", id_of(dir.path(), "LIV-1"))),
    );
    click_on(cx, "set-priority-4");
    assert_eq!(priority_of(dir.path(), "LIV-1"), 4);

    open_detail(cx);
    click_on(cx, "detail-priority");
    app.read_with(cx, |a, _| {
        assert_eq!(a.menu_open(), Some("detail-priority"))
    });
    click_on(cx, "set-priority-2");
    let open = app.read_with(cx, |a, _| {
        a.model().detail.as_ref().unwrap().node.key.clone()
    });
    assert_eq!(
        priority_of(dir.path(), &open),
        2,
        "the panel's menu set the open task"
    );
}

#[gpui_kit::test]
fn a_link_is_added_from_the_panel_and_removed_with_its_cross(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t j");
    open_detail(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().detail.as_ref().unwrap().node.key, "LIV-2")
    });
    click_on(cx, "add-link");
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("add-link")));
    click_on(cx, "link-choice-0");
    app.read_with(cx, |a, _| {
        let p = a
            .model()
            .palette
            .as_ref()
            .expect("the palette picks the other end");
        assert_eq!(p.link.as_ref().unwrap().choice.label, "Waits for…");
    });
    cx.simulate_input("first");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(
        ws.links("LIV-2").unwrap().depends_on[0].key,
        "LIV-1",
        "LIV-2 waits for LIV-1"
    );
    app.read_with(cx, |a, _| assert!(!a.palette_open()));
    click_on(cx, "unlink-waits-for-LIV-1");
    let ws = Workspace::open(dir.path()).unwrap();
    assert!(
        ws.links("LIV-2").unwrap().depends_on.is_empty(),
        "the cross removed it"
    );
}

#[gpui_kit::test]
fn a_sub_task_is_added_from_the_panel_and_removed_with_its_cross(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    open_detail(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().detail.as_ref().unwrap().node.key, "LIV-1")
    });
    click_on(cx, "add-link");
    click_on(cx, "link-choice-3");
    app.read_with(cx, |a, _| {
        let p = a
            .model()
            .palette
            .as_ref()
            .expect("the palette picks the part");
        assert_eq!(p.link.as_ref().unwrap().choice.label, "Add sub-task…");
    });
    cx.simulate_input("second");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let ws = Workspace::open(dir.path()).unwrap();
    let (part_of, _) = ws.related("LIV-2", livtk_core::Rel::Parent).unwrap();
    assert_eq!(part_of[0].key, "LIV-1", "LIV-2 is part of LIV-1");
    redraw(cx);
    click_on(cx, "unlink-sub-tasks-LIV-2");
    let ws = Workspace::open(dir.path()).unwrap();
    let (_, parts) = ws.related("LIV-1", livtk_core::Rel::Parent).unwrap();
    assert!(parts.is_empty(), "the cross removed it");
}

#[gpui_kit::test]
fn the_palette_adds_a_link_from_its_actions(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    cx.simulate_keystrokes("secondary-k");
    cx.simulate_input("add link: blocks");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert!(a.palette_open(), "it stays open to pick the other end")
    });
    cx.simulate_input("seco");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(
        ws.links("LIV-1").unwrap().blocks[0].key,
        "LIV-2",
        "LIV-1 blocks LIV-2"
    );
}

#[gpui_kit::test]
fn the_wheel_scrolls_a_board_column_down_and_shift_scrolls_across(cx: &mut TestAppContext) {
    let dir = project();
    {
        let mut ws = Workspace::open(dir.path()).unwrap();
        for i in 0..30 {
            ws.execute(
                Command::Create {
                    kind: Kind::Task,
                    title: format!("filler {i}"),
                    body: String::new(),
                    priority: None,
                    links: vec![],
                },
                &CLAUDE,
            )
            .unwrap();
        }
    }
    let (_app, cx) = open_app(cx, dir.path());
    // Every column, so the board is wider than the window and scrolls across.
    cx.simulate_keystrokes("g t b s e");
    cx.run_until_parked();
    let first = cx.debug_bounds("card-LIV-3").expect("a backlog card");
    let other = cx.debug_bounds("card-LIV-1").expect("a planned card");
    let at = first.center();
    let wheel = |cx: &mut gpui_kit::VisualTestContext, dy: f32, shift: bool| {
        cx.simulate_event(gpui_kit::ScrollWheelEvent {
            position: at,
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                gpui_kit::px(0.),
                gpui_kit::px(dy),
            )),
            modifiers: gpui_kit::Modifiers {
                shift,
                ..Default::default()
            },
            ..Default::default()
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    };
    wheel(cx, -200., false);
    let scrolled = cx.debug_bounds("card-LIV-3").expect("still drawn");
    assert!(
        scrolled.top() < first.top() - gpui_kit::px(100.),
        "the column scrolled down"
    );
    assert_eq!(scrolled.left(), first.left(), "and not across");
    assert_eq!(
        cx.debug_bounds("card-LIV-1").unwrap().top(),
        other.top(),
        "only that column"
    );

    wheel(cx, -120., true);
    let across = cx.debug_bounds("card-LIV-3").expect("still drawn");
    assert!(
        across.left() < scrolled.left(),
        "Shift+wheel scrolled the board across"
    );
    assert_eq!(across.top(), scrolled.top(), "and the column did not move");
}

#[gpui_kit::test]
fn a_rows_state_icon_opens_the_flow_and_only_moves_are_taken(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    // The inbox: LIV-1 selected, LIV-2 below it.
    click_on(
        cx,
        named(format!("state-of-{}", id_of(dir.path(), "LIV-2"))),
    );
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("row-state")));
    assert!(
        cx.debug_bounds("set-state-doing").is_some(),
        "the whole flow shows"
    );
    click_on(cx, "set-state-doing");
    assert_eq!(
        state_of(dir.path(), "LIV-2"),
        State::Planned,
        "not a move from planned"
    );
    app.read_with(cx, |a, _| {
        assert_eq!(a.menu_open(), Some("row-state"), "still open")
    });
    click_on(cx, "set-state-acked");
    assert_eq!(
        state_of(dir.path(), "LIV-2"),
        State::Acked,
        "acknowledged from its icon"
    );
    app.read_with(cx, |a, _| {
        assert_eq!(a.menu_open(), None);
        assert_eq!(
            a.model().selected().unwrap().key,
            "LIV-1",
            "the selection stayed"
        );
    });
}

#[gpui_kit::test]
fn a_board_cards_state_icon_moves_it(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t b");
    cx.run_until_parked();
    click_on(
        cx,
        named(format!("state-of-{}", id_of(dir.path(), "LIV-1"))),
    );
    click_on(cx, "set-state-parked");
    assert_eq!(state_of(dir.path(), "LIV-1"), State::Parked);
}

#[gpui_kit::test]
fn the_panels_flow_moves_the_open_task(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    open_detail(cx);
    let panel = cx.debug_bounds("detail-body-panel").unwrap();
    let flow = cx.debug_bounds("state-flow").expect("the flow is drawn");
    assert!(
        flow.left() >= panel.left() && flow.right() <= panel.right(),
        "fits: {flow:?}"
    );
    click_on(cx, "flow-doing");
    assert_eq!(
        state_of(dir.path(), "LIV-1"),
        State::Planned,
        "doing is out of reach"
    );
    click_on(cx, "flow-acked");
    assert_eq!(state_of(dir.path(), "LIV-1"), State::Acked);
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.model().detail.as_ref().unwrap().node.state,
            State::Acked,
            "the panel follows"
        )
    });
    cx.run_until_parked();
    click_on(cx, "flow-doing");
    assert_eq!(
        state_of(dir.path(), "LIV-1"),
        State::Doing,
        "now doing is the next step"
    );
    click_on(cx, "flow-canceled");
    assert_eq!(
        state_of(dir.path(), "LIV-1"),
        State::Canceled,
        "the states off the path work too"
    );
}

#[gpui_kit::test]
fn ratifying_from_the_flow_offers_the_standing_rule(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g l");
    open_detail(cx);
    click_on(cx, "flow-ratified");
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("standing")));
    click_on(cx, "ratify-standing");
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.node("LES-1").unwrap().state, State::Ratified);
    assert_eq!(
        ws.always_rules().unwrap().len(),
        1,
        "ratified as a standing rule"
    );
}

fn inside(
    inner: gpui_kit::Bounds<gpui_kit::Pixels>,
    outer: gpui_kit::Bounds<gpui_kit::Pixels>,
) -> bool {
    inner.left() >= outer.left()
        && inner.right() <= outer.right()
        && inner.top() >= outer.top()
        && inner.bottom() <= outer.bottom()
}

#[gpui_kit::test]
fn modals_fit_a_short_window_and_scroll_inside(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    let short = gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(420.));
    cx.simulate_resize(short);
    cx.run_until_parked();
    cx.simulate_keystrokes("secondary-?");
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let card = cx
        .debug_bounds("shortcuts-card")
        .expect("the shortcuts are open");
    assert!(card.top() >= gpui_kit::px(0.), "{card:?}");
    assert!(
        card.bottom() <= short.height,
        "the card {card:?} ends inside a window {} tall",
        short.height
    );
    let scroll = cx.debug_bounds("shortcuts-scroll").unwrap();
    assert!(
        scroll.size.height > gpui_kit::px(100.),
        "the list keeps room to scroll in: {scroll:?}"
    );
    cx.simulate_keystrokes("escape secondary-k");
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let palette = cx.debug_bounds("palette").expect("the palette is open");
    assert!(
        palette.bottom() <= short.height,
        "the palette fits too: {palette:?}"
    );
}

/// The top bar's field opens the palette, as its keys do; Esc closes it. The dock has no palette button.
#[gpui_kit::test]
fn the_top_bars_field_opens_the_palette_over_it(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    let bar = cx.debug_bounds("top-bar").expect("the top bar");
    let field = cx.debug_bounds("top-field").expect("its field");
    assert!(inside(field, bar));
    click_on(cx, "top-field");
    app.read_with(cx, |a, _| assert!(a.palette_open()));
    cx.simulate_keystrokes("escape");
    app.read_with(cx, |a, _| assert!(!a.palette_open()));
    cx.simulate_keystrokes("cmd-k");
    app.read_with(cx, |a, _| assert!(a.palette_open(), "its keys too"));
    assert!(cx.debug_bounds("dock-palette").is_none());
}

/// Which of the dock's buttons are drawn.
fn dock_holds(cx: &mut gpui_kit::VisualTestContext) -> Vec<&'static str> {
    [
        "dock-new-task",
        "zoom-in",
        "filter-button",
        "dock-grouping",
        "dock-ordering",
        "dock-view",
        "dock-dismissed",
        // List and Board were tabs here: kept, so they cannot come back unseen.
        "layout-tabs",
        "layout-list",
        "layout-board",
    ]
    .into_iter()
    .filter(|b| cx.debug_bounds(b).is_some())
    .collect()
}

#[gpui_kit::test]
fn the_dock_holds_what_each_view_can_do(cx: &mut TestAppContext) {
    let dir = linked_project();
    let (_app, cx) = open_app(cx, dir.path());
    assert_eq!(dock_holds(cx), ["filter-button"], "the inbox");
    cx.simulate_keystrokes("g t");
    cx.run_until_parked();
    assert_eq!(
        dock_holds(cx),
        [
            "dock-new-task",
            "filter-button",
            "dock-grouping",
            "dock-ordering",
            "dock-view",
        ],
        "tasks"
    );
    cx.simulate_keystrokes("g l");
    cx.run_until_parked();
    assert_eq!(
        dock_holds(cx),
        [
            "filter-button",
            "dock-grouping",
            "dock-ordering",
            "dock-view",
        ],
        "lessons"
    );
    cx.simulate_keystrokes("g a");
    cx.run_until_parked();
    assert!(dock_holds(cx).is_empty(), "the feed");
    assert!(cx.debug_bounds("dock").is_none(), "and no empty dock there");
    cx.simulate_keystrokes("g r");
    draw_graph(cx);
    assert_eq!(dock_holds(cx), ["zoom-in", "dock-dismissed"], "the graph");
    assert!(
        cx.debug_bounds("view-header").is_none(),
        "no header over the view"
    );
}

/// A project with `n` more backlog tasks than the usual two plans and a lesson.
fn long_project(n: usize) -> tempfile::TempDir {
    let dir = project();
    let mut ws = Workspace::open(dir.path()).unwrap();
    for i in 0..n {
        ws.execute(
            Command::Create {
                kind: Kind::Task,
                title: format!("filler {i}"),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &CLAUDE,
        )
        .unwrap();
    }
    dir
}

#[gpui_kit::test]
fn the_last_row_and_card_scroll_clear_of_the_dock(cx: &mut TestAppContext) {
    let dir = long_project(40);
    let (_app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    cx.run_until_parked();
    for _ in 0..50 {
        cx.simulate_keystrokes("j");
    }
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let dock = cx.debug_bounds("dock").unwrap();
    let last = cx
        .debug_bounds("key-LIV-42")
        .expect("the last row is shown");
    assert!(
        last.bottom() <= dock.top(),
        "the last row {last:?} ends above the dock {dock:?}"
    );

    cx.simulate_keystrokes("b");
    cx.run_until_parked();
    let at = cx.debug_bounds("card-LIV-3").unwrap().center();
    cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position: at,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
            gpui_kit::px(0.),
            gpui_kit::px(-20000.),
        )),
        ..Default::default()
    });
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let dock = cx.debug_bounds("dock").unwrap();
    let card = cx
        .debug_bounds("card-LIV-42")
        .expect("the column's last card");
    assert!(
        card.bottom() <= dock.top(),
        "the last card {card:?} ends above the dock {dock:?}"
    );
}

#[gpui_kit::test]
fn the_docks_new_task_button_works(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    cx.run_until_parked();
    click_on(cx, "dock-new-task");
    app.read_with(cx, |a, _| assert!(a.form_open()));
    cx.simulate_keystrokes("escape");
    click_on(cx, "dock-view");
    click_on(cx, "view-layout-board");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().layout(), livtk_app::model::Layout::Board)
    });
    click_on(cx, "view-show-dismissed");
    app.read_with(cx, |a, _| assert!(a.model().display().show_dismissed));
}

/// Redraw the window, as the next frame would.
fn redraw(cx: &mut gpui_kit::VisualTestContext) {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn resize(cx: &mut gpui_kit::VisualTestContext, w: f32, h: f32) {
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(w), gpui_kit::px(h)));
    redraw(cx);
    redraw(cx);
}

/// Press on `selector`, move the pointer to `x` with the button held, and let go there.
fn drag_to(cx: &mut gpui_kit::VisualTestContext, selector: &'static str, x: f32) {
    let from = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"))
        .center();
    drag(cx, from, gpui_kit::point(gpui_kit::px(x), from.y));
}

/// Drag what `selector` is (its middle) up or down to height `y`.
fn drag_down_to(cx: &mut gpui_kit::VisualTestContext, selector: &'static str, y: f32) {
    let from = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"))
        .center();
    drag(cx, from, gpui_kit::point(from.x, gpui_kit::px(y)));
}

fn drag(
    cx: &mut gpui_kit::VisualTestContext,
    from: gpui_kit::Point<gpui_kit::Pixels>,
    to: gpui_kit::Point<gpui_kit::Pixels>,
) {
    cx.simulate_event(gpui_kit::MouseDownEvent {
        position: from,
        modifiers: gpui_kit::Modifiers::default(),
        button: gpui_kit::MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    redraw(cx);
    cx.simulate_event(gpui_kit::MouseMoveEvent {
        position: to,
        modifiers: gpui_kit::Modifiers::default(),
        pressed_button: Some(gpui_kit::MouseButton::Left),
    });
    redraw(cx);
    cx.simulate_event(gpui_kit::MouseUpEvent {
        position: to,
        modifiers: gpui_kit::Modifiers::default(),
        button: gpui_kit::MouseButton::Left,
        click_count: 1,
    });
    redraw(cx);
}

fn width_of(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) -> f32 {
    f32::from(
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"))
            .size
            .width,
    )
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.
}

/// An edge's handle sits where the pointer is on it, though the edge starts under the top bar.
#[gpui_kit::test]
fn an_edges_handle_follows_the_pointer_along_it(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    resize(cx, 1400., 900.);
    let edge = cx.debug_bounds("sidebar-edge").expect("the sidebar's edge");
    let at = gpui_kit::point(edge.center().x, gpui_kit::px(400.));
    cx.simulate_mouse_move(at, None, gpui_kit::Modifiers::default());
    redraw(cx);
    let handle = cx.debug_bounds("edge-handle").expect("its handle shows");
    assert!(
        (f32::from(handle.center().y) - 400.).abs() < 1.,
        "at the pointer: {handle:?}"
    );
}

#[gpui_kit::test]
fn the_sidebar_edge_resizes_it_within_limits_and_snaps_to_the_rail(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    resize(cx, 1400., 900.);
    assert!(
        near(width_of(cx, "sidebar"), 232.),
        "it starts at its width"
    );
    drag_to(cx, "sidebar-edge", 300.);
    assert!(
        near(width_of(cx, "sidebar"), 300.),
        "it follows the pointer"
    );
    app.read_with(cx, |a, _| {
        assert!(!a.resizing(), "and lets go with the button")
    });
    drag_to(cx, "sidebar-edge", 900.);
    assert!(near(width_of(cx, "sidebar"), 360.), "up to its widest");
    drag_to(cx, "sidebar-edge", 170.);
    assert!(near(width_of(cx, "sidebar"), 200.), "down to its narrowest");
    double_click_on(cx, "sidebar-edge");
    redraw(cx);
    assert!(
        near(width_of(cx, "sidebar"), 232.),
        "a double-click resets it"
    );
    drag_to(cx, "sidebar-edge", 100.);
    assert!(
        cx.debug_bounds("sidebar").is_none(),
        "dragged under, it collapses"
    );
    assert!(near(width_of(cx, "rail"), 52.), "to the rail");
}

#[gpui_kit::test]
fn ctrl_b_and_the_top_bar_button_collapse_and_expand_the_sidebar(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    // What the toggles do, not the slide, which moves the button while it plays.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("secondary-b");
    redraw(cx);
    assert!(cx.debug_bounds("rail").is_some(), "Ctrl+B collapses it");
    assert!(cx.debug_bounds("sidebar").is_none());
    cx.simulate_keystrokes("secondary-b");
    redraw(cx);
    assert!(cx.debug_bounds("sidebar").is_some(), "and expands it again");
    click_on(cx, "toggle-left");
    redraw(cx);
    assert!(cx.debug_bounds("rail").is_some(), "so does the button");
    click_on(cx, "toggle-left");
    redraw(cx);
}

/// The top bar's toggles at its right: the bottom bar and the detail panel, each by its button
/// or its keys (the sidebar's, above). The view header has no toggle of its own any more.
#[gpui_kit::test]
fn the_top_bars_toggles_show_and_hide_the_bottom_bar_and_the_panel(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("g t");
    redraw(cx);
    assert!(cx.debug_bounds("sidebar-toggle").is_none());
    let bar = cx.debug_bounds("top-bar").unwrap();
    for t in ["toggle-left", "toggle-bottom", "toggle-right"] {
        assert!(
            inside(cx.debug_bounds(t).unwrap(), bar),
            "{t} in the top bar"
        );
    }
    click_on(cx, "toggle-bottom");
    app.read_with(cx, |a, _| assert!(a.bottom_open()));
    cx.simulate_keystrokes("secondary-j");
    app.read_with(cx, |a, _| assert!(!a.bottom_open(), "its keys too"));
    click_on(cx, "toggle-right");
    settle(cx);
    app.read_with(cx, |a, _| {
        assert!(a.model().detail.is_some(), "on the selected task")
    });
    cx.simulate_keystrokes("secondary-alt-b");
    settle(cx);
    app.read_with(cx, |a, _| assert!(a.model().detail.is_none()));
    cx.simulate_keystrokes("secondary-alt-b");
    settle(cx);
    app.read_with(cx, |a, _| assert!(a.model().detail.is_some(), "and back"));
}

/// The open project is named in the top bar; a click lists the recent ones under it, and
/// picking one opens it.
#[gpui_kit::test]
fn the_top_bar_names_the_project_and_its_menu_switches(cx: &mut TestAppContext) {
    let (a, b) = (project(), project());
    cx.update(|cx| {
        gpui_kit::init(cx);
        bind_keys(cx);
    });
    let (pa, pb) = (a.path().to_path_buf(), b.path().to_path_buf());
    let (app, cx) = cx.add_window_view(|window, cx| {
        let mut model = Model::open(&[pa, pb]);
        model.set_models_dir(None);
        LivtkApp::new(model, window, cx)
    });
    resize(cx, 1400., 900.);
    redraw(cx);
    let bar = cx.debug_bounds("top-bar").unwrap();
    assert!(inside(cx.debug_bounds("top-project").unwrap(), bar));
    assert!(cx.debug_bounds("sidebar").is_some());
    click_on(cx, "top-project");
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("projects")));
    assert!(
        cx.debug_bounds("menu-open-project").is_some(),
        "it can open another"
    );
    click_on(cx, "menu-project-1");
    app.read_with(cx, |a, _| {
        assert_eq!(a.menu_open(), None, "picking one closes it");
        assert_eq!(a.model().current, 1);
        assert_eq!(a.model().recent_projects()[0], 1, "first among the recent");
    });
}

/// G then X opens the project's terminal, with the keys: there, the list's keys (G then T) are
/// the shell's, and the app's Command keys stay the app's. A click on the sidebar takes the keys
/// back to the views.
#[gpui_kit::test]
fn the_terminal_has_the_keys_but_the_apps_command_keys(cx: &mut TestAppContext) {
    let dir = project();
    // A shell is the core's to test (tests/app/terminal.rs): here, the view and its keys.
    cx.update(|cx| cx.set_global(livtk_app::ui::terminal::Headless));
    let (app, cx) = open_app(cx, dir.path());
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("g x");
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Terminal));
    assert!(
        cx.debug_bounds("terminal").is_some(),
        "the terminal is drawn"
    );
    cx.simulate_keystrokes("g t");
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Terminal, "typed into the shell");
    });
    cx.update(|window, cx| {
        assert!(app.read(cx).terminal_focused(window, cx), "it has the keys");
    });
    cx.simulate_keystrokes("secondary-k");
    app.read_with(cx, |a, _| {
        assert!(a.palette_open(), "the palette's key is the app's")
    });
    cx.simulate_keystrokes("escape");
    redraw(cx);
    app.read_with(cx, |a, _| assert!(!a.palette_open(), "Esc closed it"));
    // A menu opened and shut from the bottom bar's grid gives the keys back here too.
    cx.simulate_keystrokes("secondary-j");
    settle(cx);
    click_on(cx, "grid-what");
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("grid-what")));
    cx.simulate_keystrokes("escape");
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), None, "Esc closed it"));
    cx.update(|window, cx| {
        assert!(
            app.read(cx).terminal_focused(window, cx),
            "the menu shut, the keys are the terminal's again"
        );
    });
    cx.update(|window, cx| {
        assert!(
            app.read(cx).terminal_focused(window, cx),
            "the palette closed, the keys are the terminal's again"
        );
    });
    click_on(cx, "view-Tasks");
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Tasks));
    cx.simulate_keystrokes("g x");
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.model().view(),
            View::Terminal,
            "the list's keys work again"
        );
    });
}

/// G then O opens the docs as a list, as the tasks are: J and K walk it, Enter opens a doc to
/// read in the list's place with its details in the panel, Esc gives the list back. A link
/// followed opens another doc, or a node in the panel (Esc gives the doc's details back), whose
/// Docs group lists what it is linked to.
#[gpui_kit::test]
fn the_docs_list_opens_a_doc_to_read_and_follows_links(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs/guide")).unwrap();
    std::fs::write(
        root.join("docs/README.md"),
        "# Start\nSee [setup](guide/setup.md).\n",
    )
    .unwrap();
    std::fs::write(root.join("docs/guide/setup.md"), "# Setup\n").unwrap();
    let (app, cx) = open_app(cx, root);
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("g o");
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Docs));
    assert!(cx.debug_bounds("docs-list").is_some(), "the list");
    assert!(
        cx.debug_bounds("doc-folder-docs/guide").is_some(),
        "a folder"
    );
    assert!(cx.debug_bounds("doc-reader").is_none(), "nothing read yet");
    assert!(cx.debug_bounds("doc-panel").is_none(), "nor its panel");
    // J onto the folder, Left shuts it; the dock's Fold all opens all again once all are shut;
    // Right steps into it; Enter reads setup.
    cx.simulate_keystrokes("j left");
    redraw(cx);
    assert!(
        cx.debug_bounds("doc-row-docs/guide/setup.md").is_none(),
        "folded"
    );
    click_on(cx, "dock-fold-docs");
    redraw(cx);
    assert!(
        cx.debug_bounds("doc-row-docs/guide/setup.md").is_some(),
        "unfolded"
    );
    cx.simulate_keystrokes("right enter");
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().docs.open.as_deref(), Some("docs/guide/setup.md"));
        assert!(a.model().docs.reading);
    });
    assert!(cx.debug_bounds("doc-reader").is_some(), "the reader");
    assert!(
        cx.debug_bounds("docs-list").is_none(),
        "in the list's place"
    );
    assert!(
        cx.debug_bounds("doc-panel").is_some(),
        "its details in the panel"
    );
    // Esc: the list again, on the doc.
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert!(cx.debug_bounds("docs-list").is_some(), "the list again");
    assert!(cx.debug_bounds("doc-panel").is_none());
    click_on(cx, "doc-row-docs/README.md");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().docs.open.as_deref(), Some("docs/README.md"))
    });
    // The panel put away stays away; the panel's key brings it back.
    click_on(cx, "close-doc-panel");
    assert!(cx.debug_bounds("doc-panel").is_none(), "put away");
    cx.dispatch_action(livtk_app::ui::TogglePanel);
    redraw(cx);
    assert!(cx.debug_bounds("doc-panel").is_some(), "back");
    cx.dispatch_action(livtk_app::ui::FollowLink {
        href: "guide/setup.md".into(),
    });
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.model().docs.open.as_deref(),
            Some("docs/guide/setup.md"),
            "a link to a doc"
        )
    });
    // A task linked, in the panel's Tasks: a click opens it in the panel, Esc gives the doc's
    // details back.
    app.update(cx, |a, _| {
        a.model_mut().link_doc("LIV-1", "docs/guide/setup.md", true)
    });
    redraw(cx);
    click_on(cx, "doc-link-LIV-1");
    settle(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.model().detail.as_ref().map(|d| d.node.key.as_str()),
            Some("LIV-1")
        )
    });
    assert!(
        cx.debug_bounds("detail-docs").is_some(),
        "the task's panel lists its docs"
    );
    cx.simulate_keystrokes("escape");
    settle(cx);
    app.read_with(cx, |a, _| {
        assert!(a.model().detail.is_none());
        assert!(a.model().docs.reading, "still reading");
    });
    assert!(
        cx.debug_bounds("doc-panel").is_some(),
        "the doc's details again"
    );
    cx.dispatch_action(livtk_app::ui::FollowLink {
        href: "livtk:LIV-1".into(),
    });
    settle(cx);
    click_on(cx, "detail-doc-docs/guide/setup.md");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().docs.open.as_deref(), Some("docs/guide/setup.md"))
    });
}

/// A doc longer than the page scrolls.
#[gpui_kit::test]
fn a_long_doc_scrolls(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let long: String = (1..=200).map(|i| format!("Line {i}\n\n")).collect();
    std::fs::write(root.join("docs/long.md"), format!("# Long\n\n{long}")).unwrap();
    let (_app, cx) = open_app(cx, root);
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("g o enter");
    redraw(cx);
    let page = cx.debug_bounds("doc-page").expect("the page");
    let before = cx.debug_bounds("doc-header").expect("the doc's header");
    let end = cx.debug_bounds("doc-footer").expect("its end");
    assert!(
        end.bottom() > page.bottom(),
        "longer than the page: {end:?}"
    );
    cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position: page.center(),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
            gpui_kit::px(0.),
            gpui_kit::px(-400.),
        )),
        ..Default::default()
    });
    redraw(cx);
    let after = cx.debug_bounds("doc-header").unwrap();
    assert!(
        after.top() < before.top(),
        "scrolled: {before:?} then {after:?}"
    );
}

/// The reader: the doc's title over it, its headings in the panel's On this page (a click goes
/// there), a link to a heading of another doc landing on it, Cmd-F finding in the doc, and
/// Cmd-[ going back.
#[gpui_kit::test]
fn the_reader_has_an_outline_finds_and_goes_back(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let filler: String = (1..=40).map(|i| format!("Line {i}\n\n")).collect();
    std::fs::write(
        root.join("docs/a.md"),
        format!("# Alpha\n\nIntro.\n\n## First\n\n{filler}## Second\n\nThe needle.\n\n{filler}"),
    )
    .unwrap();
    std::fs::write(root.join("docs/b.md"), "# Beta\n\nSee [a](a.md#second).\n").unwrap();
    let (app, cx) = open_app(cx, root);
    // Where the reader lands, not its glide there.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1600., 1000.);
    cx.simulate_keystrokes("g o enter");
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().docs.open.as_deref(), Some("docs/a.md"))
    });
    assert!(cx.debug_bounds("doc-title").is_some());
    assert!(
        cx.debug_bounds("doc-outline").is_some(),
        "two headings: an outline"
    );
    // The outline goes to a heading: its section comes to the top.
    click_on(cx, "doc-outline-second");
    redraw(cx);
    let page = cx.debug_bounds("doc-page").unwrap();
    let second = cx
        .debug_bounds("doc-section-2")
        .expect("the second heading's section");
    assert!(
        (f32::from(second.top()) - f32::from(page.top())).abs() < 2.,
        "at the top: {second:?} in {page:?}"
    );
    // Find: the needle.
    cx.simulate_keystrokes("secondary-f");
    redraw(cx);
    assert!(cx.debug_bounds("doc-find").is_some(), "the find bar");
    cx.update(|window, cx| {
        assert_eq!(
            app.read(cx).doc_find(window, cx),
            (true, true),
            "open, with the keys"
        );
    });
    cx.simulate_input("needle");
    redraw(cx);
    cx.update(|window, cx| {
        assert_eq!(
            app.read(cx).doc_find(window, cx),
            (true, true),
            "still, after typing"
        );
    });
    cx.simulate_keystrokes("escape");
    redraw(cx);
    cx.update(|window, cx| {
        assert!(!app.read(cx).doc_find(window, cx).0, "Esc shuts it");
    });
    assert!(cx.debug_bounds("doc-find").is_none(), "and it is gone");
    // Another doc, a link to a heading of the first: it lands there; Cmd-[ goes back.
    app.update(cx, |a, _| a.model_mut().open_doc("docs/b.md"));
    redraw(cx);
    cx.dispatch_action(livtk_app::ui::FollowLink {
        href: "a.md#second".into(),
    });
    redraw(cx);
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().docs.open.as_deref(), Some("docs/a.md"));
        assert_eq!(a.model().docs.anchor, None, "landed")
    });
    cx.simulate_keystrokes("secondary-[");
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().docs.open.as_deref(), Some("docs/b.md"))
    });
}

/// The docs are searched through the filter, as a list is: what it found listed in place of
/// every doc; one opens with the find bar on the query.
#[gpui_kit::test]
fn the_docs_filter_searches_and_opens_with_the_find_bar(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/a.md"), "# Alpha\n").unwrap();
    std::fs::write(root.join("docs/b.md"), "# Beta\n\nThe hidden needle.\n").unwrap();
    let (app, cx) = open_app(cx, root);
    resize(cx, 1600., 1000.);
    cx.simulate_keystrokes("g o /");
    redraw(cx);
    assert!(cx.debug_bounds("filter-menu").is_some(), "the filter");
    cx.simulate_input("needle");
    redraw(cx);
    assert!(
        cx.debug_bounds("doc-row-docs/a.md").is_none(),
        "only what it found"
    );
    assert!(cx.debug_bounds("doc-row-docs/b.md").is_some());
    // Enter gives the keys back to the list; Enter there opens what is highlighted.
    cx.simulate_keystrokes("enter");
    redraw(cx);
    cx.simulate_keystrokes("enter");
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().docs.open.as_deref(), Some("docs/b.md"));
        assert!(a.model().docs.reading);
    });
    cx.update(|window, cx| assert!(app.read(cx).doc_find(window, cx).0, "the find bar, on it"));
    // Back to the list by the bar's close: the find bar goes with the reading.
    click_on(cx, "doc-close");
    redraw(cx);
    assert!(cx.debug_bounds("docs-list").is_some());
    assert!(
        cx.debug_bounds("doc-find").is_none(),
        "no find bar over the list"
    );
}

/// A doc's callouts and diagrams are blocks of their own; its frontmatter shows in its panel.
#[gpui_kit::test]
fn callouts_diagrams_and_frontmatter_show(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(
        root.join("docs/a.md"),
        "---\ntitle: Alpha\nstatus: draft\n---\n\n> [!NOTE]\n> Read me.\n\n```mermaid\nflowchart LR\n  A --> B\n```\n",
    )
    .unwrap();
    let (_app, cx) = open_app(cx, root);
    resize(cx, 1600., 1000.);
    cx.simulate_keystrokes("g o enter");
    redraw(cx);
    assert!(cx.debug_bounds("callout-note").is_some(), "the callout");
    assert!(cx.debug_bounds("doc-status").is_some(), "the status");
}

/// Edit puts the doc's markdown in the reader's place; Cmd-S writes it; Esc with unsaved text
/// asks first, and leaves once saved.
#[gpui_kit::test]
fn a_doc_is_edited_in_place_and_saved(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/a.md"), "# Alpha\n").unwrap();
    let (app, cx) = open_app(cx, root);
    resize(cx, 1600., 1000.);
    cx.simulate_keystrokes("g o enter");
    redraw(cx);
    click_on(cx, "doc-edit");
    redraw(cx);
    assert!(cx.debug_bounds("doc-editor").is_some(), "the editor");
    assert!(
        cx.debug_bounds("doc-reader").is_none(),
        "in the reader's place"
    );
    cx.simulate_keystrokes("cmd-down");
    cx.simulate_input("More.");
    redraw(cx);
    assert!(cx.debug_bounds("doc-unsaved").is_some(), "unsaved");
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert!(
        cx.debug_bounds("doc-leaving").is_some(),
        "asked before leaving"
    );
    cx.simulate_keystrokes("secondary-s");
    redraw(cx);
    let saved = std::fs::read_to_string(root.join("docs/a.md")).unwrap();
    assert!(saved.contains("More."), "{saved:?}");
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert!(
        cx.debug_bounds("doc-editor").is_none(),
        "saved: left at once"
    );
    app.read_with(cx, |a, _| assert!(a.model().docs.editing.is_none()));
}

/// Settings: Cmd-comma opens it; the docs' text size and density change the reader and are
/// the model's to remember; Esc shuts it.
#[gpui_kit::test]
fn settings_set_how_docs_read(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("secondary-,");
    redraw(cx);
    assert!(cx.debug_bounds("settings").is_some());
    click_on(cx, "reading-size-large");
    click_on(cx, "reading-density-compact");
    app.read_with(cx, |a, _| {
        let r = a.model().reading;
        assert_eq!(
            (r.size, r.compact),
            (livtk_app::model::ReadingSize::Large, true)
        );
    });
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert!(cx.debug_bounds("settings").is_none());
}

/// A project without docs offers to start them: a README in docs/, opened.
#[gpui_kit::test]
fn the_docs_can_be_started_from_their_empty_view(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g o");
    redraw(cx);
    click_on(cx, "empty-create-docs");
    redraw(cx);
    assert!(dir.path().join("docs/README.md").is_file());
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().docs.open.as_deref(), Some("docs/README.md"))
    });
}

/// An empty list offers its way on: the filter cleared when one hides everything.
#[gpui_kit::test]
fn an_empty_list_offers_to_clear_its_filter(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    app.update(cx, |a, _| a.model_mut().set_filter("nothing matches this"));
    redraw(cx);
    click_on(cx, "empty-clear-filter");
    app.read_with(cx, |a, _| {
        assert!(a.model().display().filter.is_empty());
        assert!(!a.model().rows().is_empty());
    });
}

#[gpui_kit::test]
fn the_rails_buttons_go_to_views_and_shortcuts(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("secondary-b");
    redraw(cx);
    assert!(
        cx.debug_bounds("rail-badge").is_some(),
        "the proposed lesson waits in the inbox: its button has a dot"
    );
    click_on(cx, "rail-Tasks");
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Tasks));
    click_on(cx, "rail-Lessons");
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Lessons));
    assert!(
        cx.debug_bounds("rail-projects").is_none(),
        "projects are the top bar's"
    );
    click_on(cx, "rail-shortcuts");
    app.read_with(cx, |a, _| assert!(a.help_open()));
}

#[gpui_kit::test]
fn a_narrow_window_shows_the_rail_and_ctrl_b_opens_the_sidebar_there(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    resize(cx, 900., 700.);
    assert!(
        cx.debug_bounds("rail").is_some(),
        "too narrow for the sidebar"
    );
    cx.simulate_keystrokes("secondary-b");
    redraw(cx);
    assert!(cx.debug_bounds("sidebar").is_some(), "opened by hand");
    resize(cx, 1400., 900.);
    assert!(
        cx.debug_bounds("sidebar").is_some(),
        "wide, it is as chosen"
    );
    resize(cx, 900., 700.);
    assert!(
        cx.debug_bounds("rail").is_some(),
        "narrow again, it is the rail again: opening it there was for then"
    );
}

#[gpui_kit::test]
fn the_panel_edge_resizes_it_within_limits_and_double_click_resets(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    resize(cx, 1600., 900.);
    open_detail(cx);
    assert!(
        near(width_of(cx, "detail-panel"), 480.),
        "it opens at its width"
    );
    drag_to(cx, "panel-edge", 1000.);
    assert!(
        near(width_of(cx, "detail-panel"), 600.),
        "what lies right of the pointer"
    );
    drag_to(cx, "panel-edge", 100.);
    assert!(near(width_of(cx, "detail-panel"), 720.), "up to its widest");
    drag_to(cx, "panel-edge", 1500.);
    assert!(
        near(width_of(cx, "detail-panel"), 360.),
        "down to its narrowest"
    );
    double_click_on(cx, "panel-edge");
    redraw(cx);
    assert!(
        near(width_of(cx, "detail-panel"), 480.),
        "a double-click resets it"
    );
    assert!(
        cx.debug_bounds("panel-float").is_none(),
        "beside the content"
    );
}

#[gpui_kit::test]
fn a_narrow_window_floats_the_panel_over_the_content(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    resize(cx, 800., 700.);
    open_detail(cx);
    assert!(
        cx.debug_bounds("panel-float").is_some(),
        "no room beside it"
    );
    let content = cx.debug_bounds("content-box").unwrap();
    assert!(
        content.right() >= gpui_kit::px(780.),
        "the content keeps its width under the panel: {content:?}"
    );
    let panel = cx.debug_bounds("detail-panel").unwrap();
    assert_eq!(
        panel.right(),
        gpui_kit::px(800.),
        "which is at the window's edge"
    );
}

#[gpui_kit::test]
fn the_layout_and_each_views_display_come_back_after_a_restart(cx: &mut TestAppContext) {
    let dir = project();
    let state = tempfile::tempdir().unwrap();
    let path = state.path().join("livtk/app.toml");
    {
        let (app, cx) = open_app(cx, dir.path());
        resize(cx, 1600., 900.);
        app.update_in(cx, |a, window, cx| a.remember(path.clone(), window, cx));
        drag_to(cx, "sidebar-edge", 300.);
        cx.simulate_keystrokes("g t");
        open_detail(cx);
        drag_to(cx, "panel-edge", 1040.);
        cx.simulate_keystrokes("secondary-j");
        settle(cx);
        // 900 tall, 8 below the bar: its top at 600 makes it 292.
        drag_down_to(cx, "bottom-edge", 600.);
        cx.simulate_keystrokes("secondary-b");
        redraw(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(600));
        cx.run_until_parked();
    }
    let saved = livtk_app::model::Saved::load(&path);
    assert_eq!(saved.view.as_deref(), Some("tasks"), "{saved:?}");
    assert_eq!(saved.window.sidebar_width, Some(300.));
    assert!(saved.window.sidebar_collapsed);
    assert_eq!(saved.window.panel_width, Some(560.));
    assert!(saved.window.bottom_open);
    assert_eq!(saved.window.bottom_height, Some(292.));

    let (app, cx) = open_window(cx, dir.path());
    resize(cx, 1600., 900.);
    app.update_in(cx, |a, window, cx| a.remember(path.clone(), window, cx));
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Tasks));
    assert!(cx.debug_bounds("rail").is_some(), "still collapsed");
    cx.simulate_keystrokes("secondary-b");
    redraw(cx);
    assert!(near(width_of(cx, "sidebar"), 300.), "at the width it was");
    open_detail(cx);
    assert!(
        near(width_of(cx, "detail-panel"), 560.),
        "and the panel too"
    );
    let bar = cx.debug_bounds("bottom-bar").expect("the bottom bar, open");
    assert!(
        near(f32::from(bar.size.height), 292.),
        "at its height: {bar:?}"
    );
}

/// The bottom bar, by ⌘J or the top bar's button: under the content box, which keeps the dock
/// above it; gone again the same way.
#[gpui_kit::test]
fn the_bottom_bar_sits_under_the_content_and_the_dock_stays_above_it(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("g t");
    redraw(cx);
    assert!(cx.debug_bounds("bottom-bar").is_none(), "shut at first");
    cx.simulate_keystrokes("secondary-j");
    settle(cx);
    let bar = cx.debug_bounds("bottom-bar").expect("open");
    let content = cx.debug_bounds("content-box").unwrap();
    let dock = cx.debug_bounds("dock").unwrap();
    assert!(content.bottom() <= bar.top(), "under the content box");
    assert!(dock.bottom() <= content.bottom(), "the dock stays in it");
    assert!(
        bar.left() >= content.left() && bar.right() <= content.right() + gpui_kit::px(1.),
        "between the sidebar and the panel: {bar:?}"
    );
    click_on(cx, "toggle-bottom");
    settle(cx);
    assert!(
        cx.debug_bounds("bottom-bar").is_none(),
        "shut again, once it slid away"
    );
}

#[gpui_kit::test]
fn scrolling_over_the_shortcuts_leaves_the_list_behind_them_still(cx: &mut TestAppContext) {
    let dir = long_project(40);
    let (_app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    redraw(cx);
    let before = cx.debug_bounds("key-LIV-5").expect("a row is drawn");
    cx.simulate_keystrokes("secondary-?");
    redraw(cx);
    cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position: before.center(),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
            gpui_kit::px(0.),
            gpui_kit::px(-400.),
        )),
        ..Default::default()
    });
    redraw(cx);
    assert_eq!(
        cx.debug_bounds("key-LIV-5"),
        Some(before),
        "the rows behind the modal did not move"
    );
}

#[gpui_kit::test]
fn a_checkout_is_caught_up_in_the_background_and_shown(cx: &mut TestAppContext) {
    let dir = project();
    let log = dir.path().join(".livtk/events.jsonl");
    let checkpoint = std::fs::read(&log).unwrap();
    let mut ws = Workspace::open(dir.path()).unwrap();
    let third = ws
        .execute(
            Command::Create {
                kind: Kind::Task,
                title: "third".into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &CLAUDE,
        )
        .unwrap();
    ws.execute(
        Command::Move {
            node: third[0].op.subjects().0.to_string(),
            to: State::Planned,
        },
        &CLAUDE,
    )
    .unwrap();
    let (app, cx) = open_app(cx, dir.path());
    assert!(
        cx.debug_bounds("key-LIV-3").is_some(),
        "the third task waits"
    );

    std::fs::write(&log, &checkpoint).unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(600));
    redraw(cx);
    assert!(
        cx.debug_bounds("key-LIV-3").is_none(),
        "after the checkout it is gone"
    );
    assert!(cx.debug_bounds("key-LIV-1").is_some());
    assert!(cx.debug_bounds("catching-up-0").is_none(), "and caught up");
    app.read_with(cx, |a, _| assert!(!a.model().projects[0].catching_up));

    app.update(cx, |a, _| a.model_mut().projects[0].catching_up = true);
    redraw(cx);
    assert!(
        cx.debug_bounds("catching-up-0").is_some(),
        "while it catches up, the sidebar says so"
    );
}

fn move_to(cx: &mut gpui_kit::VisualTestContext, x: f32, y: f32) {
    cx.simulate_event(gpui_kit::MouseMoveEvent {
        position: gpui_kit::point(gpui_kit::px(x), gpui_kit::px(y)),
        modifiers: gpui_kit::Modifiers::default(),
        pressed_button: None,
    });
    redraw(cx);
}

fn centre(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) -> (f32, f32) {
    let c = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"))
        .center();
    (f32::from(c.x), f32::from(c.y))
}

#[gpui_kit::test]
fn ctrl_enter_in_the_new_tasks_body_creates_it(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("n");
    cx.run_until_parked();
    cx.simulate_input("with a plan");
    click_on(cx, "new-task-body");
    cx.simulate_input("goal: ship it");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("check: it runs");
    cx.simulate_keystrokes("secondary-enter");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert!(!a.form_open(), "created, the form closed")
    });
    let ws = Workspace::open(dir.path()).unwrap();
    let created = ws
        .tasks(&[])
        .unwrap()
        .into_iter()
        .find(|t| t.title == "with a plan")
        .expect("created");
    assert_eq!(
        ws.node(&created.key).unwrap().body,
        "goal: ship it\ncheck: it runs",
        "Enter made a line; Ctrl+Enter only created it"
    );
}

/// Show a notice, as an action that failed does.
fn warn(app: &gpui_kit::Entity<LivtkApp>, cx: &mut gpui_kit::VisualTestContext, text: &str) {
    app.update(cx, |a, cx| {
        a.model_mut().warn(text);
        cx.notify();
    });
    redraw(cx);
}

const NOTICE_FADE: Duration = Duration::from_millis(240);
/// How long an info notification stays.
const NOTICE_FOR: Duration = Duration::from_secs(6);

fn inform(app: &gpui_kit::Entity<LivtkApp>, cx: &mut gpui_kit::VisualTestContext, text: &str) {
    app.update(cx, |a, cx| {
        a.model_mut().inform(text);
        cx.notify();
    });
    redraw(cx);
}

#[gpui_kit::test]
fn the_notice_closes_with_its_x(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    warn(&app, cx, "LIV-1 waits on LIV-2");
    assert!(cx.debug_bounds("notice").is_some(), "it is shown");

    click_on(cx, "notice-close");
    redraw(cx);
    app.read_with(cx, |a, _| assert!(a.model().notice().is_none()));
    assert!(cx.debug_bounds("notice").is_none(), "and no longer drawn");
}

#[gpui_kit::test]
fn the_notice_stays_while_the_pointer_is_over_it(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    inform(&app, cx, "LIV-1 waits on LIV-2");
    let (x, y) = centre(cx, "notice");
    move_to(cx, x, y);
    app.update(cx, |a, _| {
        assert!(
            !a.model_mut()
                .age_notices(Instant::now() + NOTICE_FOR * 10, NOTICE_FADE),
            "held by the pointer, its time does not run out"
        )
    });
    redraw(cx);
    assert!(cx.debug_bounds("notice").is_some(), "still shown");

    // A key pressed with the pointer resting on it lets nothing go.
    cx.simulate_keystrokes("j");
    redraw(cx);
    app.update(cx, |a, _| {
        assert!(
            !a.model_mut()
                .age_notices(Instant::now() + NOTICE_FOR * 10, NOTICE_FADE),
            "still held after a key press"
        )
    });

    // The pointer leaves: its time starts again, and then it goes.
    move_to(cx, 10., 10.);
    let left = Instant::now();
    app.update(cx, |a, cx| {
        let m = a.model_mut();
        assert!(!m.age_notices(left + NOTICE_FOR / 2, NOTICE_FADE));
        assert!(m.age_notices(left + NOTICE_FOR, NOTICE_FADE), "it leaves");
        assert!(m.age_notices(left + NOTICE_FOR + NOTICE_FADE, NOTICE_FADE));
        cx.notify();
    });
    redraw(cx);
    assert!(cx.debug_bounds("notice").is_none(), "gone");
}

/// A notice raised by the poll itself, not by anything the human did, shows on that poll.
#[cfg(unix)]
#[gpui_kit::test]
fn a_failure_found_by_the_poll_shows_at_once(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    let log = dir.path().join(".livtk/events.jsonl");
    let mode = |m| std::fs::set_permissions(&log, std::fs::Permissions::from_mode(m)).unwrap();
    // Its length unchanged, the log is read to see whether it changed, and cannot be.
    mode(0o000);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.run_until_parked();
    mode(0o644);
    app.read_with(cx, |a, _| assert!(a.model().notice().is_some(), "told"));
    assert!(
        cx.debug_bounds("notice").is_some(),
        "and drawn on that poll, not when it starts to leave"
    );
}

#[gpui_kit::test]
fn view_options_open_upwards_and_turn_empty_groups_and_dismissed_work_on(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    cx.run_until_parked();
    let button = cx
        .debug_bounds("dock-view")
        .expect("view options in the dock");
    click_on(cx, "dock-view");
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("view")));
    let menu = cx.debug_bounds("view-menu").expect("its menu");
    assert!(
        menu.bottom() <= button.top(),
        "it opens upwards: {menu:?} over {button:?}"
    );

    click_on(cx, "view-show-empty");
    app.read_with(cx, |a, _| {
        assert!(a.model().display().show_empty);
        assert_eq!(
            a.model().groups().len(),
            7,
            "a band for every task state not dismissed"
        );
        assert_eq!(
            a.menu_open(),
            Some("view"),
            "an option leaves the menu open"
        );
    });
    click_on(cx, "view-show-dismissed");
    app.read_with(cx, |a, _| {
        assert!(a.model().display().show_dismissed);
        assert_eq!(a.model().groups().len(), 8, "and canceled");
    });

    // Closed, `s d` still shows and hides dismissed work.
    click_on(cx, "dock-view");
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), None));
    cx.simulate_keystrokes("s d");
    app.read_with(cx, |a, _| {
        assert!(!a.model().display().show_dismissed);
        assert_eq!(a.model().groups().len(), 7);
    });
}

#[gpui_kit::test]
fn view_options_choose_list_or_board_above_a_line_and_keys_set_each_option(
    cx: &mut TestAppContext,
) {
    use livtk_app::model::Layout;
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    cx.run_until_parked();
    click_on(cx, "dock-view");
    let list = cx
        .debug_bounds("view-layout-list")
        .expect("List, in the menu");
    let board = cx.debug_bounds("view-layout-board").expect("Board");
    let line = cx.debug_bounds("view-menu-divider").expect("one line");
    let empty = cx.debug_bounds("view-show-empty").unwrap();
    let dismissed = cx.debug_bounds("view-show-dismissed").unwrap();
    assert!(
        list.bottom() <= board.top()
            && board.bottom() <= line.top()
            && line.bottom() <= empty.top()
            && empty.bottom() <= dismissed.top(),
        "List, Board, the line, then the options: {list:?} {board:?} {line:?} {empty:?}"
    );

    click_on(cx, "view-layout-board");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().layout(), Layout::Board);
        assert_eq!(a.menu_open(), Some("view"), "a choice leaves the menu open");
    });
    click_on(cx, "view-layout-list");
    app.read_with(cx, |a, _| assert_eq!(a.model().layout(), Layout::List));
    click_on(cx, "dock-view");
    cx.simulate_keystrokes("b");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().layout(), Layout::Board, "b still switches")
    });
    cx.simulate_keystrokes("b s e");
    app.read_with(cx, |a, _| {
        assert!(a.model().display().show_empty, "s e shows empty groups");
        assert!(!a.model().display().show_dismissed, "and nothing else");
    });
    cx.simulate_keystrokes("s d");
    app.read_with(cx, |a, _| {
        assert!(a.model().display().show_dismissed, "h shows dismissed work");
        assert!(a.model().display().show_empty);
    });
    cx.simulate_keystrokes("s e");
    app.read_with(cx, |a, _| assert!(!a.model().display().show_empty));

    // Lessons have no board: their menu is the options alone, with no line.
    cx.simulate_keystrokes("g l");
    cx.run_until_parked();
    click_on(cx, "dock-view");
    assert!(cx.debug_bounds("view-show-empty").is_some());
    for gone in ["view-layout-list", "view-layout-board", "view-menu-divider"] {
        assert!(cx.debug_bounds(gone).is_none(), "{gone}");
    }
}

#[gpui_kit::test]
fn the_board_shows_its_empty_columns_only_with_empty_groups(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t b");
    cx.run_until_parked();
    let column = |cx: &mut gpui_kit::VisualTestContext, s: &str| {
        cx.debug_bounds(named(format!("column-{s}"))).is_some()
    };
    assert!(column(cx, "planned"), "the planned work has its column");
    assert!(!column(cx, "review"), "an empty column is hidden");

    cx.simulate_keystrokes("s e");
    redraw(cx);
    assert!(column(cx, "review"), "s e brings every column back");
    cx.simulate_keystrokes("s e");
    redraw(cx);
    assert!(!column(cx, "review"));

    // Nothing matches: no columns, and the board says so.
    app.update(cx, |a, cx| {
        a.model_mut().set_filter("nothing matches this");
        cx.notify();
    });
    redraw(cx);
    assert!(cx.debug_bounds("board-empty").is_some());
}

/// The app's own shortcuts are the platform's: Cmd on macOS, where Ctrl does nothing of the sort,
/// and Ctrl elsewhere. The palette's Ctrl+N and Ctrl+P stay Control everywhere.
#[cfg(target_os = "macos")]
#[gpui_kit::test]
fn on_macos_the_shortcuts_are_cmd_not_ctrl(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("ctrl-k");
    app.read_with(cx, |a, _| {
        assert!(!a.palette_open(), "Ctrl+K is not the palette")
    });
    cx.simulate_keystrokes("cmd-k");
    app.read_with(cx, |a, _| assert!(a.palette_open(), "Cmd+K is"));
    cx.simulate_keystrokes("escape ctrl-?");
    app.read_with(cx, |a, _| assert!(!a.help_open()));
    cx.simulate_keystrokes("cmd-?");
    app.read_with(cx, |a, _| {
        assert!(a.help_open(), "Cmd+? shows the shortcuts")
    });
}

#[gpui_kit::test]
fn the_filter_menu_narrows_by_state(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    cx.run_until_parked();
    click_on(cx, "filter-button");
    for st in ["doing", "review", "planned", "backlog", "done"] {
        assert!(
            cx.debug_bounds(named(format!("filter-state-{st}")))
                .is_some(),
            "{st} is offered"
        );
    }
    assert!(
        cx.debug_bounds("filter-state-canceled").is_none(),
        "not a dismissed state while dismissed work is hidden"
    );
    click_on(cx, "filter-state-backlog");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().display().states, [State::Backlog]);
        assert!(a.model().rows().is_empty(), "both tasks are planned");
        assert_eq!(a.menu_open(), Some("filter"), "a box leaves the menu open");
    });
    click_on(cx, "clear-filters");
    app.read_with(cx, |a, _| {
        assert!(a.model().display().states.is_empty());
        assert_eq!(a.model().rows().len(), 2);
    });

    // The inbox mixes kinds: it has no states to filter by.
    cx.simulate_keystrokes("escape g i");
    cx.run_until_parked();
    click_on(cx, "filter-button");
    assert!(cx.debug_bounds("filter-state-planned").is_none());
}

/// While the window is away, what Claude puts in the inbox is told by a system notification; a
/// click on it opens the inbox there. In front, nothing is posted.
#[gpui_kit::test]
fn what_arrives_while_the_window_is_away_is_a_system_notification(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    let poll = |cx: &mut gpui_kit::VisualTestContext| {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(250));
        cx.run_until_parked();
    };
    let plan = |title: &str| {
        let mut ws = Workspace::open(dir.path()).unwrap();
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
        let key = ws.node(&e[0].op.subjects().0.to_string()).unwrap().key;
        ws.execute(
            Command::Move {
                node: key.clone(),
                to: State::Planned,
            },
            &CLAUDE,
        )
        .unwrap();
        key
    };

    cx.update(|window, cx| {
        cx.set_app_identity(livtk_app::ui::APP_ID, livtk_app::ui::APP_NAME);
        window.activate_window();
    });
    cx.run_until_parked();
    assert!(
        cx.update(|window, _| window.is_window_active()),
        "the window is in front"
    );
    let front = plan("seen in front");
    poll(cx);
    assert!(
        cx.shown_system_notifications().is_empty(),
        "with the window in front, nothing is posted"
    );
    // It is told in the window instead, with a way to it.
    app.read_with(cx, |a, _| {
        let t = a.model().notice().expect("told in the window");
        assert_eq!(
            t.title.as_deref(),
            Some(format!("{front} waits for you").as_str())
        );
    });
    redraw(cx);
    cx.simulate_keystrokes("g t");
    click_on(cx, "notice-act-0");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Inbox, "Open goes to the inbox");
        assert_eq!(
            a.model().selected().map(|r| r.key.clone()),
            Some(front.clone())
        );
    });

    cx.deactivate_window();
    let key = plan("while away");
    poll(cx);
    let shown = cx.shown_system_notifications();
    let last = shown.last().expect("a notification");
    assert!(last.tag.starts_with("livtk-inbox"), "{}", last.tag);
    assert_eq!(last.title.to_string(), format!("{key} waits for you"));
    assert_eq!(last.body.to_string(), "while away");

    let tag = last.tag.clone();
    cx.simulate_keystrokes("g t");
    cx.simulate_system_notification_response(gpui_kit::SystemNotificationResponse {
        tag,
        action_id: None,
    });
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Inbox, "a click opens the inbox");
        assert_eq!(
            a.model().selected().map(|r| r.key.clone()),
            Some(key.clone())
        );
    });

    // Plans that come a poll apart are two notifications, each of what is new, and both stay:
    // a click on the older still opens what it told of.
    cx.deactivate_window();
    let first = plan("first of two");
    poll(cx);
    let second = plan("second of two");
    poll(cx);
    let shown = cx.shown_system_notifications();
    let [older, newer] = &shown[shown.len() - 2..] else {
        unreachable!()
    };
    assert_eq!(older.title.to_string(), format!("{first} waits for you"));
    assert_eq!(newer.title.to_string(), format!("{second} waits for you"));
    let delivered = cx.delivered_system_notifications();
    assert!(
        delivered.iter().any(|n| n.tag == older.tag)
            && delivered.iter().any(|n| n.tag == newer.tag),
        "neither replaced the other"
    );
    cx.simulate_system_notification_response(gpui_kit::SystemNotificationResponse {
        tag: older.tag.clone(),
        action_id: None,
    });
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.model().selected().map(|r| r.key.clone()),
            Some(first.clone())
        );
    });
}

/// A panel floating over the content on a narrow window is the panel's, presses and all: its ×
/// closes it and never reaches the titlebar strip under it, which would drag the window (and the
/// test window panics when asked to move).
#[gpui_kit::test]
fn the_floating_panels_close_button_closes_it_and_drags_nothing(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    resize(cx, 800., 700.);
    cx.simulate_keystrokes("g t");
    open_detail(cx);
    assert!(cx.debug_bounds("panel-float").is_some(), "it floats");
    click_on(cx, "close-detail");
    app.read_with(cx, |a, _| assert!(a.model().detail.is_none(), "closed"));
}

/// Plan a task as Claude in the project at `dir`; its key.
fn plan_as_claude(dir: &std::path::Path, title: &str) -> String {
    let mut ws = Workspace::open(dir).unwrap();
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
    let key = ws.node(&e[0].op.subjects().0.to_string()).unwrap().key;
    ws.execute(
        Command::Move {
            node: key.clone(),
            to: State::Planned,
        },
        &CLAUDE,
    )
    .unwrap();
    key
}

/// A notification from one run never takes the tag of one from an earlier run, which the system
/// may still list: a tag in use replaces what was posted under it.
#[gpui_kit::test]
fn notifications_of_two_runs_never_share_a_tag(cx: &mut TestAppContext) {
    let dir = project();
    let mut tags = Vec::new();
    for run in 0..2 {
        let (_app, cx) = if run == 0 {
            open_app(cx, dir.path())
        } else {
            open_window(cx, dir.path())
        };
        cx.update(|_, cx| cx.set_app_identity(livtk_app::ui::APP_ID, livtk_app::ui::APP_NAME));
        cx.deactivate_window();
        plan_as_claude(dir.path(), &format!("run {run}"));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(250));
        cx.run_until_parked();
        let shown = cx.shown_system_notifications();
        tags.push(shown.last().expect("a notification").tag.clone());
    }
    assert_ne!(tags[0], tags[1], "each run numbers its own: {tags:?}");
}

/// Esc in the note field gives the keys back to the list, keeping the draft with its own task: it
/// never follows the selection to another. A second Esc, in the list, closes the panel. Fields
/// that do more with Esc keep doing it.
#[gpui_kit::test]
fn esc_in_the_note_field_gives_the_keys_back_and_keeps_the_draft(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("c");
    cx.run_until_parked();
    cx.simulate_input("half a thought");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert!(
            a.model().detail.is_some(),
            "the panel stays: only the field let go"
        )
    });

    cx.simulate_keystrokes("j");
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.model().selected().map(|r| r.key.clone()).as_deref(),
            Some("LIV-2"),
            "j moves the selection: the keys are the list's again"
        )
    });
    assert_eq!(
        state_of(dir.path(), "LIV-1"),
        State::Planned,
        "nothing typed ran a command"
    );
    let last_of = |key: &str| {
        Workspace::open(dir.path())
            .unwrap()
            .history(Some(key))
            .unwrap()
            .pop()
            .unwrap()
            .op
    };
    let noted = |key: &str, text: &str| matches!(last_of(key), livtk_core::event::Op::NodeNote { data, .. } if data.text == text);

    // LIV-2's note field is its own: empty, so Enter adds nothing there.
    cx.simulate_keystrokes("c");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter escape k");
    cx.run_until_parked();
    assert!(
        !matches!(last_of("LIV-2"), livtk_core::event::Op::NodeNote { .. }),
        "the draft did not follow the selection"
    );

    // Back on LIV-1, its draft is there: Enter adds it as LIV-1's note.
    cx.simulate_keystrokes("c");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(noted("LIV-1", "half a thought"), "{:?}", last_of("LIV-1"));

    // Still in the field after Enter: Esc gives the keys back, and a second, in the list,
    // closes the panel, as it always has.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert!(a.model().detail.is_some(), "first the field lets go")
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert!(a.model().detail.is_none(), "then the panel closes")
    });

    // The filter's Esc still closes its menu.
    cx.simulate_keystrokes("g t /");
    cx.run_until_parked();
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("filter")));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), None));
}

/// The menu bar's keys, from anywhere, typing in a field included: a new task. Views have no
/// Cmd+number (the developer's choice): from a field, Cmd+3 changes nothing.
#[gpui_kit::test]
fn the_menu_bars_keys_work_from_anywhere(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("c");
    cx.run_until_parked();
    cx.simulate_keystrokes("secondary-3");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Inbox, "no view by number")
    });
    cx.simulate_keystrokes("secondary-n");
    app.read_with(cx, |a, _| assert!(a.form_open(), "Cmd+N opens a new task"));
}

#[gpui_kit::test]
fn about_says_which_build_this_is(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    cx.dispatch_action(livtk_app::ui::About);
    cx.run_until_parked();
    assert!(cx.debug_bounds("about").is_some(), "About opens");
    assert!(cx.debug_bounds("about-build").is_some());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("about").is_none(), "Esc closes it");
}

#[test]
fn the_menus_tick_what_is_on() {
    use livtk_app::ui::menus::{MenuState, menu_bar};
    let state = MenuState {
        projects: vec!["tasks".into(), "other".into()],
        current: 1,
        view: 2,
        board: true,
        show_empty: true,
        show_dismissed: false,
        inbox: 3,
        update: Some(15),
    };
    let bar = menu_bar(&state);
    let ticked = |menu: &str, item: &str| {
        let m = bar.iter().find(|m| m.name.as_ref() == menu).unwrap();
        m.items.iter().find_map(|i| match i {
            gpui_kit::MenuItem::Action { name, checked, .. } if name.as_ref() == item => {
                Some(*checked)
            }
            gpui_kit::MenuItem::Submenu(sub) => sub.items.iter().find_map(|i| match i {
                gpui_kit::MenuItem::Action { name, checked, .. } if name.as_ref() == item => {
                    Some(*checked)
                }
                _ => None,
            }),
            _ => None,
        })
    };
    assert_eq!(ticked("View", "Tasks"), Some(true));
    assert_eq!(ticked("View", "Inbox"), Some(false));
    assert_eq!(ticked("View", "Board"), Some(true));
    assert_eq!(ticked("View", "Show Empty Groups"), Some(true));
    assert_eq!(ticked("View", "Show Dismissed Work"), Some(false));
    assert_eq!(ticked("File", "other"), Some(true), "the open project");
    assert_eq!(ticked("File", "tasks"), Some(false));
    assert_eq!(
        bar.iter().map(|m| m.name.to_string()).collect::<Vec<_>>(),
        ["livtk", "File", "Edit", "View", "Task", "Window", "Help"]
    );
    assert!(
        ticked("livtk", "Restart to Update (build 15)").is_some(),
        "offered once a build is staged"
    );
    let calm = menu_bar(&MenuState {
        update: None,
        ..state.clone()
    });
    let livtk = calm.iter().find(|m| m.name.as_ref() == "livtk").unwrap();
    assert!(
        !livtk.items.iter().any(|i| matches!(
            i,
            gpui_kit::MenuItem::Action { name, .. } if name.starts_with("Restart to Update")
        )),
        "and only then"
    );
}

/// Two projects that share node ids (two clones of one repository) never share a note's draft.
#[gpui_kit::test]
fn a_draft_stays_in_its_own_project(cx: &mut TestAppContext) {
    let a = project();
    let b = tempfile::tempdir().unwrap();
    // A clone of the same log: the same node ids, the same keys.
    std::fs::create_dir_all(b.path().join(".livtk")).unwrap();
    for f in ["config.toml", "events.jsonl"] {
        std::fs::copy(
            a.path().join(".livtk").join(f),
            b.path().join(".livtk").join(f),
        )
        .unwrap();
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        bind_keys(cx);
    });
    let (pa, pb) = (a.path().to_path_buf(), b.path().to_path_buf());
    let (_app, cx) = cx.add_window_view(|window, cx| {
        let mut model = Model::open(&[pa, pb]);
        model.set_models_dir(None);
        LivtkApp::new(model, window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("c");
    cx.run_until_parked();
    cx.simulate_input("for project a");
    cx.simulate_keystrokes("escape ]");
    cx.run_until_parked();
    cx.simulate_keystrokes("c");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let notes = |dir: &std::path::Path| {
        Workspace::open(dir)
            .unwrap()
            .history(Some("LIV-1"))
            .unwrap()
            .into_iter()
            .filter(|e| matches!(e.op, livtk_core::event::Op::NodeNote { .. }))
            .count()
    };
    assert_eq!(
        notes(b.path()),
        0,
        "project b's LIV-1 had no draft of its own"
    );
    cx.simulate_keystrokes("escape [");
    cx.run_until_parked();
    cx.simulate_keystrokes("c");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        notes(a.path()),
        1,
        "project a's draft came back and went to a"
    );
}

/// The shortcuts and About have the keys while shown: a key meant for the list never acts on the
/// task behind them, and Esc closes them.
#[gpui_kit::test]
fn the_shortcuts_and_about_keep_the_keys_from_the_list(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    let before = state_of(dir.path(), "LIV-1");
    cx.dispatch_action(livtk_app::ui::About);
    cx.run_until_parked();
    cx.simulate_keystrokes("a 2 g t");
    cx.run_until_parked();
    assert_eq!(state_of(dir.path(), "LIV-1"), before, "nothing decided");
    assert_eq!(priority_of(dir.path(), "LIV-1"), 3, "no priority set");
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Inbox));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("about").is_none());

    cx.simulate_keystrokes("secondary-?");
    cx.run_until_parked();
    cx.simulate_keystrokes("a");
    cx.run_until_parked();
    assert_eq!(state_of(dir.path(), "LIV-1"), before, "nothing decided");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.simulate_keystrokes("a");
    cx.run_until_parked();
    assert_ne!(
        state_of(dir.path(), "LIV-1"),
        before,
        "the list has the keys again"
    );
}

/// Cmd+N from the palette opens the form in its place; from the form, it keeps what is typed.
#[gpui_kit::test]
fn new_task_replaces_the_palette_and_keeps_a_half_typed_form(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("secondary-k");
    cx.run_until_parked();
    cx.simulate_keystrokes("secondary-n");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert!(a.form_open());
        assert!(!a.palette_open(), "the palette makes way");
    });
    cx.simulate_input("keep me");
    cx.simulate_keystrokes("secondary-n");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let ws = Workspace::open(dir.path()).unwrap();
    let titles: Vec<String> = ws
        .list(Kind::Task, &[])
        .unwrap()
        .into_iter()
        .map(|r| r.title)
        .collect();
    assert!(titles.contains(&"keep me".to_owned()), "{titles:?}");
}

/// The Dock's items work with the window hidden or behind another app, when no window hears an
/// action: the app's own handlers bring the window forward and act in it.
#[gpui_kit::test]
fn the_docks_items_work_with_no_window_active(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    cx.deactivate_window();
    let available = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_, cx| cx.is_action_available(&livtk_app::ui::GoInbox))
    };
    assert!(!available(cx), "with no window active, nobody hears it");
    // As the Dock sends them: from the app, not from inside the window (which a test's
    // `update` is), so deferred.
    let window = gpui_kit::VisualContext::window_handle(cx);
    let weak = app.downgrade();
    cx.update(|_, cx| livtk_app::ui::dock_actions(window, weak, cx));
    assert!(available(cx), "the Dock's item is not greyed");
    cx.update(|_, cx| cx.defer(|cx| cx.dispatch_action(&livtk_app::ui::GoInbox)));
    cx.run_until_parked();
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Inbox));
    cx.deactivate_window();
    cx.update(|_, cx| cx.defer(|cx| cx.dispatch_action(&livtk_app::ui::NewTask)));
    cx.run_until_parked();
    app.read_with(cx, |a, _| assert!(a.form_open()));
}

/// The panel's Status opens the same menu as a row's state icon, as its Priority does, and the
/// menu fits inside the panel.
#[gpui_kit::test]
fn the_panels_status_opens_the_state_menu(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    open_detail(cx);
    let key = app.read_with(cx, |a, _| a.model().selected().unwrap().key.clone());
    click_on(cx, "detail-state");
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("detail-state")));
    let panel = cx.debug_bounds("detail-body-panel").unwrap();
    let menu = cx.debug_bounds("set-state-menu").unwrap();
    assert!(
        menu.left() >= panel.left() && menu.right() <= panel.right(),
        "menu {menu:?} inside the panel {panel:?}"
    );
    click_on(cx, "set-state-acked");
    assert_eq!(state_of(dir.path(), &key), State::Acked);
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), None));
}

/// The shortcuts and About open over a half-typed task and close back to it, its text kept and
/// its title typing again; a focus lost under them never hands the keys to the list.
#[gpui_kit::test]
fn the_shortcuts_and_about_keep_a_half_typed_task(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    let before = state_of(dir.path(), "LIV-1");
    cx.simulate_keystrokes("n");
    cx.run_until_parked();
    cx.simulate_input("half");
    cx.simulate_keystrokes("secondary-?");
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert!(a.form_open(), "kept under the shortcuts")
    });
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("a");
    cx.run_until_parked();
    assert_eq!(
        state_of(dir.path(), "LIV-1"),
        before,
        "nothing behind them acts"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.dispatch_action(livtk_app::ui::About);
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    app.read_with(cx, |a, _| assert!(a.form_open(), "kept under About"));
    cx.simulate_input(" typed");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let ws = Workspace::open(dir.path()).unwrap();
    let titles: Vec<String> = ws
        .list(Kind::Task, &[])
        .unwrap()
        .into_iter()
        .map(|r| r.title)
        .collect();
    assert!(titles.contains(&"half typed".to_owned()), "{titles:?}");
}

/// The palette opened over About comes out on top, with the keys; About gives way.
#[gpui_kit::test]
fn the_palette_opens_on_top_of_about(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.dispatch_action(livtk_app::ui::About);
    cx.run_until_parked();
    cx.simulate_keystrokes("secondary-k");
    cx.run_until_parked();
    assert!(cx.debug_bounds("about").is_none(), "About gives way");
    app.read_with(cx, |a, _| assert!(a.palette_open()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    app.read_with(cx, |a, _| assert!(!a.palette_open(), "one Esc closes it"));
}

/// A day clicked in the activity grid (in the bottom bar) shows that day in the feed; Esc shows
/// every day again.
#[gpui_kit::test]
fn a_day_clicked_in_the_grid_filters_the_feed_and_esc_clears_it(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    // The grid is in the bottom bar.
    cx.simulate_keystrokes("g a secondary-j");
    settle(cx);
    let today = livtk_app::model::grid::today();
    // Today's square, by the grid's geometry: 8px of room, then 18px a week or a weekday, the
    // months' row 28px above; 6px into the 13px square.
    let (w, r) = app.read_with(cx, |a, _| a.model().grid.drawn(today).at(today).unwrap());
    let origin = cx
        .debug_bounds("grid-squares")
        .expect("the grid is drawn")
        .origin;
    let at = origin
        + gpui_kit::point(
            gpui_kit::px(8. + w as f32 * 18. + 6.),
            gpui_kit::px(28. + r as f32 * 18. + 6.),
        );
    cx.simulate_click(at, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().grid.pick.span, Some((today, today)));
        assert!(!a.model().activity.is_empty(), "today's events");
    });
    assert!(
        cx.debug_bounds("grid-unpick").is_some(),
        "the pick is shown"
    );
    cx.simulate_keystrokes("escape");
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.model().grid.pick.span, None));
}

/// The terminal and the tasks side by side: Cmd-\ splits a placeholder beside the terminal, a
/// view picked there fills it; the keys are the focused pane's alone; a press in the other pane
/// focuses it; the next pane's key goes round; Cmd-Alt-W closes a pane, never the last.
#[gpui_kit::test]
fn panes_split_hold_a_view_each_and_the_keys_are_the_focused_ones(cx: &mut TestAppContext) {
    let dir = project();
    cx.update(|cx| cx.set_global(livtk_app::ui::terminal::Headless));
    let (app, cx) = open_app(cx, dir.path());
    resize(cx, 1600., 1000.);
    cx.simulate_keystrokes("g x");
    redraw(cx);
    cx.simulate_keystrokes("secondary-\\");
    redraw(cx);
    assert!(
        cx.debug_bounds("placeholder").is_some(),
        "a placeholder beside"
    );
    assert!(
        cx.debug_bounds("dock").is_none(),
        "no dock: nothing to shape yet"
    );
    assert!(
        cx.debug_bounds("terminal").is_some(),
        "the terminal still drawn"
    );
    click_on(cx, "place-tasks");
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Tasks);
        assert_eq!(a.model().panes().len(), 2);
    });
    // J moves the tasks' selection; the shell hears nothing.
    let before = app.read_with(cx, |a, _| a.model().selected().map(|r| r.key.clone()));
    cx.simulate_keystrokes("j");
    redraw(cx);
    let after = app.read_with(cx, |a, _| a.model().selected().map(|r| r.key.clone()));
    assert_ne!(before, after, "the focused list moved");
    cx.update(|window, cx| assert!(!app.read(cx).terminal_focused(window, cx)));
    // A press in the terminal's pane focuses it, and it has the keys.
    let terminal_pane = app.read_with(cx, |a, _| a.model().panes()[0].id);
    assert_eq!(terminal_pane, 0);
    click_on(cx, "pane-0");
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Terminal));
    cx.update(|window, cx| assert!(app.read(cx).terminal_focused(window, cx)));
    cx.simulate_keystrokes("g t");
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Terminal, "typed into the shell")
    });
    // The next pane: the tasks again; closed, the terminal is all there is, and stays.
    cx.simulate_keystrokes("secondary-alt-]");
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Tasks));
    cx.simulate_keystrokes("secondary-alt-w");
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().panes().len(), 1);
        assert_eq!(a.model().view(), View::Terminal);
    });
    cx.update(|window, cx| assert!(app.read(cx).terminal_focused(window, cx)));
    cx.simulate_keystrokes("secondary-alt-w");
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().panes().len(), 1, "never the last")
    });
}

/// Terminals the VS Code way: Cmd-T starts another, a list beside names them, a click shows
/// one, its × kills it, and with one left the list goes.
#[gpui_kit::test]
fn cmd_t_starts_another_terminal_listed_beside_and_its_cross_kills_it(cx: &mut TestAppContext) {
    let dir = project();
    cx.update(|cx| cx.set_global(livtk_app::ui::terminal::Headless));
    let (app, cx) = open_app(cx, dir.path());
    resize(cx, 1600., 1000.);
    // From the list: the first terminal, no list beside it yet.
    cx.simulate_keystrokes("secondary-t");
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Terminal));
    assert!(cx.debug_bounds("terminal-list").is_none(), "one: no list");
    cx.simulate_keystrokes("secondary-t");
    redraw(cx);
    assert!(cx.debug_bounds("terminal-list").is_some(), "two: listed");
    assert!(cx.debug_bounds("terminal-1").is_some());
    cx.update(|window, cx| {
        assert!(
            app.read(cx).terminal_focused(window, cx),
            "the new one has the keys"
        )
    });
    click_on(cx, "terminal-0");
    redraw(cx);
    cx.update(|window, cx| assert!(app.read(cx).terminal_focused(window, cx)));
    // Over the second, its × shows; pressed, it is gone, and so is the list.
    let line = cx.debug_bounds("terminal-1").unwrap();
    cx.simulate_mouse_move(line.center(), None, gpui_kit::Modifiers::default());
    redraw(cx);
    click_on(cx, "terminal-kill-1");
    redraw(cx);
    assert!(
        cx.debug_bounds("terminal-list").is_none(),
        "one left: no list"
    );
    assert!(cx.debug_bounds("terminal").is_some());
}
