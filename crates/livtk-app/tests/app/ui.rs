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

/// The note bar's send button sends as Enter does: the text becomes the task's note and the
/// field empties. With nothing written, or only spaces, it does nothing.
#[gpui_kit::test]
fn the_send_button_adds_the_note_as_enter_does(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    let history = || {
        Workspace::open(dir.path())
            .unwrap()
            .history(Some("LIV-1"))
            .unwrap()
    };
    cx.simulate_keystrokes("c");
    cx.run_until_parked();
    let before = history().len();

    click_on(cx, "note-send");
    cx.simulate_input("   ");
    cx.run_until_parked();
    click_on(cx, "note-send");
    assert_eq!(
        history().len(),
        before,
        "nothing, or only spaces, sends nothing"
    );
    app.read_with(cx, |a, cx| {
        assert_eq!(a.note_text(cx), "   ", "and keeps the draft")
    });

    cx.simulate_input("sent by click");
    cx.run_until_parked();
    click_on(cx, "note-send");
    let all = history();
    assert_eq!(all.len(), before + 1, "one note");
    let last = all.last().unwrap();
    assert!(
        matches!(&last.op, livtk_core::event::Op::NodeNote { data, .. } if data.text.trim() == "sent by click"),
        "{last:?}"
    );
    assert_eq!(last.by, "human");
    app.read_with(cx, |a, cx| {
        assert_eq!(a.note_text(cx), "", "the field empties")
    });
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
    double_click_at(cx, at);
}

/// A double-click at `at`, a frame drawn while the button is down the second time, as one is
/// between a real press and release (a word picked by a double-click is worked out drawing).
fn double_click_at(cx: &mut gpui_kit::VisualTestContext, at: gpui_kit::Point<gpui_kit::Pixels>) {
    // The pointer comes to it first, as a real one does: what is under it is hovered.
    cx.simulate_mouse_move(at, None, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    for click_count in [1, 2] {
        cx.simulate_event(gpui_kit::MouseDownEvent {
            position: at,
            modifiers: gpui_kit::Modifiers::default(),
            button: gpui_kit::MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
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
    cx.simulate_keystrokes("secondary-right");
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

/// A title or a body opens for editing on a double-click; one click only selects. A body with
/// nothing in it the same.
#[gpui_kit::test]
fn a_double_click_edits_one_click_only_selects(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    open_detail(cx);
    for (read, part) in [("detail-title", "title"), ("detail-body", "body")] {
        click_on(cx, read);
        app.read_with(cx, |a, _| {
            assert_eq!(a.editing(), None, "one click selects")
        });
        double_click_on(cx, read);
        app.read_with(cx, |a, _| assert_eq!(a.editing(), Some(part)));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn the_body_is_edited_in_place_and_saved_with_ctrl_enter(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    open_detail(cx);
    // An empty body opens on a double-click too.
    double_click_on(cx, "detail-body");
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

/// Settings: Cmd-comma opens them on Appearance; the docs' text size and density change the
/// reader and are the model's to remember; Esc leaves.
#[gpui_kit::test]
fn settings_set_how_docs_read(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.simulate_keystrokes("secondary-,");
    redraw(cx);
    assert!(cx.debug_bounds("settings-page").is_some());
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
    assert!(cx.debug_bounds("settings-page").is_none());
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
        view: Some(livtk_app::model::View::Tasks),
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
        cx.update(|_, cx| {
            cx.is_action_available(&livtk_app::ui::GoView {
                view: livtk_app::model::View::Inbox,
            })
        })
    };
    assert!(!available(cx), "with no window active, nobody hears it");
    // As the Dock sends them: from the app, not from inside the window (which a test's
    // `update` is), so deferred.
    let window = gpui_kit::VisualContext::window_handle(cx);
    let weak = app.downgrade();
    cx.update(|_, cx| livtk_app::ui::dock_actions(window, weak, cx));
    assert!(available(cx), "the Dock's item is not greyed");
    cx.update(|_, cx| {
        cx.defer(|cx| {
            cx.dispatch_action(&livtk_app::ui::GoView {
                view: livtk_app::model::View::Inbox,
            })
        })
    });
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
    // Where the panes land, not their growing in.
    cx.update(|_, cx| cx.set_reduce_motion(true));
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
    // The docs, in neither pane, show alone; the terminal brings the split back.
    click_on(cx, "view-Docs");
    redraw(cx);
    assert!(cx.debug_bounds("terminal").is_none(), "the docs alone");
    // The split's views sit grouped at the sidebar's top, not in their usual place.
    assert!(cx.debug_bounds("sidebar-split").is_some());
    assert!(cx.debug_bounds("view-Terminal").is_none());
    click_on(cx, "split-Terminal");
    redraw(cx);
    assert!(cx.debug_bounds("terminal").is_some(), "the split again");
    app.read_with(cx, |a, _| assert!(!a.model().solo()));
    cx.simulate_keystrokes("secondary-alt-]");
    redraw(cx);
    // Unsplit from the sidebar: the tasks' pane goes, the terminal is all there is, and the
    // group with it.
    let row = cx.debug_bounds("split-Tasks").unwrap();
    cx.simulate_mouse_move(row.center(), None, gpui_kit::Modifiers::default());
    redraw(cx);
    click_on(cx, "unsplit-Tasks");
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().panes().len(), 1);
        assert_eq!(a.model().view(), View::Terminal);
    });
    assert!(cx.debug_bounds("sidebar-split").is_none());
    assert!(
        cx.debug_bounds("view-Tasks").is_some(),
        "back in its usual place"
    );
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

/// The divider between two panes is an edge as the sidebar's: its handle follows the pointer
/// along it, a drag moves the room between the panes, a double-click evens them out.
#[gpui_kit::test]
fn the_divider_between_panes_is_dragged_as_the_sidebars_edge(cx: &mut TestAppContext) {
    let dir = project();
    cx.update(|cx| cx.set_global(livtk_app::ui::terminal::Headless));
    let (app, cx) = open_app(cx, dir.path());
    // Where the panes land, not their growing in.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1600., 1000.);
    cx.simulate_keystrokes("g t secondary-\\");
    redraw(cx);
    click_on(cx, "place-findings");
    redraw(cx);
    let edge = cx.debug_bounds("divider-0-1").expect("the divider");
    let at = gpui_kit::point(edge.center().x, gpui_kit::px(500.));
    cx.simulate_mouse_move(at, None, gpui_kit::Modifiers::default());
    redraw(cx);
    let handle = cx.debug_bounds("edge-handle").expect("its handle shows");
    assert!(
        (f32::from(handle.center().y) - 500.).abs() < 1.,
        "at the pointer: {handle:?}"
    );
    drag_to(cx, "divider-0-1", f32::from(edge.center().x) + 200.);
    let shares = app.read_with(cx, |a, _| match &a.model().split {
        livtk_app::model::Split::Split { parts, .. } => parts[0].1,
        _ => 0.,
    });
    assert!(shares > 0.55, "the left pane grew: {shares}");
    double_click_on(cx, "divider-0-1");
    redraw(cx);
    let shares = app.read_with(cx, |a, _| match &a.model().split {
        livtk_app::model::Split::Split { parts, .. } => parts[0].1,
        _ => 0.,
    });
    assert!((shares - 0.5).abs() < 1e-4, "evened out: {shares}");
}

/// On the collapsed sidebar's rail the split's views are grouped too, out of their usual
/// places; a click focuses a pane, the × at a button's corner unsplits it.
#[gpui_kit::test]
fn the_rail_groups_the_splits_views_and_unsplits_them(cx: &mut TestAppContext) {
    let dir = project();
    cx.update(|cx| cx.set_global(livtk_app::ui::terminal::Headless));
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1600., 1000.);
    cx.simulate_keystrokes("g x secondary-\\");
    redraw(cx);
    click_on(cx, "place-tasks");
    cx.simulate_keystrokes("secondary-b");
    redraw(cx);
    assert!(
        cx.debug_bounds("rail-split").is_some(),
        "the group on the rail"
    );
    assert!(
        cx.debug_bounds("rail-Terminal").is_none(),
        "out of its usual place"
    );
    click_on(cx, "rail-split-Terminal");
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.model().view(), View::Terminal));
    let button = cx.debug_bounds("rail-split-Tasks").unwrap();
    cx.simulate_mouse_move(button.center(), None, gpui_kit::Modifiers::default());
    redraw(cx);
    click_on(cx, "rail-unsplit-Tasks");
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.model().panes().len(), 1));
    assert!(cx.debug_bounds("rail-split").is_none());
    assert!(cx.debug_bounds("rail-Tasks").is_some(), "back in its place");
}

/// A view joining the split slides from its place in the list into the split's group, and
/// back out on unsplit: at the start of the move it is where it was, at the end in its place.
#[gpui_kit::test]
fn a_view_joining_the_split_slides_from_its_place_into_the_group_and_back(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    resize(cx, 1600., 1000.);
    cx.simulate_keystrokes("g t");
    redraw(cx);
    let was = cx.debug_bounds("view-Tasks").unwrap().origin.y;
    // Split: the tasks join the split's group, and start from where they were.
    cx.simulate_keystrokes("secondary-\\");
    redraw(cx);
    let start = cx
        .debug_bounds("split-Tasks")
        .expect("in the group")
        .origin
        .y;
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    redraw(cx);
    let end = cx.debug_bounds("split-Tasks").unwrap().origin.y;
    assert!(end < was, "then sits up in the group: {end:?}");
    // The animation runs on real time, so by the time it is drawn it may be a little way in:
    // what matters is that it set off from its old place, nearer it than its new one.
    let (was_y, start_y, end_y) = (f32::from(was), f32::from(start), f32::from(end));
    assert!(
        (start_y - was_y).abs() < (start_y - end_y).abs(),
        "starts from its old place: {start:?}, from {was:?} to {end:?}"
    );
    // Unsplit: back down from the group to its place.
    app.update(cx, |a, _| {
        let id = a.model().split_views()[0].0;
        a.model_mut().unsplit(id);
    });
    redraw(cx);
    let back = cx
        .debug_bounds("view-Tasks")
        .expect("in its place")
        .origin
        .y;
    assert!(
        (f32::from(back) - end_y).abs() < (f32::from(back) - was_y).abs(),
        "starts from the group: {back:?}, from {end:?} to {was:?}"
    );
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    redraw(cx);
    let home = cx.debug_bounds("view-Tasks").unwrap().origin.y;
    assert!(
        (f32::from(home) - f32::from(was)).abs() < 1.,
        "home: {home:?}"
    );
}

/// The detail opens three ways, picked from the button beside its close: side peek (the panel),
/// center peek (a card over the window; Esc or a press outside closes it, J moves on and it
/// follows), full page (in the list's place; Esc gives the list back). The choice sticks.
#[gpui_kit::test]
fn the_detail_opens_as_a_side_peek_a_center_peek_or_a_full_page(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1600., 1000.);
    cx.simulate_keystrokes("g t enter");
    settle(cx);
    assert!(
        cx.debug_bounds("detail-body-panel").is_some(),
        "a side peek first"
    );
    // Center peek, from the bar's menu: the open detail moves there at once.
    click_on(cx, "detail-mode");
    click_on(cx, "detail-mode-center-peek");
    settle(cx);
    assert!(cx.debug_bounds("center-peek").is_some(), "over the window");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().detail_mode, livtk_app::model::DetailMode::Center)
    });
    let first = app.read_with(cx, |a, _| {
        a.model().detail.as_ref().map(|d| d.node.key.clone())
    });
    cx.simulate_keystrokes("j");
    settle(cx);
    let next = app.read_with(cx, |a, _| {
        a.model().detail.as_ref().map(|d| d.node.key.clone())
    });
    assert!(
        next.is_some() && next != first,
        "it follows the list behind"
    );
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(cx.debug_bounds("center-peek").is_none(), "Esc closes it");
    app.read_with(cx, |a, _| assert!(a.model().detail.is_none()));
    // A press outside closes it too.
    cx.simulate_keystrokes("enter");
    settle(cx);
    cx.simulate_click(
        gpui_kit::point(gpui_kit::px(20.), gpui_kit::px(500.)),
        gpui_kit::Modifiers::default(),
    );
    settle(cx);
    assert!(
        cx.debug_bounds("center-peek").is_none(),
        "a press outside closes it"
    );
    // Full page: in the list's place; Esc gives the list back.
    cx.simulate_keystrokes("enter");
    settle(cx);
    click_on(cx, "detail-mode");
    click_on(cx, "detail-mode-full-page");
    settle(cx);
    assert!(
        cx.debug_bounds("full-page").is_some(),
        "in the list's place"
    );
    assert!(
        cx.debug_bounds("dock").is_none(),
        "the list's dock gone with it"
    );
    // Esc shuts a menu opened from the page first, the page after.
    click_on(cx, "detail-mode");
    assert!(cx.debug_bounds("detail-mode-menu").is_some());
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(
        cx.debug_bounds("detail-mode-menu").is_none(),
        "the menu shut"
    );
    assert!(cx.debug_bounds("full-page").is_some(), "the page stays");
    assert!(cx.debug_bounds("center-peek").is_none());
    assert!(cx.debug_bounds("detail-panel").is_none(), "no panel beside");
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(
        cx.debug_bounds("full-page").is_none(),
        "Esc gives the list back"
    );
    assert!(cx.debug_bounds("dock").is_some(), "and its dock");
    // The next detail opens the same way.
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert!(cx.debug_bounds("full-page").is_some(), "the choice sticks");
}

/// A full page opened in a split shows in its own pane only; the pane beside stays as it is.
#[gpui_kit::test]
fn a_full_page_in_a_split_stays_in_its_pane(cx: &mut TestAppContext) {
    let dir = project();
    cx.update(|cx| cx.set_global(livtk_app::ui::terminal::Headless));
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1600., 1000.);
    app.update(cx, |a, _| {
        a.model_mut().detail_mode = livtk_app::model::DetailMode::Full
    });
    cx.simulate_keystrokes("g x secondary-\\");
    redraw(cx);
    click_on(cx, "place-tasks");
    cx.simulate_keystrokes("enter");
    settle(cx);
    let page = cx.debug_bounds("full-page").expect("the full page");
    let terminal = cx.debug_bounds("terminal").expect("the terminal beside");
    assert!(
        page.left() > terminal.right(),
        "in the tasks' pane: {page:?}"
    );
    // Focusing the terminal leaves the page in its pane.
    click_on(cx, "pane-0");
    redraw(cx);
    let still = cx.debug_bounds("full-page").expect("still shown");
    assert!((f32::from(still.left()) - f32::from(page.left())).abs() < 1.);
}

#[gpui_kit::test]
fn probe_full_page_esc_with_menu_open(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1600., 1000.);
    app.update(cx, |a, _| {
        a.model_mut().detail_mode = livtk_app::model::DetailMode::Full
    });
    cx.simulate_keystrokes("g t enter");
    settle(cx);
    assert!(cx.debug_bounds("full-page").is_some());
    click_on(cx, "detail-mode");
    settle(cx);
    assert!(cx.debug_bounds("detail-mode-menu").is_some(), "menu open");
    cx.simulate_keystrokes("escape");
    settle(cx);
    let open = app.read_with(cx, |a, _| a.model().detail.is_some());
    let menu = cx.debug_bounds("detail-mode-menu").is_some();
    eprintln!("PROBE1 detail_open={open} menu_visible={menu}");
}

#[gpui_kit::test]
fn probe_full_page_note_escape(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1600., 1000.);
    cx.simulate_keystrokes("g t c");
    settle(cx);
    cx.simulate_keystrokes("escape");
    settle(cx);
    let side = app.read_with(cx, |a, _| a.model().detail.is_some());
    // full page
    app.update(cx, |a, _| {
        a.model_mut().detail_mode = livtk_app::model::DetailMode::Full
    });
    redraw(cx);
    cx.simulate_keystrokes("c");
    settle(cx);
    app.update(cx, |a, _| a.model_mut().close_detail());
    redraw(cx);
    cx.simulate_keystrokes("enter");
    settle(cx);
    eprintln!("PROBE2 side_after_note_esc_open={side}");
}

#[gpui_kit::test]
fn probe_close_pane_with_full_page(cx: &mut TestAppContext) {
    let dir = project();
    cx.update(|cx| cx.set_global(livtk_app::ui::terminal::Headless));
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1600., 1000.);
    app.update(cx, |a, _| {
        a.model_mut().detail_mode = livtk_app::model::DetailMode::Full
    });
    cx.simulate_keystrokes("g x secondary-\\");
    redraw(cx);
    click_on(cx, "place-tasks");
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert!(cx.debug_bounds("full-page").is_some());
    cx.simulate_keystrokes("secondary-alt-w");
    settle(cx);
    let open = app.read_with(cx, |a, _| {
        a.model().detail.as_ref().map(|d| d.node.key.clone())
    });
    let shown = cx.debug_bounds("full-page").is_some()
        || cx.debug_bounds("detail-panel").is_some()
        || cx.debug_bounds("center-peek").is_some();
    eprintln!("PROBE3 detail={open:?} shown={shown}");
}

#[gpui_kit::test]
fn probe_center_peek_new_task(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1600., 1000.);
    app.update(cx, |a, _| {
        a.model_mut().detail_mode = livtk_app::model::DetailMode::Center
    });
    cx.simulate_keystrokes("g t enter");
    settle(cx);
    assert!(cx.debug_bounds("center-peek").is_some());
    cx.simulate_keystrokes("n");
    settle(cx);
    let form = cx.debug_bounds("new-task-card");
    let peek = cx.debug_bounds("center-peek").is_some();
    eprintln!("PROBE4 form={form:?} peek={peek}");
    if let Some(b) = form {
        cx.simulate_click(b.center(), gpui_kit::Modifiers::default());
        settle(cx);
        let open = app.read_with(cx, |a, _| a.model().detail.is_some());
        eprintln!("PROBE4 after click on form: detail_open={open}");
    }
}

#[gpui_kit::test]
fn probe_note_esc_full(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1600., 1000.);
    cx.simulate_keystrokes("g t c");
    settle(cx);
    app.update(cx, |a, _| {
        a.model_mut().detail_mode = livtk_app::model::DetailMode::Full
    });
    redraw(cx);
    assert!(cx.debug_bounds("full-page").is_some());
    cx.simulate_keystrokes("h i");
    settle(cx);
    let open1 = app.read_with(cx, |a, _| a.model().detail.is_some());
    cx.simulate_keystrokes("escape");
    settle(cx);
    let open = app.read_with(cx, |a, _| a.model().detail.is_some());
    eprintln!("PROBE5 after typing open={open1}; after esc in note field detail_open={open}");
}

#[gpui_kit::test]
fn probe_stale_menu(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1600., 1000.);
    app.update(cx, |a, _| {
        a.model_mut().detail_mode = livtk_app::model::DetailMode::Full
    });
    cx.simulate_keystrokes("g t enter");
    settle(cx);
    click_on(cx, "detail-mode");
    settle(cx);
    cx.simulate_keystrokes("escape");
    settle(cx);
    cx.simulate_keystrokes("j enter");
    settle(cx);
    let menu = cx.debug_bounds("detail-mode-menu").is_some();
    let sel = app.read_with(cx, |a, _| {
        a.model().detail.as_ref().map(|d| d.node.key.clone())
    });
    eprintln!("PROBE6 reopened detail={sel:?} menu_still_open={menu}");
}

/// Cmd-, opens the settings: the sidebar lists their sections and the way back, the middle the
/// section. A project setting is written to its config; a bad one says why. Esc leaves.
#[gpui_kit::test]
fn settings_take_the_sidebar_and_write_the_projects_config(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("secondary-,");
    redraw(cx);
    assert!(
        cx.debug_bounds("settings-sidebar").is_some(),
        "the sections in the sidebar"
    );
    assert!(cx.debug_bounds("settings-page").is_some());
    assert!(
        cx.debug_bounds("view-Tasks").is_none(),
        "the views give way"
    );
    // What Claude is told: switching the session-start hook off writes the project's config.
    click_on(cx, "settings-what-claude-is-told");
    redraw(cx);
    click_on(cx, "settings-hook-session-start-on");
    redraw(cx);
    let config = std::fs::read_to_string(dir.path().join(".livtk/config.toml")).unwrap();
    assert!(config.contains("disable = [\"session-start\"]"), "{config}");
    assert!(cx.debug_bounds("settings-hook-session-start-off").is_some());
    // A gate that names no move is refused, with its reason.
    click_on(cx, "settings-your-gates");
    redraw(cx);
    app.update(cx, |a, cx| a.type_gate("task: nowhere -> done", cx));
    redraw(cx);
    assert!(cx.debug_bounds("settings-error").is_some(), "says why");
    // Esc: back to the app.
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert!(cx.debug_bounds("settings-sidebar").is_none());
    assert!(cx.debug_bounds("view-Tasks").is_some());
}

/// Settings › Opening work hides the state graph in a node's detail, in every mode, keeping the
/// state menu; on brings it back; the choice is saved, and a file without it shows the graph.
#[gpui_kit::test]
fn the_state_graph_is_a_setting(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    open_detail(cx);
    assert!(cx.debug_bounds("state-flow").is_some(), "shown by default");
    cx.simulate_keystrokes("secondary-,");
    redraw(cx);
    click_on(cx, "settings-opening-work");
    redraw(cx);
    click_on(cx, "settings-state-graph-on");
    redraw(cx);
    assert!(cx.debug_bounds("settings-state-graph-off").is_some());
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(
        cx.debug_bounds("detail-state").is_some(),
        "the state menu stays"
    );
    assert!(
        cx.debug_bounds("state-flow").is_none(),
        "hidden beside the list"
    );
    // And it still moves the task.
    click_on(cx, "detail-state");
    redraw(cx);
    // Acked, it leaves the inbox and the detail follows the next row: its key first.
    let key = app.read_with(cx, |a, _| {
        a.model().detail.as_ref().unwrap().node.key.clone()
    });
    click_on(cx, "set-state-acked");
    settle(cx);
    assert_eq!(
        state_of(dir.path(), &key),
        State::Acked,
        "moved with the graph hidden"
    );
    for mode in [
        livtk_app::model::DetailMode::Center,
        livtk_app::model::DetailMode::Full,
    ] {
        app.update(cx, |a, cx| {
            a.model_mut().detail_mode = mode;
            cx.notify();
        });
        settle(cx);
        assert!(cx.debug_bounds("detail-state").is_some(), "{mode:?}");
        assert!(
            cx.debug_bounds("state-flow").is_none(),
            "hidden in {mode:?}"
        );
    }
    // Saved, and brought back.
    let saved = app.read_with(cx, |a, _| a.model().saved());
    assert!(saved.prefs.hide_state_flow);
    // A file written before the setting existed: no such key.
    let file = tempfile::tempdir().unwrap();
    let path = file.path().join("app.toml");
    std::fs::write(&path, "detail_mode = \"side\"\nreading_compact = false\n").unwrap();
    let old = livtk_app::model::Saved::load(&path);
    app.update(cx, |a, cx| {
        a.model_mut().restore(&old);
        cx.notify();
    });
    settle(cx);
    assert!(
        !app.read_with(cx, |a, _| a.model().prefs.hide_state_flow),
        "a file without it shows the graph"
    );
    assert!(cx.debug_bounds("state-flow").is_some(), "shown again");
    app.update(cx, |a, cx| {
        a.model_mut().restore(&saved);
        cx.notify();
    });
    settle(cx);
    assert!(cx.debug_bounds("state-flow").is_none(), "restored hidden");
}

/// The projects view: each row's progress, health, target and what waits; the panel's own
/// section; and grouping by project offered for tasks alone.
#[gpui_kit::test]
fn projects_show_their_progress_in_rows_and_panel(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let _ws = crate::projects::setup(dir.path());
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("g p");
    redraw(cx);
    for part in ["progress", "health", "target", "waiting"] {
        assert!(
            cx.debug_bounds(named(format!("project-{part}-PRJ-1")))
                .is_some(),
            "{part}"
        );
    }
    assert!(
        cx.debug_bounds("project-waiting-PRJ-2").is_none(),
        "nothing waits"
    );
    assert!(cx.debug_bounds("too-many-active").is_none());
    open_detail(cx);
    assert!(cx.debug_bounds("project-section").is_some());
    assert!(cx.debug_bounds("project-waits-planned").is_some());
    assert!(cx.debug_bounds("project-waits-review").is_some());
    assert!(cx.debug_bounds("project-update-0").is_some());
    // Its flow: the lead time and a bar per state.
    assert!(cx.debug_bounds("project-lead").is_some());
    assert!(cx.debug_bounds("flow-time-review").is_some());
    assert!(
        cx.debug_bounds("flow-time-done").is_none(),
        "done takes no time"
    );
    // Grouping by project: offered in Tasks, not in Projects.
    app.update(cx, |a, cx| {
        a.model_mut().close_detail();
        cx.notify();
    });
    click_on(cx, "dock-grouping");
    redraw(cx);
    assert!(
        cx.debug_bounds("group-project").is_none(),
        "not for projects"
    );
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("g t");
    redraw(cx);
    click_on(cx, "dock-grouping");
    redraw(cx);
    click_on(cx, "group-project");
    redraw(cx);
    assert_eq!(
        app.read_with(cx, |a, _| a.model().display().grouping),
        livtk_app::model::Grouping::Project
    );
    // The activity view says what waits and for how long.
    cx.simulate_keystrokes("g a");
    redraw(cx);
    assert!(cx.debug_bounds("flow-strip").is_some());
}

/// One stub `claude` and Claude home for the whole test binary (livtk is pointed at them once):
/// `claude agents` answers with `.stub-agents.json` in the folder it runs in (the project),
/// `claude stop` writes `.stub-stopped` there, and `claude -p` (a run's turn) answers as the next
/// word in `.stub-turns` there says, so each test's project answers for itself. The home has
/// livtk's plugin with one agent, `implementer`.
const STUB_CLAUDE: &str = r#"#!/bin/sh
if [ "$1" = agents ]; then cat .stub-agents.json 2>/dev/null || echo '[]'; exit 0; fi
if [ "$1" = stop ]; then echo "$2" >> .stub-stopped; exit 0; fi
f=.stub-turns
what=$(head -n 1 "$f")
tail -n +2 "$f" > "$f.tmp"; mv "$f.tmp" "$f"
printf '%s\n' "$*" >> .stub-args
case "$what" in
  slow) trap 'echo "{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"is_error\":true,\"result\":\"\"}"; exit 0' INT
        echo '{"type":"system","subtype":"init"}'
        while :; do sleep 0.05; done ;;
  deny) echo '{"type":"result","subtype":"success","is_error":false,"result":"I need the tests.","permission_denials":[{"tool_name":"Bash","tool_use_id":"t1","tool_input":{"command":"cargo test"}}]}' ;;
  push) echo '{"type":"result","subtype":"success","is_error":false,"result":"Pushing it.","permission_denials":[{"tool_name":"Bash","tool_use_id":"t1","tool_input":{"command":"git push origin main"}}]}' ;;
  *) echo '{"type":"result","subtype":"success","is_error":false,"result":"Which colour?","permission_denials":[]}' ;;
esac
"#;

fn claude_stub() -> &'static std::path::Path {
    use std::os::unix::fs::PermissionsExt;
    static HOME: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let home = tempfile::tempdir().unwrap().keep();
        let plugin = home.join("plugins/cache/livtk/livtk/0.4.0");
        std::fs::create_dir_all(plugin.join("agents")).unwrap();
        std::fs::write(
            plugin.join("agents/implementer.md"),
            "---\nname: implementer\ndescription: Implements a task.\ntools:\n  - Read\n  - Edit\ncolor: blue\n---\n\nYou implement.\n",
        )
        .unwrap();
        std::fs::write(
            home.join("plugins/installed_plugins.json"),
            format!(
                r#"{{"version":2,"plugins":{{"livtk@livtk":[{{"installPath":"{}"}}]}}}}"#,
                plugin.display()
            ),
        )
        .unwrap();
        let claude = home.join("claude");
        std::fs::write(
            &claude,
            STUB_CLAUDE,
        )
        .unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        livtk_app::agents::use_paths(claude, home.clone());
        home
    })
}

/// Sessions and Agents, two views, with a stub `claude`. Sessions: this project's only, Needs
/// you first, Stop for a background one. Agents: no sessions; the agents by place, livtk's
/// copied, a copy saved and then removed after a second press.
#[gpui_kit::test]
fn sessions_and_agents_are_two_views(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path().canonicalize().unwrap();
    claude_stub();
    let sessions = root.join(".stub-agents.json");
    let write_sessions = |waiting: &str| {
        std::fs::write(
            &sessions,
            format!(
                r#"[
  {{"pid":1,"cwd":"{root}","kind":"interactive","startedAt":1792576396219,"sessionId":"aaaaaaaa-1","name":"planning","status":"busy"}},
  {{"cwd":"{root}","kind":"background","startedAt":1792576396000,"sessionId":"bbbbbbbb-2","name":"building","status":"{waiting}","id":"bg1","state":"working"}},
  {{"pid":3,"cwd":"/elsewhere","kind":"interactive","startedAt":1,"sessionId":"cccccccc-3","name":"other","status":"busy"}}
]"#,
                root = root.display()
            ),
        )
        .unwrap();
    };
    write_sessions("waiting");
    let (app, cx) = open_app(cx, &root);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("g s");
    redraw(cx);
    redraw(cx);
    assert!(cx.debug_bounds("sessions-place").is_some());
    // This project's two, the one waiting for you first; not the one elsewhere.
    let waiting = cx
        .debug_bounds("session-bbbbbbbb-2")
        .expect("the waiting one");
    let busy = cx.debug_bounds("session-aaaaaaaa-1").expect("the busy one");
    assert!(waiting.top() < busy.top(), "Needs you first");
    assert!(
        cx.debug_bounds("session-cccccccc-3").is_none(),
        "not this project's"
    );
    // It follows Claude Code: no longer waiting, it moves below.
    write_sessions("busy");
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(4));
    redraw(cx);
    redraw(cx);
    let (a, b) = (
        cx.debug_bounds("session-aaaaaaaa-1").unwrap(),
        cx.debug_bounds("session-bbbbbbbb-2").unwrap(),
    );
    assert!(a.top() < b.top(), "both working: the newest first");
    // A background session can be stopped from its panel.
    click_on(cx, "session-bbbbbbbb-2");
    redraw(cx);
    assert!(cx.debug_bounds("session-page").is_some());
    click_on(cx, "session-stop");
    assert_eq!(
        std::fs::read_to_string(root.join(".stub-stopped"))
            .unwrap()
            .trim(),
        "bg1"
    );
    // Agents: the agents, not the sessions, and no session's panel.
    cx.simulate_keystrokes("g e");
    redraw(cx);
    assert!(cx.debug_bounds("agents-place").is_some());
    assert!(cx.debug_bounds("session-aaaaaaaa-1").is_none());
    assert!(cx.debug_bounds("session-page").is_none());
    // livtk's own agent: copied, not changed or removed.
    click_on(cx, "agent-implementer-livtk");
    redraw(cx);
    assert!(cx.debug_bounds("agent-page").is_some());
    assert!(cx.debug_bounds("agent-save").is_none());
    assert!(cx.debug_bounds("agent-remove").is_none());
    click_on(cx, "agent-copy");
    redraw(cx);
    click_on(cx, "agent-save");
    redraw(cx);
    let copy = root.join(".claude/agents/implementer-copy.md");
    let text = std::fs::read_to_string(&copy).expect("saved in the project");
    assert!(
        text.contains("name: implementer-copy") && text.contains("tools: Read, Edit"),
        "{text}"
    );
    assert!(cx.debug_bounds("agent-implementer-copy-project").is_some());
    assert!(
        text.contains("color: blue"),
        "the keys livtk does not edit come along: {text}"
    );
    // Edited: a tool added, saved in place; the other keys stay.
    click_on(cx, "agent-tool-bash-off");
    redraw(cx);
    click_on(cx, "agent-save");
    redraw(cx);
    let text = std::fs::read_to_string(&copy).unwrap();
    assert!(text.contains("tools: Read, Edit, Bash"), "{text}");
    assert!(text.contains("color: blue"), "{text}");
    // Removed only on the second press.
    click_on(cx, "agent-remove");
    redraw(cx);
    assert!(copy.exists(), "asked first");
    click_on(cx, "agent-remove");
    redraw(cx);
    assert!(!copy.exists());
    let _ = app;
}

/// Sessions: a task's links name the sessions that worked on it, after its other links; a click reads one in
/// Agents, at the task's first event in it; a call opens on a click; Esc gives the list back,
/// where the session wears its transcript's title.
#[gpui_kit::test]
fn a_task_names_its_sessions_and_one_reads_from_there(cx: &mut TestAppContext) {
    let home = claude_stub();
    let dir = project();
    let root = dir.path().canonicalize().unwrap();
    // Claude worked on LIV-1 in session sess-x, at `at`.
    let at: jiff::Timestamp = {
        let mut ws = Workspace::open(&root).unwrap();
        ws.set_session(Some("sess-x".into()));
        ws.execute(
            Command::Note {
                node: "LIV-1".into(),
                text: "on it".into(),
            },
            &CLAUDE,
        )
        .unwrap()[0]
            .at
            .parse()
            .unwrap()
    };
    let t_at = |secs: i64| (at + jiff::SignedDuration::from_secs(secs)).to_string();
    // Its transcript as a real one runs: a prompt before; the call that wrote the note just
    // before it, the call's result just after; a reply later.
    let dir_t = livtk_core::claude::transcripts_in(home, &root);
    std::fs::create_dir_all(&dir_t).unwrap();
    let rec = |v: serde_json::Value| format!("{v}\n");
    let mut t = String::new();
    t += &rec(
        serde_json::json!({"type": "user", "timestamp": t_at(-10), "sessionId": "sess-x",
        "message": {"role": "user", "content": "Start the importer"}}),
    );
    t += &rec(
        serde_json::json!({"type": "assistant", "timestamp": t_at(-1), "sessionId": "sess-x",
        "message": {"role": "assistant", "content": [{"type": "tool_use", "id": "toolu_1", "name": "Bash",
        "input": {"command": "livtk note LIV-1 'on it'", "description": "Note it"}}]}}),
    );
    t += &rec(
        serde_json::json!({"type": "user", "timestamp": t_at(1), "sessionId": "sess-x",
        "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "LIV-1: note added"}]}}),
    );
    t += &rec(
        serde_json::json!({"type": "assistant", "timestamp": t_at(60), "sessionId": "sess-x",
        "message": {"role": "assistant", "content": [{"type": "text", "text": "Noted on LIV-1."}]}}),
    );
    t += &rec(
        serde_json::json!({"type": "custom-title", "customTitle": "Importer", "sessionId": "sess-x"}),
    );
    std::fs::write(dir_t.join("sess-x.jsonl"), t).unwrap();
    // The subagent toolu_1 started, beside it.
    let sub = dir_t.join("sess-x/subagents");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join("agent-r1.jsonl"),
        rec(serde_json::json!({"type": "user", "timestamp": "2000-01-01T00:00:01.500Z", "isSidechain": true,
            "message": {"role": "user", "content": "Refute LIV-1"}})),
    )
    .unwrap();
    std::fs::write(
        sub.join("agent-r1.meta.json"),
        serde_json::json!({"agentType": "livtk:refuter", "toolUseId": "toolu_1"}).to_string(),
    )
    .unwrap();
    // A session the log knows whose transcript Claude Code no longer keeps.
    {
        let mut ws = Workspace::open(&root).unwrap();
        ws.set_session(Some("sess-gone".into()));
        ws.execute(
            Command::Note {
                node: "LIV-2".into(),
                text: "earlier".into(),
            },
            &CLAUDE,
        )
        .unwrap();
    }
    let (app, cx) = open_app(cx, &root);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    // Straight from a task, the Agents place never opened.
    open_detail(cx);
    redraw(cx);
    let group = cx
        .debug_bounds("detail-sessions")
        .expect("a group of its links");
    let row = cx
        .debug_bounds("task-session-sess-x")
        .expect("in its links");
    assert!(group.contains(&row.center()), "in the Sessions group");
    assert!(
        cx.debug_bounds("detail-prop-sessions").is_none(),
        "no longer a property"
    );
    let named = app.read_with(cx, |a, _| {
        a.model()
            .session_rows()
            .into_iter()
            .any(|r| r.id == "sess-x" && r.name == "Importer")
    });
    assert!(named, "by its title, the transcripts read for the panel");
    click_on(cx, "task-session-sess-x");
    redraw(cx);
    redraw(cx);
    assert_eq!(app.read_with(cx, |a, _| a.model().view()), View::Sessions);
    assert!(
        cx.debug_bounds("session-reader").is_some(),
        "read in the middle"
    );
    assert!(
        cx.debug_bounds("reader-focus").is_some(),
        "at the task's first event"
    );
    // Marked: the call that wrote it, not its result after.
    let marked = app.read_with(cx, |a, _| {
        let r = a.model().agents.reader.as_ref().unwrap();
        r.focus_index()
            .and_then(|i| r.drawn().nth(i).cloned())
            .map(|e| matches!(e, livtk_core::claude::Entry::Tool { .. }))
    });
    assert_eq!(marked, Some(true));
    // Esc in Agents leaves the session read in Sessions alone.
    cx.simulate_keystrokes("g e");
    redraw(cx);
    cx.simulate_keystrokes("escape");
    redraw(cx);
    cx.simulate_keystrokes("g s");
    redraw(cx);
    assert!(
        cx.debug_bounds("session-reader").is_some(),
        "still read, back in Sessions"
    );
    // A call is one line until opened.
    click_on(cx, "call-toolu_1");
    redraw(cx);
    let open = app.read_with(cx, |a, _| {
        a.model()
            .agents
            .reader
            .as_ref()
            .is_some_and(|r| r.open.contains("toolu_1"))
    });
    assert!(open);
    // The subagent it started: its work, read; Esc goes back to the session.
    click_on(cx, "subagent-toolu_1");
    redraw(cx);
    redraw(cx);
    let reading = app.read_with(cx, |a, _| {
        a.model()
            .agents
            .reader
            .as_ref()
            .map(|r| (r.path.ends_with("agent-r1.jsonl"), r.entries.len()))
    });
    assert_eq!(reading, Some((true, 1)), "the subagent's prompt");
    cx.simulate_keystrokes("escape");
    redraw(cx);
    let back = app.read_with(cx, |a, _| {
        a.model()
            .agents
            .reader
            .as_ref()
            .map(|r| r.path.ends_with("sess-x.jsonl"))
    });
    assert_eq!(back, Some(true), "back to the session");
    // Esc again: the list, the session by its title.
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert!(cx.debug_bounds("session-reader").is_none());
    let rows = app.read_with(cx, |a, _| a.model().session_rows());
    assert_eq!(
        rows.iter()
            .find(|r| r.id == "sess-x")
            .map(|r| r.name.as_str()),
        Some("Importer")
    );
    // A session without a transcript still shows, from the log, and says so.
    click_on(cx, "session-sess-gone");
    redraw(cx);
    assert!(cx.debug_bounds("session-gone").is_some());
    assert!(
        cx.debug_bounds("session-node-LIV-2").is_some(),
        "what it worked on, from the log"
    );
}

/// The sidebar: Agents in a section of its own, between the views and the tools.
#[gpui_kit::test]
fn agents_have_their_own_sidebar_section_above_the_tools(cx: &mut TestAppContext) {
    let dir = project();
    let (_app, cx) = open_app(cx, dir.path());
    resize(cx, 1400., 900.);
    redraw(cx);
    let docs = cx.debug_bounds("view-Docs").unwrap();
    let agents = cx.debug_bounds("view-Agents").unwrap();
    let graph = cx.debug_bounds("view-Graph").unwrap();
    assert!(docs.top() < agents.top() && agents.top() < graph.top());
    // A heading between Docs and Agents: more than a row's step apart.
    let step = agents.top() - docs.top();
    assert!(step > docs.size.height * 1.5, "{step:?}");
}

/// G and a view's key goes to it, for every view; and the View menu lists every view, the one
/// shown ticked. Both read the views' own table.
#[gpui_kit::test]
fn every_view_has_its_keys_and_its_menu_item(cx: &mut TestAppContext) {
    use livtk_app::model::View;
    use livtk_app::ui::menus::{MenuState, menu_bar};
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    for v in View::ALL {
        let keys = v
            .go_keys()
            .expect("every built-in view has keys")
            .to_lowercase();
        // The terminal would start a real shell here: its key is checked by its binding.
        if v == View::Terminal {
            let bound = cx.update(|_, cx| {
                cx.key_bindings()
                    .borrow()
                    .bindings_for_action(&livtk_app::ui::GoView { view: v })
                    .any(|b| {
                        b.keystrokes()
                            .iter()
                            .map(|k| k.unparse())
                            .collect::<Vec<_>>()
                            .join(" ")
                            == keys
                    })
            });
            assert!(bound, "{keys}");
            continue;
        }
        cx.simulate_keystrokes(&keys);
        redraw(cx);
        assert_eq!(app.read_with(cx, |a, _| a.model().view()), v, "{keys}");
    }
    let bar = menu_bar(&MenuState {
        view: Some(View::Agents),
        ..MenuState::default()
    });
    let view_menu = bar.iter().find(|m| m.name.as_ref() == "View").unwrap();
    for v in View::ALL {
        let ticked = view_menu.items.iter().find_map(|i| match i {
            gpui_kit::MenuItem::Action { name, checked, .. } if name.as_ref() == v.title() => {
                Some(*checked)
            }
            _ => None,
        });
        assert_eq!(ticked, Some(v == View::Agents), "{}", v.title());
    }
}

/// A kind wears one icon wherever it shows: its view's is its rows' slot's. A declared kind may
/// name one (`icon = "bug"`); a name the app does not have is said, with the names it has.
#[gpui_kit::test]
fn a_kind_has_one_icon_and_a_declared_one_may_name_it(cx: &mut TestAppContext) {
    use livtk_app::model::View;
    use livtk_app::ui::{kind_icon, view_icon};
    for k in [
        Kind::Task,
        Kind::Lesson,
        Kind::Decision,
        Kind::Finding,
        Kind::Project,
    ] {
        let view = View::ALL.into_iter().find(|v| v.kind() == Some(k)).unwrap();
        assert_eq!(view_icon(view), kind_icon(k), "{k}");
    }
    let dir = project();
    let config = dir.path().join(".livtk/config.toml");
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str(
        "\n[[kinds]]\nname = \"defect\"\nstates = [\"open\", \"fixed\"]\nkey = \"DEF\"\nicon = \"bug\"\n\
         [[kinds]]\nname = \"chore\"\nstates = [\"open\", \"done\"]\nkey = \"CHO\"\nicon = \"nope\"\n",
    );
    std::fs::write(&config, text).unwrap();
    let (app, cx) = open_app(cx, dir.path());
    let defect = Kind::named("defect").unwrap();
    assert_eq!(kind_icon(defect).file(), "bug");
    assert_eq!(view_icon(View::Of(defect)), kind_icon(defect));
    // The unknown name is said, naming the kind and what there is; it shows a general icon.
    let said = app.read_with(cx, |a, _| {
        a.model()
            .toasts()
            .shown()
            .0
            .iter()
            .any(|t| t.text.contains("`chore`: no icon `nope`") && t.text.contains("bug"))
    });
    assert!(said);
    assert_eq!(kind_icon(Kind::named("chore").unwrap()).file(), "blocks");
}

/// A node named elsewhere opens on a click, in its own view, even where the view hid it: a sub-
/// task's parent chip in its row, a link in a task's panel. A link's × only unlinks.
#[gpui_kit::test]
fn a_named_node_opens_on_a_click(cx: &mut TestAppContext) {
    let dir = linked_project();
    let part = {
        let mut ws = Workspace::open(dir.path()).unwrap();
        // LIV-1 canceled: hidden in Tasks until dismissed work is shown.
        ws.execute(
            Command::Move {
                node: "LIV-1".into(),
                to: State::Canceled,
            },
            &Actor::Human,
        )
        .unwrap();
        let e = ws
            .execute(
                Command::Create {
                    kind: Kind::Task,
                    title: "a part".into(),
                    body: String::new(),
                    priority: None,
                    links: vec![],
                },
                &CLAUDE,
            )
            .unwrap();
        let key = ws.node(&e[0].op.subjects().0.to_string()).unwrap().key;
        ws.execute(
            Command::Link {
                src: key.clone(),
                rel: livtk_core::Rel::Parent,
                dst: "LIV-2".into(),
            },
            &CLAUDE,
        )
        .unwrap();
        assert_eq!(
            ws.related(&key, livtk_core::Rel::Parent).unwrap().0[0].key,
            "LIV-2"
        );
        key
    };
    let (app, cx) = open_app(cx, dir.path());
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("g t");
    // Not nested: the part names its parent in its row.
    app.update(cx, |a, _| {
        if a.model().display().nest {
            a.model_mut().toggle_nest();
        }
    });
    redraw(cx);
    let detail = |cx: &mut gpui_kit::VisualTestContext| {
        app.read_with(cx, |a, _| {
            a.model().detail.as_ref().map(|d| d.node.key.clone())
        })
    };
    click_on(cx, "parent-LIV-2");
    settle(cx);
    assert_eq!(
        detail(cx).as_deref(),
        Some("LIV-2"),
        "the chip went to the parent"
    );
    assert_ne!(detail(cx).as_deref(), Some(part.as_str()));
    // The link to the hidden LIV-1: its view, dismissed work shown, it selected and open.
    click_on(cx, "link-waits-for-LIV-1");
    settle(cx);
    assert_eq!(detail(cx).as_deref(), Some("LIV-1"));
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Tasks);
        assert!(a.model().display().show_dismissed, "shown, to be seen");
    });
    // The × takes the link away and opens nothing.
    click_on(cx, "unlink-blocks-LIV-2");
    assert_eq!(detail(cx).as_deref(), Some("LIV-1"));
    let ws = Workspace::open(dir.path()).unwrap();
    assert!(ws.links("LIV-1").unwrap().blocks.is_empty());
}

/// Ends every run's turn in the project at `.0` when dropped: a failing test never leaves a stub
/// turn looping behind it.
struct KillRuns(std::path::PathBuf);

impl Drop for KillRuns {
    fn drop(&mut self) {
        for live in livtk_core::run::lives(&self.0.join(".livtk")) {
            if let Some(pid) = live.pid {
                let _ = std::process::Command::new("kill")
                    .args(["-KILL", &format!("-{pid}")])
                    .status();
            }
        }
    }
}

/// A run of the stub on LIV-1 (acknowledged first), its turn writing until interrupted.
fn a_run_writing(root: &std::path::Path) -> livtk_core::run::Runner {
    let runner = livtk_core::run::Runner {
        claude: claude_stub().join("claude"),
    };
    std::fs::write(root.join(".stub-turns"), "slow\nask\n").unwrap();
    let mut ws = Workspace::open(root).unwrap();
    ws.execute(
        Command::Move {
            node: "LIV-1".into(),
            to: State::Acked,
        },
        &Actor::Human,
    )
    .unwrap();
    let live = runner
        .start(&mut ws, "LIV-1", None, None, &[], &Actor::Human)
        .unwrap();
    // Under way: the stub ready for the interrupt.
    let start = Instant::now();
    while !std::fs::read_to_string(livtk_core::run::stream_path(ws.dir(), &live.session))
        .is_ok_and(|s| s.contains("init"))
    {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the turn never started"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    runner
}

/// Tick until the app has surveyed the runs and `done` holds of its model.
fn until_runs(
    app: &gpui_kit::Entity<LivtkApp>,
    cx: &mut gpui_kit::VisualTestContext,
    done: impl Fn(&Model) -> bool,
) {
    let start = Instant::now();
    while !app.read_with(cx, |a, _| done(a.model())) {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "the runs never got there"
        );
        cx.executor().advance_clock(Duration::from_secs(3));
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(30));
    }
    redraw(cx);
}

/// Quitting while an agent works asks first, as things are at that moment (a run the app found
/// at its start, before a survey could): Cancel quits nothing and leaves it working; Stop them
/// all ends its turn, the run noted as stopped by the quit, and quits.
#[gpui_kit::test]
fn quitting_with_an_agent_working_asks_first(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path().canonicalize().unwrap();
    let _runs = KillRuns(root.clone());
    let runner = a_run_writing(&root);
    let (app, cx) = open_app(cx, &root);
    // Asked at once, before any survey: the quit looks for itself.
    cx.simulate_keystrokes("secondary-q");
    redraw(cx);
    assert!(cx.debug_bounds("quit-runs").is_some(), "asked at once");
    click_on(cx, "runs-cancel");
    // A run kept working through a quit is found again by the next start.
    until_runs(&app, cx, |m| m.runs.going.len() == 1);
    cx.simulate_keystrokes("secondary-q");
    redraw(cx);
    assert!(cx.debug_bounds("quit-runs").is_some(), "asked first");
    assert!(
        cx.debug_bounds("asked-run-LIV-1").is_some(),
        "naming the work"
    );
    click_on(cx, "runs-cancel");
    redraw(cx);
    assert!(cx.debug_bounds("quit-runs").is_none());
    let mut ws = Workspace::open(&root).unwrap();
    livtk_core::run::settle(&mut ws, &runner).unwrap();
    assert_eq!(
        ws.runs().unwrap()[0].ended(),
        None,
        "Cancel stopped nothing"
    );
    // Stop them all: the turn ends cleanly, noted as stopped by the quit.
    cx.simulate_keystrokes("secondary-q");
    redraw(cx);
    click_on(cx, "runs-stop");
    assert!(app.read_with(cx, |a, _| a.quitting()), "and quits");
    let start = Instant::now();
    loop {
        livtk_core::run::settle(&mut ws, &runner).unwrap();
        if let Some(how) = ws.runs().unwrap()[0].ended() {
            assert_eq!(how, livtk_core::event::Ending::Quit);
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "never stopped: {:?} {:?}",
            livtk_core::run::lives(ws.dir()),
            std::fs::read_dir(ws.dir().join("runs"))
                .unwrap()
                .flatten()
                .map(|e| (
                    e.file_name(),
                    std::fs::read_to_string(e.path()).unwrap_or_default()
                ))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// On the next start, the runs a quit stopped are offered back once: continued, each resumes
/// its own session where it left off.
#[gpui_kit::test]
fn runs_a_quit_stopped_are_offered_back(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path().canonicalize().unwrap();
    let _runs = KillRuns(root.clone());
    let runner = a_run_writing(&root);
    let mut ws = Workspace::open(&root).unwrap();
    runner
        .stop(
            &mut ws,
            "LIV-1",
            livtk_core::event::Ending::Quit,
            &Actor::Human,
        )
        .unwrap();
    let start = Instant::now();
    while ws.runs().unwrap()[0].ended().is_none() {
        livtk_core::run::settle(&mut ws, &runner).unwrap();
        assert!(start.elapsed() < Duration::from_secs(20), "never stopped");
        std::thread::sleep(Duration::from_millis(30));
    }
    let (app, cx) = open_app(cx, &root);
    until_runs(&app, cx, |m| m.runs.quit.len() == 1);
    assert!(cx.debug_bounds("continue-runs").is_some(), "offered back");
    click_on(cx, "runs-continue");
    redraw(cx);
    assert!(cx.debug_bounds("continue-runs").is_none());
    let start = Instant::now();
    let session = ws.runs().unwrap()[0].session.clone();
    loop {
        let args = std::fs::read_to_string(root.join(".stub-args")).unwrap_or_default();
        if args.lines().count() == 2 {
            assert!(args.contains(&format!("--resume {session}")), "{args}");
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "never continued: {args}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Keep them working: the app quits, the agent goes on.
#[gpui_kit::test]
fn keep_them_working_quits_and_leaves_the_agent_working(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path().canonicalize().unwrap();
    let _runs = KillRuns(root.clone());
    let runner = a_run_writing(&root);
    let (app, cx) = open_app(cx, &root);
    cx.simulate_keystrokes("secondary-q");
    redraw(cx);
    click_on(cx, "runs-keep");
    assert!(app.read_with(cx, |a, _| a.quitting()));
    let mut ws = Workspace::open(&root).unwrap();
    let live = livtk_core::run::lives(ws.dir()).pop().unwrap();
    assert!(livtk_core::run::writing(&live), "still working");
    // Done with it: stopped, so no stub is left behind.
    runner
        .stop(
            &mut ws,
            "LIV-1",
            livtk_core::event::Ending::Stopped,
            &Actor::Human,
        )
        .unwrap();
}

/// Quit heard with no window active (the window minimized, or another app in front) asks in
/// the window too, and the app-wide quit behind it does not run.
#[gpui_kit::test]
fn quit_with_no_window_active_asks_and_does_not_quit(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path().canonicalize().unwrap();
    let _runs = KillRuns(root.clone());
    let runner = a_run_writing(&root);
    let quit_anyway = std::rc::Rc::new(std::cell::Cell::new(false));
    // The app-wide quit, registered first as the app does.
    let flag = quit_anyway.clone();
    cx.update(|cx| {
        cx.on_action(move |_: &livtk_app::ui::Quit, _| flag.set(true));
    });
    let (app, cx) = open_app(cx, &root);
    let window = gpui_kit::VisualContext::window_handle(cx);
    cx.update(|_, cx| livtk_app::ui::dock_actions(window, app.downgrade(), cx));
    cx.deactivate_window();
    // Heard by the app, outside any window, as the menu bar sends it.
    cx.cx.update(|cx| cx.dispatch_action(&livtk_app::ui::Quit));
    redraw(cx);
    let quitting = app.read_with(cx, |a, _| a.quitting());
    assert!(
        cx.debug_bounds("quit-runs").is_some(),
        "asked in the window (app-wide quit ran: {}, quitting: {quitting})",
        quit_anyway.get()
    );
    assert!(!quit_anyway.get(), "and nothing quit behind it");
    let mut ws = Workspace::open(&root).unwrap();
    runner
        .stop(
            &mut ws,
            "LIV-1",
            livtk_core::event::Ending::Stopped,
            &Actor::Human,
        )
        .unwrap();
}

/// A task's run from its panel, without a terminal: given to an agent, started (Starting… at
/// once), asking to run a command (marked Needs you, in its row too), allowed, asking a
/// question, answered with a note, then writing, as its row shows, and stopped.
#[gpui_kit::test]
fn a_run_from_the_tasks_panel(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path().canonicalize().unwrap();
    let _runs = KillRuns(root.clone());
    claude_stub();
    std::fs::write(root.join(".stub-turns"), "deny\nask\nslow\n").unwrap();
    Workspace::open(&root)
        .unwrap()
        .execute(
            Command::Move {
                node: "LIV-1".into(),
                to: State::Acked,
            },
            &Actor::Human,
        )
        .unwrap();
    let (app, cx) = open_app(cx, &root);
    resize(cx, 1400., 1400.);
    app.update(cx, |a, _| a.model_mut().go_to("LIV-1"));
    settle(cx);
    // Given to an agent from the panel, once the runs are known (claude is there).
    until_runs(&app, cx, |m| m.runs.claude);
    // The run sits in the panel's title bar, above the properties.
    let (run, agent) = (
        cx.debug_bounds("detail-run").expect("the run"),
        cx.debug_bounds("detail-agent").expect("the agent"),
    );
    assert!(run.top() < agent.top(), "in the title bar");
    click_on(cx, "detail-agent");
    redraw(cx);
    click_on(cx, "set-agent-livtk:implementer");
    let agent = Workspace::open(&root).unwrap().node("LIV-1").unwrap().props["agent"].clone();
    assert_eq!(agent, "livtk:implementer");
    // Started: something shows at once.
    click_on(cx, "run-start");
    let args = |n: usize| {
        let start = Instant::now();
        loop {
            let a = std::fs::read_to_string(root.join(".stub-args")).unwrap_or_default();
            if let Some(l) = a.lines().nth(n - 1) {
                return l.to_owned();
            }
            assert!(start.elapsed() < Duration::from_secs(5), "turn {n}: {a}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    assert!(args(1).contains("--agent livtk:implementer"), "{}", args(1));
    // It asks to run a command: Needs you, on the note and in its row; Allow lets it.
    until_runs(&app, cx, |m| m.run_needs_you("LIV-1"));
    settle(cx);
    assert!(cx.debug_bounds("needs-you").is_some(), "on the note");
    assert!(cx.debug_bounds("row-needs-you").is_some(), "and in its row");
    click_on(cx, "run-allow");
    assert!(args(2).contains("Bash(cargo test)"), "{}", args(2));
    // Then a question, answered with a note: the run reads it next.
    until_runs(&app, cx, |m| m.run_needs_you("LIV-1"));
    settle(cx);
    assert!(
        cx.debug_bounds("run-allow").is_none(),
        "a question has no Allow"
    );
    cx.simulate_keystrokes("c");
    cx.run_until_parked();
    cx.simulate_input("Blue.");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    until_runs(&app, cx, |m| m.writing_on("LIV-1"));
    assert!(
        args(3).starts_with("-p Blue.") && args(3).contains("--resume"),
        "{}",
        args(3)
    );
    assert!(
        cx.debug_bounds("row-writing").is_some(),
        "writing, in its row"
    );
    // Stopped from the panel: the mark clears with the turn.
    click_on(cx, "run-stop");
    until_runs(&app, cx, |m| !m.writing_on("LIV-1"));
    assert!(cx.debug_bounds("row-writing").is_none());
}

/// A task waiting for unfinished work offers no Start in its panel, and says what it waits for.
#[gpui_kit::test]
fn a_blocked_task_offers_no_start(cx: &mut TestAppContext) {
    let dir = linked_project();
    let root = dir.path().canonicalize().unwrap();
    claude_stub();
    Workspace::open(&root)
        .unwrap()
        .execute(
            Command::Move {
                node: "LIV-2".into(),
                to: State::Acked,
            },
            &Actor::Human,
        )
        .unwrap();
    let (app, cx) = open_app(cx, &root);
    resize(cx, 1400., 900.);
    app.update(cx, |a, _| a.model_mut().go_to("LIV-2"));
    settle(cx);
    until_runs(&app, cx, |m| m.runs.claude);
    assert!(cx.debug_bounds("run-start").is_none(), "no Start");
    assert!(cx.debug_bounds("detail-run-status").is_some(), "but why");
    assert!(
        cx.debug_bounds("run-waits").is_some(),
        "with the blocked mark"
    );
}

/// Runs that start on their own, set in the project's settings: one build at once, so of two
/// acknowledged tasks given to an agent one builds and the other waits its turn, its panel and
/// Agents saying so.
#[gpui_kit::test]
fn builds_start_on_their_own_and_the_rest_wait_their_turn(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path().canonicalize().unwrap();
    let _runs = KillRuns(root.clone());
    claude_stub();
    std::fs::write(root.join(".stub-turns"), "slow\nslow\n").unwrap();
    {
        let mut ws = Workspace::open(&root).unwrap();
        for k in ["LIV-1", "LIV-2"] {
            ws.execute(
                Command::Move {
                    node: k.into(),
                    to: State::Acked,
                },
                &Actor::Human,
            )
            .unwrap();
            ws.execute(
                Command::SetAgent {
                    node: k.into(),
                    agent: "livtk:implementer".into(),
                },
                &Actor::Human,
            )
            .unwrap();
        }
    }
    let (app, cx) = open_app(cx, &root);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    // Settings, this project's Runs: on, one build at once.
    cx.simulate_keystrokes("secondary-,");
    redraw(cx);
    click_on(cx, "settings-runs");
    redraw(cx);
    click_on(cx, "settings-runs-auto-off");
    redraw(cx);
    click_on(cx, "settings-builds-1");
    redraw(cx);
    let config = std::fs::read_to_string(root.join(".livtk/config.toml")).unwrap();
    assert!(
        config.contains("auto = true") && config.contains("builds_at_once = 1"),
        "{config}"
    );
    cx.simulate_keystrokes("escape");
    redraw(cx);
    // One builds; the other waits its turn.
    until_runs(&app, cx, |m| {
        m.runs.going.len() == 1 && m.runs.queued.len() == 1
    });
    let waiting = app.read_with(cx, |a, _| a.model().runs.queued[0].key.clone());
    app.update(cx, |a, _| a.model_mut().go_to(&waiting));
    settle(cx);
    assert!(cx.debug_bounds("detail-run-status").is_some());
    let at = app.read_with(cx, |a, _| a.model().queued_at(&waiting));
    assert_eq!(at, Some(1), "first in line");
    cx.simulate_keystrokes("g e");
    redraw(cx);
    assert!(
        cx.debug_bounds(named(format!("queued-{waiting}")))
            .is_some(),
        "listed in Agents"
    );
}

/// Key hints (the keys beside buttons, rows and menus) are a setting of yours: shown unless
/// turned off in Appearance, kept through a restart; what they label works either way.
#[gpui_kit::test]
fn key_hints_are_a_setting(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    assert!(
        !app.read_with(cx, |a, _| a.model().prefs.hide_key_hints),
        "shown by default"
    );
    cx.simulate_keystrokes("secondary-,");
    redraw(cx);
    click_on(cx, "settings-key-hints-on");
    redraw(cx);
    assert!(
        app.read_with(cx, |a, _| a.model().prefs.hide_key_hints),
        "turned off"
    );
    // Kept through a restart.
    let saved = app.read_with(cx, |a, _| a.model().saved());
    let mut back = Model::open(&[dir.path().to_path_buf()]);
    back.restore(&saved);
    assert!(back.prefs.hide_key_hints);
    // Written by an older app, each choice at the top under its own name: read the same.
    let file = tempfile::tempdir().unwrap();
    let path = file.path().join("app.toml");
    std::fs::write(&path, "hide_state_flow = true\nhide_key_hints = true\n").unwrap();
    let old = livtk_app::model::Saved::load(&path);
    assert!(old.prefs.hide_state_flow && old.prefs.hide_key_hints);
    // And written back the same way.
    let text = toml::to_string(&old).unwrap();
    assert!(text.contains("hide_state_flow = true"), "{text}");
    // The rows still open what they open.
    click_on(cx, "settings-back");
    redraw(cx);
    click_on(cx, "shortcuts");
    assert!(app.read_with(cx, |a, _| a.help_open()), "the shortcuts");
}

/// P makes a project from anywhere, through the same form as a task: a bad target, or Claude as
/// owner, is refused and nothing made; filled right (owner, part of a project with nothing in it
/// yet), it is made active with its done-when ending its body, and shown in Projects with its
/// panel open. Switched off, the next starts proposed. N still makes a task.
#[gpui_kit::test]
fn p_makes_a_project_through_the_one_form(cx: &mut TestAppContext) {
    let dir = project();
    // A project with nothing in it yet, to make the new one part of.
    let outer = Workspace::open(dir.path())
        .unwrap()
        .new_project(
            &livtk_core::NewProject {
                title: "The bigger outcome".into(),
                done_when: "it is done".into(),
                commit: true,
                ..Default::default()
            },
            &Actor::Human,
        )
        .unwrap()[0]
        .op
        .subjects()
        .0
        .to_string();
    let outer = Workspace::open(dir.path())
        .unwrap()
        .node(&outer)
        .unwrap()
        .key;
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    cx.simulate_keystrokes("p");
    settle(cx);
    assert!(cx.debug_bounds("new-project-card").is_some(), "New project");
    cx.simulate_input("Ship the importer");
    click_on(cx, "new-project-done");
    cx.simulate_input("every record reads the same");
    click_on(cx, "new-project-target");
    cx.simulate_input("next week");
    click_on(cx, "create-project");
    redraw(cx);
    let projects = |dir: &std::path::Path| Workspace::open(dir).unwrap().projects().unwrap();
    assert_eq!(projects(dir.path()).len(), 1, "refused: nothing made");
    assert!(
        cx.debug_bounds("new-project-card").is_some(),
        "the form stays"
    );
    // Claude never owns it, whatever the case.
    click_on(cx, "new-project-owner");
    cx.simulate_input("Claude:implementer");
    // Put right.
    click_on(cx, "new-project-target");
    cx.simulate_keystrokes("secondary-a backspace");
    cx.simulate_input("2026-12-01");
    click_on(cx, "create-project");
    redraw(cx);
    assert!(projects(dir.path()).len() == 1, "refused: Claude as owner");
    click_on(cx, "new-project-owner");
    cx.simulate_keystrokes("secondary-a backspace");
    cx.simulate_input("Karl");
    // Part of the project with nothing in it yet: offered all the same.
    click_on(cx, "part-of-select");
    redraw(cx);
    click_on(cx, named(format!("part-of-{outer}")));
    click_on(cx, "create-project");
    settle(cx);
    let made: Vec<_> = projects(dir.path())
        .into_iter()
        .filter(|p| p.key != outer)
        .collect();
    assert_eq!(made.len(), 1);
    let n = Workspace::open(dir.path())
        .unwrap()
        .node(&made[0].key)
        .unwrap();
    assert_eq!(n.props["owner"], "Karl");
    let (part_of, _) = Workspace::open(dir.path())
        .unwrap()
        .related(&n.key, livtk_core::Rel::Parent)
        .unwrap();
    assert_eq!(part_of[0].key, outer, "part of it");
    assert_eq!(n.title, "Ship the importer");
    assert_eq!(n.state, State::Active, "committed to, made by you");
    assert!(
        n.body.ends_with("Done when: every record reads the same"),
        "{}",
        n.body
    );
    assert_eq!(n.props["target"], "2026-12-01");
    app.read_with(cx, |a, _| {
        assert_eq!(a.model().view(), View::Projects, "shown where projects are");
        assert!(!a.form_open());
        assert_eq!(
            a.model().detail.as_ref().map(|d| d.node.key.clone()),
            Some(n.key.clone()),
            "selected, its panel open"
        );
    });
    // Not committed to: proposed.
    cx.simulate_keystrokes("escape");
    redraw(cx);
    cx.simulate_keystrokes("p");
    settle(cx);
    cx.simulate_input("A maybe");
    click_on(cx, "new-project-done");
    cx.simulate_input("we know");
    click_on(cx, "new-project-commit-on");
    click_on(cx, "create-project");
    settle(cx);
    let maybe = projects(dir.path())
        .into_iter()
        .find(|p| p.title == "A maybe")
        .expect("made");
    assert_eq!(maybe.state, State::Proposed);
    // N is still a task.
    cx.simulate_keystrokes("n");
    settle(cx);
    assert!(cx.debug_bounds("new-task-card").is_some(), "New task");
}

/// A task's panel accounts for what it carries: how long it has been in its state, when it was
/// last changed and by whom, what it waits for, its sub-tasks, a prop no row knows (by its
/// name), and its project, picked and changed there; a sub-task's project is its parent's.
#[gpui_kit::test]
fn a_tasks_panel_shows_everything_it_carries(cx: &mut TestAppContext) {
    let dir = linked_project(); // LIV-2 waits for LIV-1
    let root = dir.path().canonicalize().unwrap();
    let (prj, sub) = {
        let mut ws = Workspace::open(&root).unwrap();
        let e = ws
            .new_project(
                &livtk_core::NewProject {
                    title: "Ship it".into(),
                    done_when: "shipped".into(),
                    commit: true,
                    ..Default::default()
                },
                &Actor::Human,
            )
            .unwrap();
        let prj = ws.node(&e[0].op.subjects().0.to_string()).unwrap().key;
        let e = ws
            .execute(
                Command::Create {
                    kind: Kind::Task,
                    title: "a part of the first".into(),
                    body: String::new(),
                    priority: None,
                    links: vec![(livtk_core::Rel::Parent, "LIV-1".into())],
                },
                &CLAUDE,
            )
            .unwrap();
        let sub = ws.node(&e[0].op.subjects().0.to_string()).unwrap().key;
        // A prop set by some tool the panel has no row for.
        let node = ws.node("LIV-2").unwrap().id;
        let mut props = serde_json::Map::new();
        props.insert("estimate".into(), serde_json::Value::String("3h".into()));
        let e = livtk_core::Event::new(
            ws.newest_event().unwrap(),
            &CLAUDE,
            livtk_core::Op::NodeSet {
                node,
                data: livtk_core::event::NodePatch {
                    props: Some(props),
                    ..Default::default()
                },
            },
        );
        use std::io::Write as _;
        let mut log = std::fs::OpenOptions::new()
            .append(true)
            .open(root.join(".livtk/events.jsonl"))
            .unwrap();
        writeln!(log, "{}", e.to_line().unwrap()).unwrap();
        (prj, sub)
    };
    let (app, cx) = open_app(cx, &root);
    resize(cx, 1400., 1400.);
    app.update(cx, |a, _| a.model_mut().go_to("LIV-2"));
    settle(cx);
    for sel in [
        "detail-state-since",
        "detail-meta",
        "detail-blocked",
        "detail-prop-estimate",
        "detail-project",
    ] {
        assert!(cx.debug_bounds(sel).is_some(), "{sel}");
    }
    assert!(
        cx.debug_bounds("blocked-by-LIV-1").is_some(),
        "naming what it waits for"
    );
    // Its project, picked from the panel, then taken back.
    click_on(cx, "detail-project");
    redraw(cx);
    click_on(cx, named(format!("set-project-{prj}")));
    settle(cx);
    let in_project = |k: &str| {
        Workspace::open(&root)
            .unwrap()
            .project_of(k)
            .unwrap()
            .map(|p| p.key)
    };
    assert_eq!(in_project("LIV-2"), Some(prj.clone()));
    app.read_with(cx, |a, _| {
        let d = a.model().detail.as_ref().unwrap();
        assert_eq!(
            d.in_project.as_ref().map(|p| p.key.clone()),
            Some(prj.clone()),
            "the panel follows"
        );
        assert!(
            matches!(d.history.last().unwrap().op, livtk_core::Op::EdgeAdd { .. }),
            "its last change is the move, as Updated says"
        );
    });
    click_on(cx, "detail-project");
    redraw(cx);
    click_on(cx, "set-project-none");
    settle(cx);
    assert_eq!(in_project("LIV-2"), None);
    // A sub-task of a task in no project: none, read-only, naming its parent.
    app.update(cx, |a, _| a.model_mut().go_to(&sub));
    settle(cx);
    assert!(
        cx.debug_bounds("detail-project-through").is_some(),
        "names its parent"
    );
    assert!(cx.debug_bounds("detail-project").is_none());
    // The first, with a sub-task, in the project: its sub-task's project is through it.
    Workspace::open(&root)
        .unwrap()
        .set_project_of("LIV-1", Some(&prj), &Actor::Human)
        .unwrap();
    app.update(cx, |a, _| a.model_mut().go_to("LIV-1"));
    settle(cx);
    assert!(
        cx.debug_bounds("detail-sub-tasks").is_some(),
        "its sub-tasks"
    );
    app.update(cx, |a, _| a.model_mut().go_to(&sub));
    settle(cx);
    assert!(
        cx.debug_bounds("detail-project-through").is_some(),
        "through its parent"
    );
    assert!(
        cx.debug_bounds("detail-project").is_none(),
        "changed on the parent"
    );
    // A canceled sub-task is out of the count; a closed task waits for nothing.
    {
        let mut ws = Workspace::open(&root).unwrap();
        for k in [sub.as_str(), "LIV-2"] {
            ws.execute(
                Command::Move {
                    node: k.into(),
                    to: State::Canceled,
                },
                &Actor::Human,
            )
            .unwrap();
        }
    }
    app.update(cx, |a, _| a.model_mut().go_to("LIV-1"));
    settle(cx);
    assert!(
        cx.debug_bounds("detail-sub-tasks").is_none(),
        "its only sub-task canceled: none to count"
    );
    app.update(cx, |a, _| a.model_mut().go_to("LIV-2"));
    settle(cx);
    assert!(
        cx.debug_bounds("detail-blocked").is_none(),
        "canceled: waits for nothing"
    );
}

/// The title is edited where it reads: the same place and size, no background of its own;
/// Enter saves, Esc keeps the old one. When it was made and changed is a quiet line under it.
#[gpui_kit::test]
fn the_title_is_edited_where_it_reads(cx: &mut TestAppContext) {
    let dir = project();
    // A title long enough to wrap: edited, it wraps the same, never one scrolling line.
    let long = "A title long enough to wrap onto a second line and then a third one in the \
                panel, as long titles do when a task says a lot about itself up front";
    Workspace::open(dir.path())
        .unwrap()
        .execute(
            Command::Edit {
                node: "LIV-1".into(),
                title: Some(long.into()),
                body: None,
            },
            &Actor::Human,
        )
        .unwrap();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    open_detail(cx);
    assert!(
        cx.debug_bounds("detail-meta").is_some(),
        "made and changed, under the title"
    );
    let reading = cx.debug_bounds("detail-title").expect("the title");
    let meta = cx.debug_bounds("detail-meta").unwrap();
    assert!(
        reading.size.height > gpui_kit::px(60.),
        "it wraps: {:?}",
        reading.size
    );
    double_click_on(cx, "detail-title");
    redraw(cx);
    let editing = cx.debug_bounds("title-editor").expect("editing in place");
    assert_eq!(editing.origin, reading.origin, "where it was");
    assert_eq!(
        editing.size.height, reading.size.height,
        "as tall as it was, every line shown"
    );
    assert_eq!(
        cx.debug_bounds("detail-meta").unwrap().origin,
        meta.origin,
        "nothing under it moves"
    );
    // Esc keeps the old title.
    cx.simulate_input(" changed");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(node_of(dir.path(), "LIV-1").title, long, "Esc keeps it");
    app.read_with(cx, |a, _| assert_eq!(a.editing(), None));
    // The word double-clicked is picked editing: typing replaces it, in the middle.
    double_click_on(cx, "detail-title");
    cx.simulate_input("§");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let saved = node_of(dir.path(), "LIV-1").title;
    let at = saved.find('§').expect("typed");
    assert!(
        at > 10 && at < long.len() - 10,
        "typed where it was double-clicked, at {at} of {}: {saved}",
        long.len()
    );
    let word = &long[at..long.len() - (saved.len() - at - '§'.len_utf8())];
    assert!(
        !word.is_empty() && word.chars().all(char::is_alphanumeric),
        "a whole word replaced: {word:?}"
    );
}

/// Editing a long body down the panel: the panel keeps its scroll, the editor starts where the
/// body read; a click anywhere outside it (not only on something that takes the keys) saves it
/// and leaves editing.
#[gpui_kit::test]
fn a_body_edited_down_the_panel_keeps_its_place_and_a_click_outside_saves(cx: &mut TestAppContext) {
    let dir = project();
    let body: String = (1..=60)
        .map(|i| format!("Line {i} of a long body.\n\n"))
        .collect();
    Workspace::open(dir.path())
        .unwrap()
        .execute(
            Command::Edit {
                node: "LIV-1".into(),
                title: None,
                body: Some(body.trim_end().into()),
            },
            &Actor::Human,
        )
        .unwrap();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    open_detail(cx);
    let at = cx.debug_bounds("detail-title").unwrap().center();
    cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position: at,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
            gpui_kit::px(0.),
            gpui_kit::px(-600.),
        )),
        modifiers: gpui_kit::Modifiers::default(),
        ..Default::default()
    });
    redraw(cx);
    let reading = cx.debug_bounds("detail-body").expect("the body");
    assert!(
        reading.origin.y < gpui_kit::px(0.),
        "scrolled down to it: {reading:?}"
    );
    // The block a third of the way down the window, every point of the body being one's;
    // double-clicked on its text.
    let row = gpui_kit::point(reading.center().x, gpui_kit::px(400.));
    let block = (0..60)
        .filter_map(|i| cx.debug_bounds(Box::leak(format!("body-block-{i}").into_boxed_str())))
        .find(|b| b.contains(&row))
        .expect("a block there");
    let click = gpui_kit::point(
        block.origin.x + gpui_kit::px(20.),
        block.origin.y + gpui_kit::px(11.),
    );
    double_click_at(cx, click);
    redraw(cx);
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.editing(), Some("body")));
    let editing = cx
        .debug_bounds("body-block-editor")
        .expect("editing in place");
    assert_eq!(editing.origin, block.origin, "the panel kept its scroll");
    assert_eq!(
        editing.size.height, block.size.height,
        "and the block its height"
    );
    // A click on the panel's plain space outside it saves and leaves.
    cx.simulate_keystrokes("secondary-down");
    cx.simulate_input("\n\nMore.");
    // Back up to the top, where the line under the title is plain space.
    cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position: click,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
            gpui_kit::px(0.),
            gpui_kit::px(5000.),
        )),
        modifiers: gpui_kit::Modifiers::default(),
        ..Default::default()
    });
    redraw(cx);
    click_on(cx, "detail-meta");
    redraw(cx);
    app.read_with(cx, |a, _| assert_eq!(a.editing(), None, "left editing"));
    assert!(
        node_of(dir.path(), "LIV-1").body.contains(".\n\nMore.\n\n"),
        "saved, a block of its own after the one clicked"
    );
}

/// A title wraps the same edited as read at every length, near each place a line breaks too:
/// opening it never drops a word to a new line.
#[gpui_kit::test]
fn a_title_wraps_the_same_edited_at_every_length(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    open_detail(cx);
    let words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi \
                 omicron pi rho sigma tau upsilon phi chi psi omega";
    let words: Vec<&str> = words.split_whitespace().collect();
    let mut differ = Vec::new();
    for n in 8..=words.len() {
        for tail in ["", " W", " iii"] {
            let title = format!("{}{tail}", words[..n].join(" "));
            app.update(cx, |a, cx| {
                let id = a.model().detail.as_ref().unwrap().node.id;
                a.model_mut().edit(id, Some(&title), None);
                cx.notify();
            });
            redraw(cx);
            let reading = cx.debug_bounds("detail-title").unwrap();
            double_click_on(cx, "detail-title");
            redraw(cx);
            let editing = cx.debug_bounds("title-editor").expect("editing");
            if editing.size.height != reading.size.height {
                differ.push(format!(
                    "{title}: {:?} -> {:?}",
                    reading.size.height, editing.size.height
                ));
            }
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
        }
    }
    assert!(
        differ.is_empty(),
        "wrapped differently:\n{}",
        differ.join("\n")
    );
}

/// A body that holds a little of everything, block by block.
const RICH_BODY: &str = "## Goal\n\nShip the **block editor** with [links](https://x.y) and the caret where it was clicked.\n\n```rust\nfn main() {}\n```\n\n- [ ] first box\n- [x] second box\n\nLast paragraph.";

fn rich_body(dir: &std::path::Path) {
    Workspace::open(dir)
        .unwrap()
        .execute(
            Command::Edit {
                node: "LIV-1".into(),
                title: None,
                body: Some(RICH_BODY.into()),
            },
            &Actor::Human,
        )
        .unwrap();
}

/// Each block of a body edits in its own place, at its own style: a heading, a paragraph with
/// markup, a code block; the rest stay drawn; a click puts the caret where it fell, past the
/// markup before it; Esc drops the edit, ⌘Enter keeps it; nothing else in the body changes.
#[gpui_kit::test]
fn each_block_of_a_body_edits_in_its_place(cx: &mut TestAppContext) {
    let dir = project();
    rich_body(dir.path());
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 1200.);
    open_detail(cx);
    redraw(cx);
    for (i, what) in [(0, "heading"), (1, "paragraph"), (2, "code")] {
        let sel: &'static str = Box::leak(format!("body-block-{i}").into_boxed_str());
        let reading = cx
            .debug_bounds(sel)
            .unwrap_or_else(|| panic!("{what} drawn"));
        double_click_on(cx, sel);
        redraw(cx);
        app.read_with(cx, |a, _| assert_eq!(a.editing(), Some("body"), "{what}"));
        let editing = cx.debug_bounds("body-block-editor").expect("editing");
        assert_eq!(
            editing.origin, reading.origin,
            "{what} edits where it reads"
        );
        if what != "code" {
            assert_eq!(editing.size.height, reading.size.height, "{what}: as tall");
        }
        // The other blocks stay drawn.
        assert!(
            cx.debug_bounds("body-block-3").is_some(),
            "the list still drawn"
        );
        cx.simulate_input("XYZ");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(
            node_of(dir.path(), "LIV-1").body,
            RICH_BODY,
            "Esc dropped it"
        );
        redraw(cx);
    }
    // A double-click picks the word under it in the source, past the markup around it: typing
    // replaces exactly that word, inside bold, inside a link's text, after both.
    let block = cx.debug_bounds("body-block-1").unwrap();
    let y = block.origin.y + gpui_kit::px(11.);
    let mut words = Vec::new();
    for step in 0..24 {
        // On the text: past the room the block hangs into the margin.
        let at = gpui_kit::point(block.origin.x + gpui_kit::px(12. + step as f32 * 18.), y);
        double_click_at(cx, at);
        redraw(cx);
        if app.read_with(cx, |a, _| a.editing()).is_none() {
            continue;
        }
        cx.simulate_input("§");
        cx.simulate_keystrokes("secondary-enter");
        cx.run_until_parked();
        let saved = node_of(dir.path(), "LIV-1").body;
        let mark = saved.find('§').expect("typed");
        let word = &RICH_BODY[mark..mark + RICH_BODY.len() + '§'.len_utf8() - saved.len()];
        // Past the line's end there is no word: the caret goes to the block's end.
        if word.is_empty() {
            assert_eq!(saved, RICH_BODY.replacen("clicked.", "clicked.§", 1));
            app.update(cx, |a, cx| {
                let id = a.model().detail.as_ref().unwrap().node.id;
                a.model_mut().edit(id, None, Some(RICH_BODY));
                cx.notify();
            });
            redraw(cx);
            continue;
        }
        // A word, or the space between two.
        assert!(
            !word.is_empty() && (word.chars().all(char::is_alphanumeric) || word.trim().is_empty()),
            "a whole word, its markup kept: {word:?} in {saved:?}"
        );
        assert_eq!(
            saved.replacen('§', word, 1),
            RICH_BODY,
            "only that word changed"
        );
        words.push(word.to_owned());
        // Back as it was, for the next.
        app.update(cx, |a, cx| {
            let id = a.model().detail.as_ref().unwrap().node.id;
            a.model_mut().edit(id, None, Some(RICH_BODY));
            cx.notify();
        });
        redraw(cx);
    }
    for want in ["Ship", "block", "editor", "links", "and"] {
        assert!(words.iter().any(|w| w == want), "{want} picked: {words:?}");
    }
}

/// Up on a block's first line walks into the block above, Down on its last into the block
/// below, saving as it goes; Down past the last block starts a new one.
#[gpui_kit::test]
fn arrows_walk_a_body_block_by_block(cx: &mut TestAppContext) {
    let dir = project();
    rich_body(dir.path());
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 1200.);
    open_detail(cx);
    redraw(cx);
    let editing_text = |app: &gpui_kit::Entity<LivtkApp>, cx: &mut gpui_kit::VisualTestContext| {
        app.read_with(cx, |a, cx| a.editing_text(cx))
    };
    double_click_on(cx, "body-block-1");
    redraw(cx);
    cx.simulate_keystrokes("secondary-up");
    cx.simulate_keystrokes("up");
    redraw(cx);
    assert_eq!(
        editing_text(&app, cx).as_deref(),
        Some("## Goal"),
        "up into the heading"
    );
    cx.simulate_keystrokes("up");
    redraw(cx);
    assert_eq!(
        editing_text(&app, cx).as_deref(),
        Some("## Goal"),
        "nothing above the first"
    );
    cx.simulate_keystrokes("down");
    redraw(cx);
    assert!(
        editing_text(&app, cx).unwrap().starts_with("Ship the"),
        "down into the paragraph"
    );
    // To the last block, then past it: a new one, typed into and kept.
    double_click_on(cx, "body-block-4");
    redraw(cx);
    cx.simulate_keystrokes("secondary-down");
    cx.simulate_keystrokes("down");
    redraw(cx);
    assert_eq!(editing_text(&app, cx).as_deref(), Some(""), "a new block");
    cx.simulate_input("Added.");
    cx.simulate_keystrokes("secondary-enter");
    cx.run_until_parked();
    assert_eq!(
        node_of(dir.path(), "LIV-1").body,
        format!("{RICH_BODY}\n\nAdded.")
    );
}

/// A task box in a body ticks and unticks from reading, with one click, without opening the
/// list to edit; only its mark changes in the body.
#[gpui_kit::test]
fn a_task_box_ticks_from_reading(cx: &mut TestAppContext) {
    let dir = project();
    rich_body(dir.path());
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 1200.);
    open_detail(cx);
    redraw(cx);
    let first = RICH_BODY.find("[ ] first").unwrap() + 1;
    let second = RICH_BODY.find("[x] second").unwrap() + 1;
    let first_box: &'static str = Box::leak(format!("task-box-{first}").into_boxed_str());
    let second_box: &'static str = Box::leak(format!("task-box-{second}").into_boxed_str());
    click_on(cx, first_box);
    redraw(cx);
    app.read_with(cx, |a, _| {
        assert_eq!(a.editing(), None, "not opened to edit")
    });
    assert_eq!(
        node_of(dir.path(), "LIV-1").body,
        RICH_BODY.replace("[ ] first", "[x] first"),
        "ticked, nothing else changed"
    );
    click_on(cx, second_box);
    redraw(cx);
    assert_eq!(
        node_of(dir.path(), "LIV-1").body,
        RICH_BODY
            .replace("[ ] first", "[x] first")
            .replace("[x] second", "[ ] second"),
        "the other unticked"
    );
}

/// The body written elsewhere while a block of it is open (by an agent, or the CLI): saving
/// the block unchanged keeps the body as it now is; saving it changed writes it where its text
/// now is, nothing duplicated and nothing lost.
#[gpui_kit::test]
fn a_block_saves_into_the_body_as_it_now_is(cx: &mut TestAppContext) {
    let dir = project();
    rich_body(dir.path());
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 1200.);
    open_detail(cx);
    redraw(cx);
    let elsewhere =
        |app: &gpui_kit::Entity<LivtkApp>, cx: &mut gpui_kit::VisualTestContext, body: &str| {
            let body = body.to_owned();
            app.update(cx, |a, cx| {
                let id = a.model().detail.as_ref().unwrap().node.id;
                a.model_mut().edit(id, None, Some(&body));
                cx.notify();
            });
            redraw(cx);
        };
    // Unchanged: the body as written elsewhere stays.
    double_click_on(cx, "body-block-4");
    redraw(cx);
    let moved = format!("Inserted first.\n\n{RICH_BODY}");
    elsewhere(&app, cx, &moved);
    cx.simulate_keystrokes("secondary-enter");
    cx.run_until_parked();
    assert_eq!(
        node_of(dir.path(), "LIV-1").body,
        moved,
        "nothing duplicated"
    );
    // Changed: written where its text now is.
    double_click_on(cx, "body-block-5");
    redraw(cx);
    assert_eq!(
        app.read_with(cx, |a, cx| a.editing_text(cx)).as_deref(),
        Some("Last paragraph.")
    );
    let moved_again = format!("Again.\n\n{moved}");
    elsewhere(&app, cx, &moved_again);
    cx.simulate_keystrokes("secondary-right");
    cx.simulate_input(" More.");
    cx.simulate_keystrokes("secondary-enter");
    cx.run_until_parked();
    assert_eq!(
        node_of(dir.path(), "LIV-1").body,
        moved_again.replace("Last paragraph.", "Last paragraph. More."),
        "written where it now is"
    );
}

/// A block emptied, then Down: the block that was below it opens, not a new one at the end.
#[gpui_kit::test]
fn down_from_an_emptied_block_opens_the_next(cx: &mut TestAppContext) {
    let dir = project();
    rich_body(dir.path());
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 1200.);
    open_detail(cx);
    redraw(cx);
    double_click_on(cx, "body-block-1");
    redraw(cx);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("   ");
    cx.simulate_keystrokes("down");
    redraw(cx);
    assert_eq!(
        app.read_with(cx, |a, cx| a.editing_text(cx)).as_deref(),
        Some("```rust\nfn main() {}\n```"),
        "the code block that was below"
    );
    assert!(
        !node_of(dir.path(), "LIV-1").body.contains("Ship"),
        "the emptied one gone"
    );
}

/// The bounds of a timeline row's mark, by its place on the rail.
fn rail_mark(
    cx: &mut gpui_kit::VisualTestContext,
    order: usize,
) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    cx.debug_bounds(Box::leak(format!("timeline-mark-{order}").into_boxed_str()))
}

/// A task's activity is one rail, oldest first: each event a mark on it in order; the work of
/// a Claude session on a lane of its own; small changes in a row folded until opened; the note
/// composer at the rail's next point, after the latest event and before the states ahead; a note
/// sent from it joins the rail there.
#[gpui_kit::test]
fn a_tasks_activity_is_one_rail(cx: &mut TestAppContext) {
    let dir = project();
    {
        let mut ws = Workspace::open(dir.path()).unwrap();
        for title in ["a", "b", "c", "d", "e"] {
            ws.execute(
                Command::Edit {
                    node: "LIV-1".into(),
                    title: Some(title.into()),
                    body: None,
                },
                &Actor::Human,
            )
            .unwrap();
        }
        ws.set_session(Some("sess-r".into()));
        for text in ["on it", "done here"] {
            ws.execute(
                Command::Note {
                    node: "LIV-1".into(),
                    text: text.into(),
                },
                &CLAUDE,
            )
            .unwrap();
        }
    }
    let history = Workspace::open(dir.path())
        .unwrap()
        .history(Some("LIV-1"))
        .unwrap();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 1200.);
    open_detail(cx);
    redraw(cx);
    // Oldest first, down the rail.
    let first = rail_mark(cx, 0).expect("the first event's mark");
    let second = rail_mark(cx, 1).expect("the next");
    assert!(first.origin.y < second.origin.y, "oldest first");
    // The five renames fold past the first.
    let fold = (0..history.len() + 4)
        .find_map(|i| {
            let sel: &'static str = Box::leak(
                format!("timeline-fold-{}", history[i.min(history.len() - 1)].id).into_boxed_str(),
            );
            cx.debug_bounds(sel).map(|_| sel)
        })
        .expect("the renames folded");
    let rows_before = (0..40).filter(|&i| rail_mark(cx, i).is_some()).count();
    click_on(cx, fold);
    redraw(cx);
    let rows_after = (0..40).filter(|&i| rail_mark(cx, i).is_some()).count();
    assert!(
        rows_after >= rows_before + 3,
        "opened: the folded changes show ({rows_before} -> {rows_after})"
    );
    assert!(cx.debug_bounds(fold).is_none(), "the fold goes");
    // Claude's session on its own lane, right of the rail.
    let marks: Vec<_> = (0..rows_after).filter_map(|i| rail_mark(cx, i)).collect();
    let rail_x = marks[0].origin.x;
    assert!(
        marks.iter().any(|m| m.origin.x > rail_x),
        "a lane of its own for the session"
    );
    // The composer after the latest event, before what is ahead.
    let composer = cx
        .debug_bounds("note-composer")
        .expect("the composer on the rail");
    let latest = marks
        .iter()
        .filter(|m| m.origin.y < composer.origin.y)
        .count();
    assert!(latest >= history.len(), "every event above the composer");
    assert!(
        marks.iter().any(|m| m.origin.y > composer.origin.y),
        "the states ahead below it"
    );
    // A note sent from it joins the rail just above it.
    cx.simulate_keystrokes("c");
    cx.simulate_input("from the rail");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    redraw(cx);
    let all = Workspace::open(dir.path())
        .unwrap()
        .history(Some("LIV-1"))
        .unwrap();
    assert_eq!(all.len(), history.len() + 1);
    let composer_after = cx.debug_bounds("note-composer").unwrap();
    assert!(
        composer_after.origin.y > composer.origin.y,
        "the composer moved down past it"
    );
    app.read_with(cx, |a, cx| assert_eq!(a.note_text(cx), ""));
}

/// On a long timeline, what heads the rows in view sticks to the panel's edges: the composer at
/// the foot while its place on the rail is below the view, a Claude session's first row at the
/// top while its lane runs on in view; scrolled to its place, each goes back in line.
#[gpui_kit::test]
fn the_timeline_sticks_what_heads_the_view(cx: &mut TestAppContext) {
    let dir = project();
    {
        let mut ws = Workspace::open(dir.path()).unwrap();
        ws.set_session(Some("sess-long".into()));
        for i in 0..30 {
            ws.execute(
                Command::Note {
                    node: "LIV-1".into(),
                    text: format!("step {i} of a long run"),
                },
                &CLAUDE,
            )
            .unwrap();
        }
    }
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    open_detail(cx);
    redraw(cx);
    redraw(cx);
    let stuck = |cx: &mut gpui_kit::VisualTestContext| {
        (0..80)
            .filter(|i| {
                cx.debug_bounds(Box::leak(format!("timeline-stuck-{i}").into_boxed_str()))
                    .is_some()
            })
            .count()
    };
    let composer = cx.debug_bounds("note-composer").expect("the composer");
    let panel = cx.debug_bounds("detail-body-panel").unwrap();
    assert!(
        composer.bottom() <= panel.bottom(),
        "stuck at the foot, in reach: {composer:?} in {panel:?}"
    );
    assert!(stuck(cx) >= 1, "the composer stuck");
    // Down the timeline: the session's first row stuck at the top, its lane running on below.
    let at = cx.debug_bounds("detail-title").unwrap().center();
    let mut lane_start_stuck = false;
    for _ in 0..12 {
        cx.simulate_event(gpui_kit::ScrollWheelEvent {
            position: at,
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                gpui_kit::px(0.),
                gpui_kit::px(-150.),
            )),
            modifiers: gpui_kit::Modifiers::default(),
            ..Default::default()
        });
        redraw(cx);
        redraw(cx);
        // The session's lane starts at the third row (after made and moved).
        lane_start_stuck |= cx.debug_bounds("timeline-stuck-2").is_some();
    }
    redraw(cx);
    assert!(
        lane_start_stuck,
        "the session's first row stuck at the top while its lane ran on"
    );
    // At the end: the composer back in its place, nothing stuck at the foot; the lane's start
    // stuck at the top while its marks are in view.
    let composer_end = cx.debug_bounds("note-composer").expect("the composer");
    assert!(composer_end.bottom() <= panel.bottom());
    app.read_with(cx, |a, cx| assert_eq!(a.note_text(cx), ""));
    cx.simulate_keystrokes("c");
    cx.simulate_input("typed while stuck or not");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let all = Workspace::open(dir.path())
        .unwrap()
        .history(Some("LIV-1"))
        .unwrap();
    assert!(matches!(
        &all.last().unwrap().op,
        livtk_core::event::Op::NodeNote { data, .. } if data.text == "typed while stuck or not"
    ));
}

/// A staged build's restart is offered wherever the sidebar is: a row that fits the sidebar at
/// any width, a button on the collapsed rail, a command in the palette; each restarts. With
/// nothing staged, none of them shows.
#[gpui_kit::test]
fn restart_to_update_is_offered_at_any_sidebar_width(cx: &mut TestAppContext) {
    let dir = project();
    let (app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 900.);
    redraw(cx);
    assert!(
        cx.debug_bounds("restart-to-update").is_none(),
        "nothing staged"
    );
    app.update(cx, |a, cx| {
        a.stage_update_for_test(267);
        cx.notify();
    });
    redraw(cx);
    // At the sidebar's narrowest, the row stays inside it.
    drag_to(cx, "sidebar-edge", 170.);
    redraw(cx);
    let side = cx.debug_bounds("sidebar").unwrap();
    let row = cx.debug_bounds("restart-to-update").expect("the row");
    assert!(
        row.right() <= side.right() + gpui_kit::px(0.5),
        "inside the sidebar: {row:?} in {side:?}"
    );
    click_on(cx, "restart-to-update");
    assert_eq!(app.read_with(cx, |a, _| a.restarts_asked()), 1);
    // Collapsed: a button on the rail.
    cx.simulate_keystrokes("secondary-b");
    redraw(cx);
    assert!(cx.debug_bounds("restart-to-update").is_none());
    click_on(cx, "rail-update");
    assert_eq!(app.read_with(cx, |a, _| a.restarts_asked()), 2);
    // The palette offers it too.
    cx.simulate_keystrokes("secondary-k");
    cx.simulate_input("restart to update");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(app.read_with(cx, |a, _| a.restarts_asked()), 3);
}

/// Asked to run a command, a run can be allowed it always: the kind of command goes into the
/// project's rules for its stage, shown and removable in Settings → Runs, and the run goes on. A
/// task says where its builds work: its own worktree, or the checkout.
#[gpui_kit::test]
fn a_run_is_allowed_always_and_a_task_says_where_it_builds(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path().canonicalize().unwrap();
    let _runs = KillRuns(root.clone());
    claude_stub();
    std::fs::write(root.join(".stub-turns"), "deny\nslow\n").unwrap();
    Workspace::open(&root)
        .unwrap()
        .execute(
            Command::Move {
                node: "LIV-1".into(),
                to: State::Acked,
            },
            &Actor::Human,
        )
        .unwrap();
    let (app, cx) = open_app(cx, &root);
    resize(cx, 1400., 1400.);
    app.update(cx, |a, _| a.model_mut().go_to("LIV-1"));
    settle(cx);
    until_runs(&app, cx, |m| m.runs.claude);
    // Where it builds: off its own worktree, as the task says.
    click_on(cx, "detail-worktree");
    redraw(cx);
    click_on(cx, "set-worktree-off");
    let node = Workspace::open(&root).unwrap().node("LIV-1").unwrap();
    assert_eq!(node.props["worktree"], false);
    // Started, it asks; allowed always.
    click_on(cx, "run-start");
    until_runs(&app, cx, |m| m.run_needs_you("LIV-1"));
    settle(cx);
    assert!(
        cx.debug_bounds("run-allow").is_some(),
        "once, still offered"
    );
    click_on(cx, "run-allow-always");
    let ws = Workspace::open(&root).unwrap();
    let own = ws
        .config()
        .runs
        .allow
        .get("build")
        .cloned()
        .unwrap_or_default();
    assert!(own.contains(&"Bash(cargo test:*)".to_owned()), "{own:?}");
    until_runs(&app, cx, |m| m.writing_on("LIV-1"));
    // In Settings → Runs, the rule shows and goes.
    click_on(cx, "run-stop");
    until_runs(&app, cx, |m| !m.writing_on("LIV-1"));
    cx.simulate_keystrokes("secondary-,");
    redraw(cx);
    click_on(cx, "settings-runs");
    redraw(cx);
    click_on(cx, "settings-rule-build-Bash(cargo test:*)");
    redraw(cx);
    let own = Workspace::open(&root)
        .unwrap()
        .config()
        .runs
        .allow
        .get("build")
        .cloned()
        .unwrap_or_default();
    assert!(
        !own.contains(&"Bash(cargo test:*)".to_owned()),
        "removed: {own:?}"
    );
}

/// A build runs with its whole job allowed (cargo, the checks, commits, edits) and what is never
/// allowed refused: a push it tries is refused and said on the task, and it goes on without a
/// Needs you. Where it works is chosen beside Start.
#[gpui_kit::test]
fn a_build_does_its_job_and_a_push_is_refused_without_asking(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path().canonicalize().unwrap();
    let _runs = KillRuns(root.clone());
    claude_stub();
    std::fs::write(root.join(".stub-turns"), "push\nslow\n").unwrap();
    Workspace::open(&root)
        .unwrap()
        .execute(
            Command::Move {
                node: "LIV-1".into(),
                to: State::Acked,
            },
            &Actor::Human,
        )
        .unwrap();
    let (app, cx) = open_app(cx, &root);
    resize(cx, 1400., 1400.);
    app.update(cx, |a, _| a.model_mut().go_to("LIV-1"));
    settle(cx);
    until_runs(&app, cx, |m| m.runs.claude);
    // Beside Start: where it builds, switched for this task.
    click_on(cx, "run-worktree");
    redraw(cx);
    let node = Workspace::open(&root).unwrap().node("LIV-1").unwrap();
    assert_eq!(node.props["worktree"], false, "switched to the checkout");
    click_on(cx, "run-start");
    let args = |n: usize| {
        let start = Instant::now();
        loop {
            let a = std::fs::read_to_string(root.join(".stub-args")).unwrap_or_default();
            if let Some(l) = a.lines().nth(n - 1) {
                return l.to_owned();
            }
            assert!(start.elapsed() < Duration::from_secs(5), "turn {n}: {a}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let first = args(1);
    for rule in [
        "Bash(cargo:*)",
        "Bash(scripts/check:*)",
        "Bash(git commit:*)",
        "Edit",
    ] {
        assert!(first.contains(rule), "{rule} allowed: {first}");
    }
    assert!(
        first.contains("--disallowedTools") && first.contains("Bash(git push:*)"),
        "{first}"
    );
    // The push refused: said on the task, and the run goes on to its next turn by itself.
    until_runs(&app, cx, |_| {
        std::fs::read_to_string(root.join(".stub-args"))
            .unwrap_or_default()
            .lines()
            .count()
            >= 2
    });
    let second = args(2);
    assert!(
        second.contains("Not allowed: Bash(git push origin main)"),
        "{second}"
    );
    until_runs(&app, cx, |m| m.writing_on("LIV-1"));
    assert!(
        !app.read_with(cx, |a, _| a.model().run_needs_you("LIV-1")),
        "never asked"
    );
    let said = Workspace::open(&root)
        .unwrap()
        .history(Some("LIV-1"))
        .unwrap();
    assert!(said.iter().any(
        |e| matches!(&e.op, livtk_core::event::Op::NodeNote { data, .. }
        if data.text.contains("Not allowed, ever"))
    ));
    click_on(cx, "run-stop");
    until_runs(&app, cx, |m| !m.writing_on("LIV-1"));
}

/// A run's last word, several lines of markdown, reads as a card on the rail: every row of the
/// timeline below the one above, nothing drawn over another.
#[gpui_kit::test]
fn a_runs_long_last_note_never_spills_over_the_rows_below(cx: &mut TestAppContext) {
    let dir = project();
    {
        let mut ws = Workspace::open(dir.path()).unwrap();
        let run = |mark| livtk_core::event::RunData {
            session: "sess-long".into(),
            agent: None,
            stage: livtk_core::run::Stage::Plan,
            mark,
        };
        for (mark, text) in [
            (livtk_core::event::RunMark::Started, "Started to plan it."),
            (
                livtk_core::event::RunMark::Ended {
                    how: livtk_core::event::Ending::Done,
                },
                "LIV-1 now has a plan and is in **planned**.\n\n**The plan**\n- one\n- two\n- three",
            ),
        ] {
            ws.execute(
                Command::RunNote {
                    node: "LIV-1".into(),
                    text: text.into(),
                    run: run(mark),
                },
                &CLAUDE,
            )
            .unwrap();
        }
    }
    let (_app, cx) = open_app(cx, dir.path());
    cx.update(|_, cx| cx.set_reduce_motion(true));
    resize(cx, 1400., 1400.);
    open_detail(cx);
    redraw(cx);
    let rows: Vec<_> = (0..40)
        .filter_map(|i| cx.debug_bounds(Box::leak(format!("timeline-row-{i}").into_boxed_str())))
        .collect();
    assert!(rows.len() >= 4, "{rows:?}");
    for pair in rows.windows(2) {
        assert!(
            pair[1].top() >= pair[0].bottom(),
            "a row starts below the one above: {pair:?}"
        );
    }
    // The last word is a card, tall enough for its lines.
    assert!(
        rows.iter().any(|r| r.size.height > gpui_kit::px(80.)),
        "the note's card holds its lines"
    );
}
