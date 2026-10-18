//! The project's docs: the tree, their names, paths and links within them; and nodes linked to
//! them, many to many, through the log.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::fs;
use std::path::Path;

use livtk_core::docs::{self, Target};
use livtk_core::event::Op;
use livtk_core::{Actor, Command, Kind, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

fn workspace() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    ws.set_models_dir(None);
    let root = dir.path();
    write(root, "docs/README.md", "# Getting started\n");
    write(root, "docs/zeta.md", "no heading here\n");
    write(
        root,
        "docs/Alpha.md",
        "```\n# not a title\n```\n## Alpha, at last ##\n",
    );
    write(root, "docs/guide/setup.md", "# Setup\n");
    write(root, "docs/.hidden/secret.md", "# Secret\n");
    write(root, "docs/.draft.md", "# Draft\n");
    write(root, "docs/empty/notes.txt", "not a doc\n");
    (dir, ws)
}

fn task(ws: &mut Workspace, title: &str) -> String {
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
    ws.node(&e[0].op.subjects().0.to_string()).unwrap().key
}

fn link(ws: &mut Workspace, key: &str, path: &str) -> usize {
    ws.execute(
        Command::DocLink {
            node: key.into(),
            path: path.into(),
        },
        &CLAUDE,
    )
    .unwrap()
    .len()
}

#[test]
fn the_tree_puts_folders_first_readme_first_and_names_docs_by_their_heading() {
    let (_dir, ws) = workspace();
    let tree = ws.docs();
    assert_eq!(tree.path, "docs");
    assert_eq!(
        tree.folders.len(),
        1,
        "hidden and doc-less folders left out"
    );
    assert_eq!(tree.folders[0].name, "guide");
    let names: Vec<_> = tree
        .docs
        .iter()
        .map(|d| (d.path.as_str(), d.title.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("docs/README.md", "Getting started"),
            ("docs/Alpha.md", "Alpha, at last"),
            ("docs/zeta.md", "zeta"),
        ]
    );
    let all: Vec<_> = tree.all().iter().map(|d| d.path.clone()).collect();
    assert_eq!(all[0], "docs/guide/setup.md", "a folder's docs come first");
}

#[test]
fn the_config_can_name_another_folder() {
    let (dir, _ws) = workspace();
    write(dir.path(), "handbook/intro.md", "# Intro\n");
    let config = dir.path().join(".livtk/config.toml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(&config, format!("{text}docs = \"handbook\"\n")).unwrap();
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.docs_dir(), "handbook");
    assert_eq!(ws.docs().all()[0].title, "Intro");
}

#[test]
fn paths_and_links_resolve_within_the_project() {
    let root = Path::new("/p");
    assert_eq!(
        docs::normalize(root, "./docs/a.md").as_deref(),
        Some("docs/a.md")
    );
    assert_eq!(
        docs::normalize(root, "docs/x/../a.md").as_deref(),
        Some("docs/a.md")
    );
    assert_eq!(
        docs::normalize(root, "../out.md"),
        None,
        "outside the project"
    );
    let doc = |p: &str, a: Option<&str>| Target::Doc {
        path: p.into(),
        anchor: a.map(Into::into),
    };
    let from = "docs/guide/setup.md";
    assert_eq!(
        docs::resolve(from, "install.md"),
        Some(doc("docs/guide/install.md", None))
    );
    assert_eq!(
        docs::resolve(from, "../README.md#start"),
        Some(doc("docs/README.md", Some("start")))
    );
    assert_eq!(
        docs::resolve(from, "/docs/zeta.md"),
        Some(doc("docs/zeta.md", None))
    );
    assert_eq!(
        docs::resolve(from, "#usage"),
        Some(Target::Anchor("usage".into()))
    );
    assert_eq!(
        docs::resolve(from, "https://example.com/x"),
        Some(Target::Web("https://example.com/x".into()))
    );
    assert_eq!(docs::resolve(from, "../../../x.md"), None);
}

#[test]
fn nodes_and_docs_link_many_to_many_through_the_log() {
    let (_dir, mut ws) = workspace();
    let (a, b) = (task(&mut ws, "first"), task(&mut ws, "second"));
    assert_eq!(link(&mut ws, &a, "docs/README.md"), 1);
    assert_eq!(link(&mut ws, &a, "./docs/guide/setup.md"), 1);
    assert_eq!(link(&mut ws, &b, "docs/README.md"), 1);
    assert_eq!(
        link(&mut ws, &a, "docs/README.md"),
        0,
        "linked already: nothing new"
    );
    let docs_of_a: Vec<_> = ws
        .docs_of(&a)
        .unwrap()
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert_eq!(docs_of_a, ["docs/README.md", "docs/guide/setup.md"]);
    let nodes: Vec<_> = ws
        .doc_nodes("docs/README.md")
        .unwrap()
        .into_iter()
        .map(|n| n.key)
        .collect();
    assert_eq!(nodes, [a.clone(), b.clone()]);
    // Each in the node's history.
    let history = ws.history(Some(&a)).unwrap();
    assert!(
        history.iter().any(
            |e| matches!(&e.op, Op::DocLink { data, .. } if data.path == "docs/guide/setup.md")
        )
    );
    ws.execute(
        Command::DocUnlink {
            node: a.clone(),
            path: "docs/README.md".into(),
        },
        &CLAUDE,
    )
    .unwrap();
    assert_eq!(ws.doc_nodes("docs/README.md").unwrap().len(), 1);
    assert!(
        ws.execute(
            Command::DocUnlink {
                node: a.clone(),
                path: "docs/README.md".into(),
            },
            &CLAUDE,
        )
        .is_err(),
        "not linked any more"
    );
}

#[test]
fn a_doc_must_exist_to_be_linked_and_its_links_stay_once_it_is_gone() {
    let (dir, mut ws) = workspace();
    let a = task(&mut ws, "first");
    for bad in ["docs/nope.md", "../elsewhere.md"] {
        assert!(
            ws.execute(
                Command::DocLink {
                    node: a.clone(),
                    path: bad.into(),
                },
                &CLAUDE,
            )
            .is_err(),
            "{bad}"
        );
    }
    link(&mut ws, &a, "docs/zeta.md");
    fs::remove_file(dir.path().join("docs/zeta.md")).unwrap();
    assert_eq!(
        ws.docs_of(&a).unwrap(),
        [("docs/zeta.md".to_owned(), None)],
        "missing"
    );
    ws.execute(
        Command::DocUnlink {
            node: a,
            path: "docs/zeta.md".into(),
        },
        &CLAUDE,
    )
    .unwrap();
}

#[test]
fn links_come_back_when_the_store_is_rebuilt_from_the_log() {
    let (dir, mut ws) = workspace();
    let a = task(&mut ws, "first");
    link(&mut ws, &a, "docs/README.md");
    drop(ws);
    fs::remove_file(dir.path().join(".livtk/livtk.db")).unwrap();
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.docs_of(&a).unwrap().len(), 1);
}

#[test]
fn a_log_rewritten_without_a_link_takes_it_away() {
    let (dir, mut ws) = workspace();
    let a = task(&mut ws, "first");
    let log = dir.path().join(".livtk/events.jsonl");
    let before = fs::read_to_string(&log).unwrap();
    link(&mut ws, &a, "docs/README.md");
    drop(ws);
    // As a checkout puts the log back as it was.
    fs::write(&log, before).unwrap();
    let mut ws = Workspace::open(dir.path()).unwrap();
    assert!(ws.docs_of(&a).unwrap().is_empty(), "gone with its event");
    assert_eq!(
        link(&mut ws, &a, "docs/README.md"),
        1,
        "and can be made again"
    );
}

#[test]
fn titles_keep_a_hash_of_their_own_and_links_are_decoded() {
    assert_eq!(docs::title_of("# Using C#\n").as_deref(), Some("Using C#"));
    assert_eq!(docs::title_of("# Closed ##\n").as_deref(), Some("Closed"));
    assert_eq!(
        docs::resolve("docs/a.md", "my%20doc.md?x=1#top"),
        Some(Target::Doc {
            path: "docs/my doc.md".into(),
            anchor: Some("top".into())
        })
    );
}

#[test]
fn an_absolute_path_through_a_linked_folder_is_the_projects() {
    let (dir, _ws) = workspace();
    let real = dir.path().canonicalize().unwrap();
    // The project as reached through a link to it, and a doc that is not there (any more).
    let link_dir = tempfile::tempdir().unwrap();
    let via = link_dir.path().join("project");
    std::os::unix::fs::symlink(&real, &via).unwrap();
    let gone = via.join("docs/gone.md");
    assert_eq!(
        docs::normalize(dir.path(), gone.to_str().unwrap()).as_deref(),
        Some("docs/gone.md")
    );
}

#[test]
fn links_in_a_doc_are_read_and_the_docs_linking_to_one_found() {
    let text = "See [setup](guide/setup.md \"Setup\") and <https://x.y>.\n`[not](a.md)`\n```\n[no](b.md)\n```\n[ref]: ../c.md\n";
    assert_eq!(
        docs::links_in(text),
        ["guide/setup.md", "https://x.y", "../c.md"].map(String::from)
    );
    let (dir, ws) = workspace();
    write(
        dir.path(),
        "docs/README.md",
        "# Start\nSee [setup](guide/setup.md).\n",
    );
    write(
        dir.path(),
        "docs/zeta.md",
        "Back to [setup](/docs/guide/setup.md#top).\n",
    );
    let tree = ws.docs();
    let all = tree.all();
    let linking: Vec<_> = docs::linking_to(dir.path(), &all, "docs/guide/setup.md")
        .into_iter()
        .map(|d| d.path.clone())
        .collect();
    assert_eq!(linking, ["docs/README.md", "docs/zeta.md"]);
}

#[test]
fn a_search_finds_docs_by_title_first_then_by_text() {
    let (dir, ws) = workspace();
    write(
        dir.path(),
        "docs/zeta.md",
        "Nothing.\nHow to set up the build.\n",
    );
    let tree = ws.docs();
    let found = docs::search(dir.path(), &tree.all(), "SETUP");
    let paths: Vec<_> = found.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["docs/guide/setup.md"], "the title, case aside");
    let found = docs::search(dir.path(), &tree.all(), "set up");
    assert_eq!(found[0].path, "docs/zeta.md");
    assert_eq!(
        found[0].lines,
        ["How to set up the build."],
        "the line it is in"
    );
    assert!(docs::search(dir.path(), &tree.all(), "  ").is_empty());
}

#[test]
fn nodes_mentioning_a_doc_are_found_but_not_those_linked() {
    let (_dir, mut ws) = workspace();
    let a = task(&mut ws, "Rewrite docs/guide/setup.md");
    let b = task(&mut ws, "second");
    ws.execute(
        Command::Note {
            node: b.clone(),
            text: "see docs/guide/setup.md for 100% of it".into(),
        },
        &CLAUDE,
    )
    .unwrap();
    let c = task(&mut ws, "third");
    link(&mut ws, &c, "docs/guide/setup.md");
    let mut keys: Vec<_> = ws
        .doc_mentions("docs/guide/setup.md")
        .unwrap()
        .into_iter()
        .map(|n| n.key)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [a, b],
        "by title and by note; the linked one is its link"
    );
    assert!(
        ws.doc_mentions("docs/guide/set%p.md").unwrap().is_empty(),
        "% is a character"
    );
}
