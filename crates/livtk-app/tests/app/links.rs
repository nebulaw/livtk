//! Linking and priorities from the app, without a window.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::path::Path;

use livtk_app::model::{Ask, Item, Model, View, link_choices};
use livtk_core::{Actor, Command, Kind, Rel, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

fn create(ws: &mut Workspace, kind: Kind, title: &str) -> String {
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
    ws.node(&e[0].op.subjects().0.to_string()).unwrap().key
}

/// Three tasks, two lessons and a finding; the Tasks view, keywords only.
fn app(dir: &Path) -> Model {
    let mut ws = Workspace::init(dir, Some("LIV")).unwrap();
    for t in ["foundation", "walls", "roof"] {
        create(&mut ws, Kind::Task, t);
    }
    create(&mut ws, Kind::Lesson, "measure twice");
    create(&mut ws, Kind::Lesson, "measure twice, in release");
    create(&mut ws, Kind::Finding, "the ground is clay");
    drop(ws);
    let mut m = Model::open(&[dir.to_path_buf()]);
    m.set_models_dir(None);
    m.set_view(View::Tasks);
    m
}

fn id(dir: &Path, key: &str) -> uuid::Uuid {
    Workspace::open(dir).unwrap().node(key).unwrap().id
}

fn edges(dir: &Path) -> Vec<(String, Rel, String)> {
    let ws = Workspace::open(dir).unwrap();
    let key = |u: uuid::Uuid| ws.node(&u.to_string()).unwrap().key;
    ws.edges()
        .unwrap()
        .into_iter()
        .map(|(s, r, d)| (key(s), r, key(d)))
        .collect()
}

fn select(m: &mut Model, dir: &Path, key: &str) {
    let view = if key.starts_with("LES") {
        View::Lessons
    } else if key.starts_with("FND") {
        View::Findings
    } else {
        View::Tasks
    };
    m.set_view(view);
    m.select(id(dir, key));
    m.open_detail();
}

#[test]
fn each_kind_offers_the_links_it_can_have() {
    let labels = |k| -> Vec<&str> { link_choices(k).iter().map(|c| c.label).collect() };
    assert_eq!(
        labels(Kind::Task),
        [
            "Waits for…",
            "Blocks…",
            "Part of…",
            "Add sub-task…",
            "Related to…"
        ]
    );
    assert_eq!(
        labels(Kind::Lesson),
        ["Learned from…", "Supersedes…", "Related to…"]
    );
    assert_eq!(
        labels(Kind::Decision),
        ["Learned from…", "Supersedes…", "Related to…"]
    );
    assert_eq!(labels(Kind::Finding), ["Learned from…", "Related to…"]);
}

#[test]
fn links_are_added_both_ways_shown_in_groups_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut m = app(d);
    select(&mut m, d, "LIV-2");
    let [waits, blocks, _, _, related] = link_choices(Kind::Task)[..] else {
        panic!()
    };
    m.link(id(d, "LIV-2"), waits, id(d, "LIV-1"));
    m.link(id(d, "LIV-2"), blocks, id(d, "LIV-3"));
    m.link(id(d, "LIV-2"), related, id(d, "FND-1"));
    let mut got = edges(d);
    got.sort_by_key(|(s, r, d)| (s.clone(), r.as_str(), d.clone()));
    assert_eq!(
        got,
        [
            ("LIV-2".into(), Rel::DependsOn, "LIV-1".into()),
            ("LIV-2".into(), Rel::Relates, "FND-1".into()),
            ("LIV-3".into(), Rel::DependsOn, "LIV-2".into()),
        ],
        "Blocks… makes the other one wait"
    );
    let groups: Vec<(&str, Vec<&str>)> = m
        .detail
        .as_ref()
        .unwrap()
        .link_groups
        .iter()
        .map(|g| (g.label, g.briefs.iter().map(|b| b.key.as_str()).collect()))
        .collect();
    assert_eq!(
        groups,
        [
            ("Waits for", vec!["LIV-1"]),
            ("Blocks", vec!["LIV-3"]),
            ("Related to", vec!["FND-1"])
        ],
        "the panel shows every link, related ones too"
    );

    m.unlink(Rel::DependsOn, false, "LIV-3");
    m.unlink(Rel::Relates, true, "FND-1");
    assert_eq!(edges(d), [("LIV-2".into(), Rel::DependsOn, "LIV-1".into())]);
    assert_eq!(m.detail.as_ref().unwrap().link_groups.len(), 1);
}

#[test]
fn sub_tasks_are_linked_from_either_end_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut m = app(d);
    let [_, _, part_of, add_sub, related] = link_choices(Kind::Task)[..] else {
        panic!()
    };
    m.link(id(d, "LIV-2"), part_of, id(d, "LIV-1"));
    m.link(id(d, "LIV-1"), add_sub, id(d, "LIV-3"));
    let mut got = edges(d);
    got.sort_by_key(|(s, r, d)| (s.clone(), r.as_str(), d.clone()));
    assert_eq!(
        got,
        [
            ("LIV-2".into(), Rel::Parent, "LIV-1".into()),
            ("LIV-3".into(), Rel::Parent, "LIV-1".into()),
        ],
        "Add sub-task… makes the other one a part"
    );
    select(&mut m, d, "LIV-1");
    let groups = |m: &Model| -> Vec<(&'static str, Vec<String>)> {
        m.detail
            .as_ref()
            .unwrap()
            .link_groups
            .iter()
            .map(|g| (g.label, g.briefs.iter().map(|b| b.key.clone()).collect()))
            .collect()
    };
    assert_eq!(
        groups(&m),
        [("Sub-tasks", vec!["LIV-2".to_owned(), "LIV-3".to_owned()])]
    );
    m.link(id(d, "LIV-1"), part_of, id(d, "LIV-2"));
    assert!(
        m.notice()
            .map(|n| n.text.as_str())
            .is_some_and(|n| n.contains("cycle")),
        "a task cannot be part of its own part: {:?}",
        m.notice()
    );

    m.unlink(Rel::Parent, false, "LIV-3");
    assert_eq!(groups(&m), [("Sub-tasks", vec!["LIV-2".to_owned()])]);
    select(&mut m, d, "LIV-2");
    assert_eq!(groups(&m), [("Part of", vec!["LIV-1".to_owned()])]);

    // Related from the other end shows in the same group and is removed from there.
    m.link(id(d, "LIV-3"), related, id(d, "LIV-2"));
    select(&mut m, d, "LIV-2");
    assert_eq!(
        groups(&m),
        [
            ("Part of", vec!["LIV-1".to_owned()]),
            ("Related to", vec!["LIV-3".to_owned()])
        ]
    );
    m.unlink(Rel::Relates, true, "LIV-3");
    assert_eq!(groups(&m), [("Part of", vec!["LIV-1".to_owned()])]);
}

#[test]
fn the_other_end_sees_the_link_and_a_cycle_is_refused_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut m = app(d);
    let waits = link_choices(Kind::Task)[0];
    m.link(id(d, "LIV-2"), waits, id(d, "LIV-1"));
    select(&mut m, d, "LIV-1");
    let g = &m.detail.as_ref().unwrap().link_groups;
    assert_eq!(
        (g[0].label, g[0].briefs[0].key.as_str()),
        ("Blocks", "LIV-2")
    );

    m.link(id(d, "LIV-1"), waits, id(d, "LIV-2"));
    assert!(
        m.notice()
            .map(|n| n.text.as_str())
            .unwrap_or_default()
            .contains("cycle"),
        "{:?}",
        m.notice()
    );
    assert_eq!(edges(d).len(), 1);
}

#[test]
fn the_palette_picks_a_links_other_end_from_what_can_join() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut m = app(d);
    select(&mut m, d, "LIV-2");
    m.open_palette();
    m.palette_query("add link: waits");
    assert_eq!(
        m.palette_run(),
        Some(Ask::PickLink),
        "the view keeps the palette open"
    );
    let p = m.palette.as_ref().expect("still open, now picking");
    assert_eq!(p.link.as_ref().unwrap().key, "LIV-2");
    let keys = |m: &Model| -> Vec<String> {
        m.palette
            .as_ref()
            .unwrap()
            .items
            .iter()
            .map(|i| match i {
                Item::Node(n) => n.key.clone(),
                Item::Action(a) => a.label.clone(),
            })
            .collect()
    };
    let mut shown = keys(&m);
    shown.sort();
    assert_eq!(
        shown,
        ["LIV-1", "LIV-3"],
        "tasks only, never itself, no actions"
    );
    m.palette_query("found");
    assert_eq!(keys(&m), ["LIV-1"]);
    m.palette_run();
    assert!(m.palette.is_none());
    assert_eq!(edges(d), [("LIV-2".into(), Rel::DependsOn, "LIV-1".into())]);

    select(&mut m, d, "LES-2");
    m.open_link_picker(link_choices(Kind::Lesson)[1]);
    assert_eq!(
        keys(&m),
        ["LES-1"],
        "a lesson supersedes only another lesson"
    );
    m.open_link_picker(link_choices(Kind::Lesson)[0]);
    assert_eq!(keys(&m).len(), 5, "learned from anything but itself");
}

#[test]
fn a_priority_is_set_on_any_task_not_only_the_selected_one() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut m = app(d);
    m.select(id(d, "LIV-1"));
    m.set_priority_of(id(d, "LIV-3"), 1);
    let ws = Workspace::open(d).unwrap();
    assert_eq!(ws.node("LIV-3").unwrap().priority, 1);
    assert_eq!(
        ws.node("LIV-1").unwrap().priority,
        3,
        "the selection is untouched"
    );
    assert_eq!(m.selected().unwrap().key, "LIV-1");
}
