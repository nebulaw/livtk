//! The Docs view without a window: the tree and its keys, the doc open and remembered, links
//! between docs and nodes from either end and through the palette, keys in docs made links,
//! and docs read again when they change on disk.

use std::fs;
use std::path::Path;

use livtk_app::model::docs::{
    CalloutKind, DocRow, callout, find_in, front, link_keys, local_images, page, slug,
};
use livtk_app::model::{DocPick, Item, Model, Saved, View};
use livtk_core::{Actor, Command, Kind, Workspace};

const CLAUDE: Actor = Actor::Claude { agent: None };

fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

/// A project with two tasks and docs: a README, a guide folder with two docs.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::init(dir.path(), Some("LIV")).unwrap();
    for t in ["Write the guide", "Ship it"] {
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
    write(
        dir.path(),
        "docs/README.md",
        "# Start here\nSee LIV-1 and `LIV-2`.\n",
    );
    write(dir.path(), "docs/guide/setup.md", "# Setup\n");
    write(dir.path(), "docs/guide/usage.md", "# Usage\n");
    dir
}

fn open(dir: &Path) -> Model {
    let mut m = Model::open(&[dir.to_path_buf()]);
    m.set_models_dir(None);
    m.set_view(View::Docs);
    m
}

fn paths(m: &Model) -> Vec<String> {
    m.docs.rows().iter().map(|r| r.path().to_owned()).collect()
}

#[test]
fn the_tree_opens_the_first_doc_and_the_keys_walk_it() {
    let dir = project();
    let mut m = open(dir.path());
    assert_eq!(
        paths(&m),
        [
            "docs/guide",
            "docs/guide/setup.md",
            "docs/guide/usage.md",
            "docs/README.md"
        ]
    );
    assert_eq!(
        m.docs.open.as_deref(),
        Some("docs/guide/setup.md"),
        "the first doc"
    );
    assert_eq!(m.docs.text.as_deref(), Some("# Setup\n"));
    // Enter on the folder shuts it; J then Enter opens the README.
    m.enter();
    assert!(matches!(
        m.docs.rows()[0],
        DocRow::Folder { open: false, .. }
    ));
    assert_eq!(paths(&m), ["docs/guide", "docs/README.md"]);
    m.select_by(1);
    m.enter();
    assert_eq!(m.docs.open.as_deref(), Some("docs/README.md"));
    // Opening a doc in a shut folder opens the folder, so the tree shows it.
    m.open_doc("docs/guide/usage.md");
    assert_eq!(paths(&m).len(), 4);
    assert_eq!(m.docs.rows()[m.docs.index].path(), "docs/guide/usage.md");
}

#[test]
fn the_doc_open_is_remembered_per_project() {
    let dir = project();
    let mut m = open(dir.path());
    m.open_doc("docs/README.md");
    let file = tempfile::tempdir().unwrap();
    let path = file.path().join("livtk/app.toml");
    m.saved().save(&path).unwrap();
    let mut back = Model::open(&[dir.path().to_path_buf()]);
    back.set_models_dir(None);
    back.restore(&Saved::load(&path));
    back.set_view(View::Docs);
    assert_eq!(back.docs.open.as_deref(), Some("docs/README.md"));
}

#[test]
fn docs_and_nodes_link_from_either_end() {
    let dir = project();
    let mut m = open(dir.path());
    m.open_doc("docs/README.md");
    m.link_doc("LIV-1", "docs/README.md", true);
    m.link_doc("LIV-2", "docs/README.md", true);
    let linked: Vec<_> = m.docs.linked.iter().map(|n| n.key.clone()).collect();
    assert_eq!(linked, ["LIV-1", "LIV-2"], "the doc's nodes");
    m.link_doc("LIV-1", "docs/guide/setup.md", true);
    m.show_node("LIV-1");
    let docs: Vec<_> = m
        .detail
        .as_ref()
        .unwrap()
        .docs
        .iter()
        .map(|(p, _)| p.clone())
        .collect();
    assert_eq!(
        docs,
        ["docs/README.md", "docs/guide/setup.md"],
        "the node's docs"
    );
    m.link_doc("LIV-2", "docs/README.md", false);
    assert_eq!(m.docs.linked.len(), 1);
}

#[test]
fn the_palette_links_a_doc_to_a_node_and_a_node_to_a_doc() {
    let dir = project();
    let mut m = open(dir.path());
    m.open_doc("docs/README.md");
    // From the doc: pick a node.
    m.open_doc_node_picker();
    assert_eq!(
        m.palette.as_ref().unwrap().doc_pick,
        Some(DocPick::ForDoc("docs/README.md".into()))
    );
    m.palette_query("ship");
    assert!(matches!(&m.palette.as_ref().unwrap().items[0], Item::Node(n) if n.key == "LIV-2"));
    m.palette_run();
    assert_eq!(m.docs.linked.len(), 1);
    // From the node: pick a doc; those linked already are not offered.
    m.show_node("LIV-2");
    m.open_doc_picker();
    m.palette_query("");
    let offered: Vec<_> = m
        .palette
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Doc(d) => Some(d.path.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !offered.contains(&"docs/README.md".to_owned()),
        "{offered:?}"
    );
    m.palette_query("usage");
    m.palette_run();
    let docs: Vec<_> = m
        .detail
        .as_ref()
        .unwrap()
        .docs
        .iter()
        .map(|(p, _)| p.clone())
        .collect();
    assert!(docs.contains(&"docs/guide/usage.md".to_owned()), "{docs:?}");
}

#[test]
fn the_palette_finds_docs_and_opens_them() {
    let dir = project();
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_models_dir(None);
    m.open_palette();
    m.palette_query("setup");
    let at = m
        .palette
        .as_ref()
        .unwrap()
        .items
        .iter()
        .position(|i| matches!(i, Item::Doc(d) if d.path == "docs/guide/setup.md"))
        .expect("the doc is found");
    m.palette_run_at(at);
    assert_eq!(m.view, View::Docs);
    assert_eq!(m.docs.open.as_deref(), Some("docs/guide/setup.md"));
}

#[test]
fn keys_in_a_doc_become_links_but_not_in_code() {
    assert_eq!(
        link_keys("See LIV-1 and `LIV-2`.\n", "LIV"),
        "See [LIV-1](livtk:LIV-1) and `LIV-2`.\n"
    );
    assert_eq!(link_keys("```\nLIV-3\n```\n", "LIV"), "```\nLIV-3\n```\n");
    assert_eq!(
        link_keys("[LIV-4](x) xLIV-5 LIV-6a", "LIV"),
        "[LIV-4](x) xLIV-5 LIV-6a"
    );
    assert_eq!(link_keys("café LIV-7", "LIV"), "café [LIV-7](livtk:LIV-7)");
    // Where a link would break the markdown or mean nothing, keys stay.
    for kept in [
        "[see LIV-8 notes](url)\n",
        "[x](https://jira/browse?id=LIV-9)\n",
        "<https://x/LIV-10>\n",
        "https://jira/browse/LIV-11 and more\n",
        "    LIV-12 in indented code\n",
        "[ref]: https://x/LIV-13\n",
    ] {
        assert_eq!(link_keys(kept, "LIV"), kept);
    }
    assert_eq!(
        link_keys("[a](b) then LIV-14\n", "LIV"),
        "[a](b) then [LIV-14](livtk:LIV-14)\n",
        "after a link, keys link again"
    );
}

/// Whatever the text, linking keys never breaks it: every character kept, in order.
#[test]
fn linking_keys_keeps_any_text_whole() {
    let pieces = [
        "LIV-1", "é", " ", "[", "]", "(", ")", "<", ">", "`", "\n", "https://", "日本", "LIV-",
        "🦀", "\t", "#", "LIV-22x", "www.",
    ];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                let text = format!("{a}{b}{c}");
                let out = link_keys(&text, "LIV");
                // Taking the links away gives the text back.
                let mut back = out.clone();
                while let Some(i) = back.find("](livtk:") {
                    let start = back[..i].rfind('[').unwrap();
                    let end = back[i..].find(')').unwrap() + i;
                    let key = back[start + 1..i].to_owned();
                    back.replace_range(start..=end, &key);
                }
                assert_eq!(back, text, "{out:?}");
            }
        }
    }
}

#[test]
fn a_remembered_doc_gone_from_disk_gives_way_to_the_first() {
    let dir = project();
    let mut m = open(dir.path());
    m.open_doc("docs/README.md");
    fs::remove_file(dir.path().join("docs/README.md")).unwrap();
    m.set_view(View::Tasks);
    m.set_view(View::Docs);
    assert_eq!(m.docs.open.as_deref(), Some("docs/guide/setup.md"));
}

#[test]
fn docs_are_read_again_when_they_change_on_disk() {
    let dir = project();
    let mut m = open(dir.path());
    assert!(!m.refresh_docs(), "nothing changed");
    write(dir.path(), "docs/new.md", "# New\n");
    assert!(m.refresh_docs());
    assert!(paths(&m).contains(&"docs/new.md".to_owned()));
}

#[test]
fn a_doc_is_laid_out_as_its_title_and_a_section_at_each_heading() {
    let text = "# Guide\n\nIntro [ref].\n\n## Install it\n\nSteps.\n\n```\n## not a heading\n```\n\n### Then\n\n## Install it\n\n#### Deep stays in\n\n[ref]: https://example.com\n";
    let p = page(text);
    assert_eq!(p.title.as_deref(), Some("Guide"));
    let heads: Vec<_> = p
        .sections
        .iter()
        .map(|s| s.heading.as_ref().map(|h| (h.level, h.slug.clone())))
        .collect();
    assert_eq!(
        heads,
        [
            None,
            Some((2, "install-it".into())),
            Some((3, "then".into())),
            Some((2, "install-it-1".into())),
        ],
        "the intro, then each heading to level three; the same twice numbered"
    );
    assert!(
        p.sections[1].text.contains("## not a heading"),
        "fences are text"
    );
    assert!(p.sections[3].text.contains("#### Deep stays in"));
    assert!(
        p.sections
            .iter()
            .all(|s| s.text.contains("[ref]: https://example.com")),
        "every section knows the doc's references"
    );
    assert_eq!(p.section_of("then"), Some(2));
    assert_eq!(
        page("Text first\n# Not the title\n").title,
        None,
        "only a heading on top"
    );
    assert_eq!(slug("What's new, in 2.0?"), "whats-new-in-20");
}

#[test]
fn finding_in_the_text_is_case_blind() {
    assert_eq!(find_in("Setup the setup", "SETUP"), [0..5, 10..15]);
    assert!(find_in("anything", "  ").is_empty());
}

#[test]
fn back_and_forward_walk_the_docs_opened_and_neighbours_follow_the_tree() {
    let dir = project();
    let mut m = open(dir.path());
    assert_eq!(m.docs.open.as_deref(), Some("docs/guide/setup.md"));
    m.open_doc("docs/guide/usage.md");
    m.open_doc("docs/README.md#start-here");
    assert_eq!(
        m.docs.anchor.as_deref(),
        Some("start-here"),
        "a heading to land on"
    );
    m.doc_back();
    assert_eq!(m.docs.open.as_deref(), Some("docs/guide/usage.md"));
    m.doc_back();
    assert_eq!(m.docs.open.as_deref(), Some("docs/guide/setup.md"));
    m.doc_forward();
    assert_eq!(m.docs.open.as_deref(), Some("docs/guide/usage.md"));
    let (prev, next) = m.doc_neighbours();
    assert_eq!(prev.map(|d| d.path.as_str()), Some("docs/guide/setup.md"));
    assert_eq!(next.map(|d| d.path.as_str()), Some("docs/README.md"));
    m.open_doc("docs/README.md");
    assert!(
        m.docs.forward.is_empty(),
        "a new way forgets the way forward"
    );
}

#[test]
fn the_open_doc_knows_what_links_to_it_and_what_mentions_it() {
    let dir = project();
    write(
        dir.path(),
        "docs/guide/usage.md",
        "# Usage\nFirst [set it up](setup.md).\n",
    );
    let mut ws = Workspace::open(dir.path()).unwrap();
    ws.execute(
        Command::Note {
            node: "LIV-2".into(),
            text: "docs/guide/setup.md is out of date".into(),
        },
        &CLAUDE,
    )
    .unwrap();
    drop(ws);
    let mut m = open(dir.path());
    m.open_doc("docs/guide/setup.md");
    let linking: Vec<_> = m.docs.backlinks.iter().map(|d| d.path.clone()).collect();
    assert_eq!(linking, ["docs/guide/usage.md"]);
    let mentioning: Vec<_> = m.docs.mentions.iter().map(|n| n.key.clone()).collect();
    assert_eq!(mentioning, ["LIV-2"]);
}

#[test]
fn searching_the_docs_finds_by_title_and_text_and_the_palette_shows_the_line() {
    let dir = project();
    write(
        dir.path(),
        "docs/guide/usage.md",
        "# Usage\nRun the deploy script.\n",
    );
    let mut m = open(dir.path());
    m.search_docs("deploy");
    let found: Vec<_> = m.docs.results.iter().map(|f| f.path.clone()).collect();
    assert_eq!(found, ["docs/guide/usage.md"]);
    m.search_docs("");
    assert!(m.docs.results.is_empty(), "the tree again");
    m.open_palette();
    m.palette_query("deploy script");
    let line = m
        .palette
        .as_ref()
        .unwrap()
        .items
        .iter()
        .find_map(|i| match i {
            Item::Doc(d) => d.line.clone(),
            _ => None,
        });
    assert_eq!(line.as_deref(), Some("Run the deploy script."));
}

#[test]
fn frontmatter_gives_a_title_a_status_and_tags() {
    let (f, rest) = front(
        "---\ntitle: \"The guide\"\nstatus: draft\ntags: [setup, 'ci']\nother: x\n---\n# Body\n",
    );
    assert_eq!(f.title.as_deref(), Some("The guide"));
    assert_eq!(f.status.as_deref(), Some("draft"));
    assert_eq!(f.tags, ["setup", "ci"]);
    assert_eq!(rest, "# Body\n");
    let (f, _) = front("---\ntags:\n  - one\n  - two\n---\n");
    assert_eq!(f.tags, ["one", "two"], "a list, an item a line");
    let (f, rest) = front("# No frontmatter\n---\n");
    assert_eq!((f.title, rest), (None, "# No frontmatter\n---\n"));
}

#[test]
fn local_images_load_from_the_docs_folder() {
    let dir = Path::new("/p/docs/guide");
    assert_eq!(
        local_images(
            "![a](img/a.png) ![w](https://x/w.png \"t\")\n```\n![c](c.png)\n```\n",
            dir
        ),
        "![a](/p/docs/guide/img/a.png) ![w](https://x/w.png \"t\")\n```\n![c](c.png)\n```\n"
    );
}

#[test]
fn callouts_are_read_from_quotes() {
    assert_eq!(
        callout("> [!WARNING] Mind the gap\n> Step back.\n> Then go."),
        Some((
            CalloutKind::Warning,
            Some("Mind the gap".into()),
            "Step back.\nThen go.".into()
        ))
    );
    assert_eq!(
        callout("> [!note]\n> Lower case too."),
        Some((CalloutKind::Note, None, "Lower case too.".into()))
    );
    assert_eq!(callout("> A plain quote."), None);
    assert_eq!(callout("> [!SHRUG] Unknown."), None);
}

#[test]
fn a_mermaid_diagram_draws_and_a_broken_one_says_why() {
    let (w, h) =
        livtk_app::ui::diagram::size("flowchart LR\n  A[Plan] --> B[Build] --> C[Ship]\n").unwrap();
    assert!(w > h && h > 10., "wide, as left to right: {w}x{h}");
    assert!(livtk_app::ui::diagram::size("not a diagram at all").is_err());
    assert_eq!(
        livtk_app::ui::diagram::size_of("<svg viewBox=\"0 0 120.5 40\" width=\"10\">"),
        Some((120.5, 40.))
    );
}
