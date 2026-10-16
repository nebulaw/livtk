//! The window with real key events, headless: what the keys do, and what they must not do.
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
