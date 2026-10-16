//! The window with real key events, headless: what the keys do, and what they must not do.
//!
//! These tests find elements by debug selectors, which GPUI records only in debug builds: run
//! them without `--release`. Only the ignored benchmark is meant for release.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use gpui_kit::TestAppContext;
use livtk_app::model::{Grouping, Model, Ordering, View};
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
    cx.simulate_keystrokes("ctrl-?");
    app.read_with(cx, |a, _| assert!(a.help_open()));
    cx.simulate_keystrokes("escape");
    app.read_with(cx, |a, _| assert!(!a.help_open(), "Esc closes it"));
    cx.simulate_keystrokes("ctrl-shift-/");
    app.read_with(cx, |a, _| {
        assert!(a.help_open(), "Shift with / is the same key")
    });
    cx.simulate_keystrokes("ctrl-?");
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
fn grouping_and_ordering_are_menus_of_their_own_in_display(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    click_on(cx, "display-button");
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("display")));
    assert!(
        cx.debug_bounds("group-priority").is_none(),
        "grouping waits in its own menu"
    );

    click_on(cx, "grouping-select");
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("grouping")));
    click_on(cx, "group-priority");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().display().grouping, Grouping::Priority);
        assert_eq!(
            a.menu_open(),
            Some("display"),
            "a choice closes only its menu"
        );
    });

    click_on(cx, "ordering-select");
    click_on(cx, "order-created");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().display().ordering, Ordering::Created)
    });

    // Outside everything: an open submenu closes first, then Display.
    let outside = gpui_kit::point(gpui_kit::px(8.), gpui_kit::px(400.));
    click_on(cx, "ordering-select");
    cx.simulate_click(outside, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    app.read_with(cx, |a, _| assert_eq!(a.menu_open(), Some("display")));
    cx.simulate_click(outside, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    app.read_with(cx, |a, _| {
        assert_eq!(a.menu_open(), None, "outside closed it")
    });

    click_on(cx, "display-button");
    click_on(cx, "display-button");
    app.read_with(cx, |a, _| {
        assert_eq!(a.menu_open(), None, "its button closes it too")
    });
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
    cx.simulate_keystrokes("ctrl-a");
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
    cx.simulate_keystrokes("ctrl-enter");
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

#[gpui_kit::test]
fn the_detail_panel_slides_out_before_it_goes(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    open_detail(cx);
    let panel = cx.debug_bounds("detail-body-panel").expect("drawn");
    let window = cx.update(|window, _| window.viewport_size());
    assert!(
        panel.size.height >= window.height - gpui_kit::px(24.),
        "the panel {panel:?} runs the full height of the window {window:?}"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    app.read_with(cx, |a, _| assert!(a.model().detail.is_none()));
    assert!(
        cx.debug_bounds("detail-panel").is_some(),
        "still drawn while it animates out"
    );
    settle(cx);
    assert!(
        cx.debug_bounds("detail-panel").is_none(),
        "gone once it has"
    );
}

#[gpui_kit::test]
fn editing_keeps_the_shape_of_what_is_edited(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    open_detail(cx);
    let view = cx.debug_bounds("detail-title").expect("drawn");
    double_click_on(cx, "detail-title");
    let editor = cx.debug_bounds("title-editor").expect("editing");
    assert_eq!(
        (view.origin, view.size),
        (editor.origin, editor.size),
        "the title editor takes the title's place exactly"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    let view = cx.debug_bounds("detail-body").expect("drawn");
    click_on(cx, "detail-body");
    let editor = cx.debug_bounds("body-editor").expect("editing");
    assert_eq!(
        view.origin, editor.origin,
        "the body editor starts where the body was"
    );
    assert_eq!(view.size.width, editor.size.width);
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
    cx.simulate_keystrokes("ctrl-k");
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
    cx.simulate_keystrokes("ctrl-k");
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
    cx.simulate_keystrokes("ctrl-k");
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
    cx.simulate_keystrokes("ctrl-k");
    cx.simulate_keystrokes("ctrl-k");
    app.read_with(cx, |a, _| {
        assert!(!a.palette_open(), "and so does Ctrl+K again")
    });
}

#[gpui_kit::test]
fn a_click_runs_an_item_and_new_task_opens_the_form(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("ctrl-k");
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
    cx.simulate_keystrokes("ctrl-k");
    app.read_with(cx, |a, _| {
        assert!(
            !a.palette_open(),
            "the form keeps the palette out, so nothing typed is lost"
        )
    });
}

#[gpui_kit::test]
fn the_palette_field_sits_centred_in_its_row(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("ctrl-k");
    cx.run_until_parked();
    let row = cx.debug_bounds("palette-field").expect("drawn");
    let input = cx.debug_bounds("palette-input").expect("drawn");
    let above = input.top() - row.top();
    let below = row.bottom() - input.bottom();
    assert_eq!(above, below, "as much room above the field as below it");
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
        assert_eq!(a.model().view, View::Activity);
        assert_eq!(a.model().projects[0].unseen, 0, "looking clears the badge");
    });
    assert!(cx.debug_bounds("event-0").is_some(), "the lines are drawn");

    // Newest first: the lesson, then the second plan's move.
    cx.simulate_keystrokes("j");
    open_detail(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view, View::Activity, "the feed stays");
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
fn feed_lines_span_the_feed_as_list_rows_do(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g a");
    cx.run_until_parked();
    let feed = cx.debug_bounds("feed").expect("drawn");
    let line = cx.debug_bounds("event-0").expect("drawn");
    assert_eq!(
        (line.left(), line.size.width),
        (feed.left(), feed.size.width),
        "a line runs the feed's whole width"
    );
}

#[gpui_kit::test]
fn the_keys_still_work_when_focus_was_lost(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    // Focus can end up nowhere: an input that blurs itself, a panel that closed under it.
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-?");
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
    cx.simulate_keystrokes("ctrl-?");
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
        assert_eq!(a.model().view, View::Graph);
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
        assert_eq!(a.model().view, View::Graph, "the graph stays");
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
        assert_eq!(a.model().layout, livtk_app::model::Layout::Board)
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
fn the_palette_adds_a_link_from_its_actions(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    cx.simulate_keystrokes("ctrl-k");
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
    cx.simulate_keystrokes("g t b");
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
fn the_panels_flow_moves_the_open_task_and_fits_the_panel(cx: &mut TestAppContext) {
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
    let names = ["backlog", "planned", "acked", "doing", "review", "done"];
    let labels: Vec<_> = names
        .iter()
        .map(|n| {
            cx.debug_bounds(named(format!("flow-label-{n}")))
                .expect("labelled")
        })
        .collect();
    for pair in labels.windows(2) {
        assert!(pair[0].right() < pair[1].left(), "labels apart: {pair:?}");
    }
    assert!(
        labels[5].right() <= panel.right(),
        "the last label is inside the panel"
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

#[gpui_kit::test]
fn the_flows_arrows_sit_between_the_states_they_join(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    open_detail(cx);
    let names = ["backlog", "planned", "acked", "doing", "review", "done"];
    let rings: Vec<_> = names
        .iter()
        .map(|n| cx.debug_bounds(named(format!("flow-{n}"))).expect("a ring"))
        .collect();
    for gap in 0..5 {
        let arrow = cx
            .debug_bounds(named(format!("flow-arrow-{gap}")))
            .expect("an arrow");
        assert!(
            arrow.left() >= rings[gap].right() - gpui_kit::px(20.)
                && arrow.right() <= rings[gap + 1].left() + gpui_kit::px(20.),
            "arrow {gap} {arrow:?} between {:?} and {:?}",
            rings[gap],
            rings[gap + 1]
        );
        assert!(arrow.size.width >= gpui_kit::px(10.), "visible: {arrow:?}");
    }
}

/// An icon's drawn bounds, by its file name.
fn icon_at(cx: &mut gpui_kit::VisualTestContext, file: &str) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    cx.debug_bounds(named(format!("icon-{file}")))
        .unwrap_or_else(|| panic!("icon {file} is drawn"))
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
fn each_view_has_its_icon_in_the_sidebar_at_16_px(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    for file in [
        "inbox",
        "bolt-alt",
        "task",
        "cognition",
        "route",
        "medical-flask",
        "circuit-board",
        "folder",
        "folder-plus",
        "help-circle",
    ] {
        let b = icon_at(cx, file);
        assert_eq!(
            (b.size.width, b.size.height),
            (gpui_kit::px(16.), gpui_kit::px(16.)),
            "{file}"
        );
        assert!(
            b.right() < gpui_kit::px(240.),
            "{file} is in the sidebar: {b:?}"
        );
    }
}

#[gpui_kit::test]
fn toolbar_and_panel_icons_sit_inside_their_buttons(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("g t");
    cx.run_until_parked();
    let filter = cx.debug_bounds("filter-button").unwrap();
    let display = cx.debug_bounds("display-button").unwrap();
    assert!(inside(icon_at(cx, "menu-filter"), filter));
    assert!(inside(icon_at(cx, "slider-alt"), display));
    assert_eq!(
        filter.size.height,
        gpui_kit::px(28.),
        "the button kept its height"
    );
    assert!(icon_at(cx, "list-ul").size.width == gpui_kit::px(14.));
    assert!(icon_at(cx, "columns-4").size.width == gpui_kit::px(14.));

    open_detail(cx);
    let close = cx.debug_bounds("close-detail").unwrap();
    assert!(
        inside(icon_at(cx, "dock-right-arrow"), close),
        "the close icon fits its button"
    );
    let add = cx.debug_bounds("add-link").unwrap();
    assert!(inside(icon_at(cx, "link"), add));
}

#[gpui_kit::test]
fn the_palette_has_a_search_icon_and_an_icon_on_every_action(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("ctrl-k");
    cx.run_until_parked();
    let field = cx.debug_bounds("palette-field").unwrap();
    assert!(inside(icon_at(cx, "search"), field));
    cx.simulate_input("go to graph");
    cx.run_until_parked();
    let row = cx.debug_bounds("palette-item-0").unwrap();
    assert_eq!(row.size.height, gpui_kit::px(36.), "rows keep their height");
    // The graph's icon now shows twice: in the sidebar and on this row.
    let rows_icon = cx.debug_bounds("icon-circuit-board").expect("drawn");
    assert!(
        inside(rows_icon, row) || rows_icon.right() < gpui_kit::px(240.),
        "{rows_icon:?}"
    );
    assert!(
        inside(icon_at(cx, "enter"), cx.debug_bounds("palette").unwrap()),
        "the footer's enter"
    );
}
