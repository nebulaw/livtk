//! The command palette without a window: what it offers, and what choosing does.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::path::Path;

use livtk_app::model::{Ask, Grouping, Item, Model, View, fuzzy};
use livtk_core::{Actor, Command, Kind, State, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

fn create(ws: &mut Workspace, kind: Kind, title: &str, to: Option<State>) -> String {
    let e = ws
        .execute(
            Command::Create {
                kind,
                title: title.into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &CLAUDE,
        )
        .unwrap();
    let key = ws.node(&e[0].op.subjects().0.to_string()).unwrap().key;
    if let Some(to) = to {
        ws.execute(
            Command::Move {
                node: key.clone(),
                to,
            },
            &CLAUDE,
        )
        .unwrap();
    }
    key
}

/// Two plans in the inbox, a task in the backlog, a canceled one and a lesson; keywords only.
fn app(dir: &Path) -> Model {
    let mut ws = Workspace::init(dir, Some("LIV")).unwrap();
    create(
        &mut ws,
        Kind::Task,
        "Resize the window without flicker",
        Some(State::Planned),
    );
    create(&mut ws, Kind::Task, "Second plan", Some(State::Planned));
    create(&mut ws, Kind::Task, "Handle two monitors", None);
    create(
        &mut ws,
        Kind::Task,
        "Old idea nobody wants",
        Some(State::Canceled),
    );
    create(
        &mut ws,
        Kind::Lesson,
        "Run cargo check before reporting done",
        None,
    );
    drop(ws);
    let mut m = Model::open(&[dir.to_path_buf()]);
    m.set_models_dir(None);
    m
}

fn labels(m: &Model) -> Vec<String> {
    m.palette
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| match i {
            Item::Action(a) => a.label.clone(),
            Item::Node(n) => n.key.clone(),
            Item::Doc(d) => d.path.clone(),
        })
        .collect()
}

fn state(dir: &Path, key: &str) -> State {
    Workspace::open(dir).unwrap().node(key).unwrap().state
}

#[test]
fn fuzzy_prefers_word_starts_and_runs() {
    assert!(fuzzy("ack", "Acknowledge") > fuzzy("ack", "Back to backlog"));
    assert!(fuzzy("gtl", "Go to Lessons").is_some());
    assert!(fuzzy("go les", "Go to Lessons") > fuzzy("go les", "Order by last updated"));
    assert_eq!(fuzzy("zz", "Acknowledge"), None);
    assert_eq!(fuzzy("", "anything"), Some(0));
}

#[test]
fn empty_it_offers_what_the_selected_node_can_do_then_recent_nodes() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.open_palette();
    let shown = labels(&m);
    assert_eq!(
        shown[0], "Acknowledge",
        "the selected plan's decision comes first"
    );
    let p = m.palette.as_ref().unwrap();
    match &p.items[0] {
        Item::Action(a) => {
            assert_eq!(a.context.as_deref(), Some("LIV-1"));
            assert_eq!(a.shortcut, Some("A"));
        }
        other => panic!("{other:?}"),
    }
    assert!(shown.contains(&"Set priority: Urgent".to_owned()));
    assert!(
        shown.contains(&"LES-1".to_owned()) && shown.contains(&"LIV-3".to_owned()),
        "recent nodes follow: {shown:?}"
    );
    assert!(
        !shown.contains(&"LIV-4".to_owned()),
        "dismissed work is not recent work"
    );
}

#[test]
fn typing_matches_actions_by_letters_and_nodes_by_words_and_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.open_palette();
    m.palette_query("ack");
    assert_eq!(labels(&m)[0], "Acknowledge");

    m.palette_query("monit");
    assert_eq!(labels(&m), ["LIV-3"], "the word being typed is a prefix");

    m.palette_query("LIV-2");
    assert_eq!(labels(&m).first().map(String::as_str), Some("LIV-2"));
    m.palette_query("2");
    assert_eq!(labels(&m).first().map(String::as_str), Some("LIV-2"));

    m.palette_query("go les");
    assert_eq!(labels(&m)[0], "Go to Lessons");

    m.palette_query("another project");
    assert_eq!(labels(&m)[0], "Open another project…");
    assert_eq!(
        m.palette_run(),
        Some(Ask::OpenFolder),
        "the view asks for the folder"
    );
}

#[test]
fn enter_runs_the_highlighted_action_and_closes() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.open_palette();
    m.palette_query("ack");
    assert_eq!(m.palette_run(), None, "the model did it itself");
    assert!(m.palette.is_none());
    assert_eq!(state(dir.path(), "LIV-1"), State::Acked);

    m.open_palette();
    m.palette_query("group by prio");
    m.palette_run();
    assert_eq!(m.display().grouping, Grouping::Priority);
}

#[test]
fn the_highlight_moves_and_wraps_and_the_pointer_sets_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.open_palette();
    let n = m.palette.as_ref().unwrap().items.len();
    m.palette_move(-1);
    assert_eq!(
        m.palette.as_ref().unwrap().selected,
        n - 1,
        "up from the top wraps"
    );
    m.palette_move(1);
    assert_eq!(m.palette.as_ref().unwrap().selected, 0);
    m.palette_highlight(2);
    assert_eq!(m.palette.as_ref().unwrap().selected, 2);
    m.palette_query("set priority: low");
    assert_eq!(
        m.palette.as_ref().unwrap().selected,
        0,
        "a new query starts at the top"
    );
}

#[test]
fn choosing_a_node_goes_to_it_wherever_it_hides() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());

    m.set_view(View::Tasks);
    m.open_palette();
    m.palette_query("cargo");
    m.palette_run();
    assert_eq!(m.view(), View::Lessons);
    assert_eq!(m.selected().map(|r| r.key.as_str()), Some("LES-1"));
    assert_eq!(
        m.detail.as_ref().map(|d| d.node.key.as_str()),
        Some("LES-1")
    );

    // Canceled, behind a filter that excludes it: the palette still gets there.
    m.set_view(View::Tasks);
    m.set_filter("resize");
    m.open_palette();
    m.palette_query("nobody");
    m.palette_run();
    assert!(
        m.display().show_dismissed,
        "dismissed work is shown in its view to show it"
    );
    assert_eq!(m.display().filter, "", "the filter that hid it is cleared");
    assert_eq!(m.selected().map(|r| r.key.as_str()), Some("LIV-4"));
    assert!(m.detail.is_some());
}

#[test]
fn a_node_waiting_in_the_inbox_opens_there() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.open_palette();
    m.palette_query("second");
    m.palette_run();
    assert_eq!(m.view(), View::Inbox, "it is already in view");
    assert_eq!(m.selected().map(|r| r.key.as_str()), Some("LIV-2"));
}

#[test]
fn what_only_the_view_can_do_comes_back_to_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    for (query, ask) in [
        ("new task", Ask::NewTask),
        ("add a note", Ask::AddNote),
        ("edit title", Ask::EditTitle),
        ("keyboard shortcuts", Ask::Help),
    ] {
        m.open_palette();
        m.palette_query(query);
        assert_eq!(m.palette_run(), Some(ask), "{query}");
        assert!(m.palette.is_none());
    }
}

#[test]
fn the_palette_turns_empty_groups_on_and_off_in_the_lists_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.open_palette();
    m.palette_query("empty groups");
    assert!(
        !labels(&m).iter().any(|l| l.contains("empty groups")),
        "the inbox has no groups of one kind: {:?}",
        labels(&m)
    );

    m.set_view(View::Tasks);
    m.open_palette();
    m.palette_query("empty groups");
    assert_eq!(
        labels(&m).first().map(String::as_str),
        Some("Show empty groups")
    );
    let shortcut = match &m.palette.as_ref().unwrap().items[0] {
        Item::Action(a) => a.shortcut,
        Item::Node(_) | Item::Doc(_) => None,
    };
    assert_eq!(
        shortcut,
        Some(livtk_app::model::keys::EMPTY_GROUPS),
        "its key is shown, as the platform names it"
    );
    m.palette_run();
    assert!(m.display().show_empty);
    m.open_palette();
    m.palette_query("empty groups");
    assert_eq!(
        labels(&m).first().map(String::as_str),
        Some("Hide empty groups")
    );

    m.set_view(View::Graph);
    m.open_palette();
    m.palette_query("dismissed");
    assert_eq!(
        labels(&m).first().map(String::as_str),
        Some("Show dismissed work"),
        "the graph keeps its dismissed work"
    );
    m.palette_query("empty groups");
    assert!(!labels(&m).iter().any(|l| l.contains("empty groups")));
}

#[test]
fn keys_are_named_as_the_platform_names_them() {
    use livtk_app::model::keys;
    if cfg!(target_os = "macos") {
        assert_eq!(
            (keys::MOD, keys::PALETTE, keys::STANDING),
            ("⌘", "⌘ K", "⇧ A")
        );
    } else {
        assert_eq!(
            (keys::MOD, keys::PALETTE, keys::STANDING),
            ("Ctrl", "Ctrl K", "Shift A")
        );
    }
}

#[test]
fn choosing_a_node_clears_a_state_filter_that_hides_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.set_view(View::Tasks);
    m.toggle_state(State::Planned);
    m.open_palette();
    m.palette_query("monitors");
    m.palette_run();
    assert!(
        m.display().states.is_empty(),
        "the state filter that hid it is cleared"
    );
    assert_eq!(m.selected().map(|r| r.key.as_str()), Some("LIV-3"));
}

/// Every view is offered as "Go to …" with the keys the view says it has: one table, no list
/// kept by hand.
#[test]
fn every_view_is_offered_with_its_own_keys() {
    use livtk_app::model::View;
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.open_palette();
    for v in View::ALL {
        if v == m.view() {
            continue;
        }
        let label = format!("Go to {}", v.title());
        m.palette_query(&label);
        let first = m
            .palette
            .as_ref()
            .unwrap()
            .items
            .first()
            .and_then(|i| match i {
                Item::Action(a) => Some((a.label.clone(), a.shortcut)),
                _ => None,
            });
        assert_eq!(first, Some((label, v.go_keys())));
    }
}
