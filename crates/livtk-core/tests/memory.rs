//! Lessons, decisions and findings; keyword search; recall with and without the model.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::path::{Path, PathBuf};

use livtk_core::event::Op;
use livtk_core::{Actor, Command, Error, Kind, Rel, Scope, State, Workspace};

const HUMAN: Actor = Actor::Human;
const CLAUDE: Actor = Actor::Claude { agent: None };

/// A workspace that ranks by keywords only: no models directory.
fn keywords_only(dir: &Path) -> Workspace {
    let mut ws = Workspace::init(dir, Some("LIV")).unwrap();
    ws.set_models_dir(None);
    ws
}

fn create(
    ws: &mut Workspace,
    kind: Kind,
    title: &str,
    body: &str,
    links: &[(Rel, &str)],
) -> String {
    let events = ws
        .execute(
            Command::Create {
                kind,
                title: title.into(),
                body: body.into(),
                priority: None,
                links: links.iter().map(|(r, k)| (*r, (*k).to_owned())).collect(),
            },
            &CLAUDE,
        )
        .unwrap();
    match &events[0].op {
        Op::NodeCreate { data, .. } => data.key.clone(),
        other => panic!("{other:?}"),
    }
}

fn mv(ws: &mut Workspace, key: &str, to: State, by: &Actor) -> Result<(), Error> {
    ws.execute(
        Command::Move {
            node: key.into(),
            to,
        },
        by,
    )
    .map(|_| ())
}

fn ratify(ws: &mut Workspace, key: &str, always: bool, by: &Actor) -> Result<(), Error> {
    ws.execute(
        Command::Ratify {
            node: key.into(),
            always,
        },
        by,
    )
    .map(|_| ())
}

fn state(ws: &Workspace, key: &str) -> State {
    ws.node(key).unwrap().state
}

// ---- lessons and decisions ----------------------------------------------------------------------

#[test]
fn each_kind_counts_its_own_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    assert_eq!(create(&mut ws, Kind::Task, "t", "", &[]), "LIV-1");
    assert_eq!(create(&mut ws, Kind::Lesson, "l", "", &[]), "LES-1");
    assert_eq!(create(&mut ws, Kind::Decision, "d", "", &[]), "DEC-1");
    assert_eq!(create(&mut ws, Kind::Finding, "f", "", &[]), "FND-1");
    assert_eq!(create(&mut ws, Kind::Lesson, "l2", "", &[]), "LES-2");
    assert_eq!(create(&mut ws, Kind::Task, "t2", "", &[]), "LIV-2");
    assert!(Workspace::init(tempfile::tempdir().unwrap().path(), Some("LES")).is_err());
}

#[test]
fn only_the_human_decides_what_binds() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let les = create(
        &mut ws,
        Kind::Lesson,
        "Run cargo check before done",
        "why",
        &[],
    );
    let dec = create(&mut ws, Kind::Decision, "SQLite for the store", "why", &[]);

    for (key, to) in [
        (&les, State::Ratified),
        (&les, State::Rejected),
        (&dec, State::Accepted),
        (&dec, State::Rejected),
    ] {
        let err = mv(&mut ws, key, to, &CLAUDE).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "{key} -> {to}: {err}");
    }
    let err = ratify(&mut ws, &les, false, &CLAUDE).unwrap_err();
    assert!(err.to_string().contains("only a human can ratify"), "{err}");

    ratify(&mut ws, &les, false, &HUMAN).unwrap();
    mv(&mut ws, &dec, State::Accepted, &HUMAN).unwrap();
    assert_eq!(state(&ws, &les), State::Ratified);
    assert_eq!(state(&ws, &dec), State::Accepted);
    assert!(
        mv(&mut ws, &les, State::Retired, &CLAUDE).is_err(),
        "retiring a rule is the human's"
    );
    mv(&mut ws, &les, State::Retired, &HUMAN).unwrap();
    ratify(&mut ws, &les, false, &HUMAN).unwrap();
    assert_eq!(
        state(&ws, &les),
        State::Ratified,
        "a retired rule can come back"
    );
}

#[test]
fn rewording_a_rule_needs_ratifying_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let les = create(&mut ws, Kind::Lesson, "Keep commits small", "why", &[]);
    ratify(&mut ws, &les, false, &HUMAN).unwrap();
    ws.execute(
        Command::Edit {
            node: les.clone(),
            title: Some("Keep commits large".into()),
            body: None,
        },
        &CLAUDE,
    )
    .unwrap();
    assert_eq!(state(&ws, &les), State::Proposed);
    let dec = create(&mut ws, Kind::Decision, "d", "why", &[]);
    mv(&mut ws, &dec, State::Accepted, &HUMAN).unwrap();
    ws.execute(
        Command::Edit {
            node: dec.clone(),
            title: None,
            body: Some("new reason".into()),
        },
        &CLAUDE,
    )
    .unwrap();
    assert_eq!(state(&ws, &dec), State::Proposed);
}

#[test]
fn a_note_on_a_rule_is_not_a_change_of_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let les = create(&mut ws, Kind::Lesson, "rule", "why", &[]);
    ratify(&mut ws, &les, false, &HUMAN).unwrap();
    ws.execute(
        Command::Note {
            node: les.clone(),
            text: "seen again today".into(),
        },
        &CLAUDE,
    )
    .unwrap();
    assert_eq!(state(&ws, &les), State::Ratified);
}

#[test]
fn ratifying_a_replacement_retires_the_rule_it_supersedes() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let old = create(
        &mut ws,
        Kind::Lesson,
        "Commit at the end of the day",
        "",
        &[],
    );
    ratify(&mut ws, &old, false, &HUMAN).unwrap();
    let new = create(
        &mut ws,
        Kind::Lesson,
        "Commit as soon as checks pass",
        "",
        &[(Rel::Supersedes, &old)],
    );
    assert_eq!(
        state(&ws, &old),
        State::Ratified,
        "proposing changes nothing yet"
    );
    ratify(&mut ws, &new, false, &HUMAN).unwrap();
    assert_eq!(state(&ws, &old), State::Retired);

    let d1 = create(&mut ws, Kind::Decision, "Postgres", "", &[]);
    mv(&mut ws, &d1, State::Accepted, &HUMAN).unwrap();
    let d2 = create(
        &mut ws,
        Kind::Decision,
        "SQLite",
        "",
        &[(Rel::Supersedes, &d1)],
    );
    let events = ws
        .execute(
            Command::Move {
                node: d2.clone(),
                to: State::Accepted,
            },
            &HUMAN,
        )
        .unwrap();
    assert_eq!(events.len(), 2, "one step, both changes");
    assert!(events.iter().all(|e| e.by == "human"));
    assert_eq!(state(&ws, &d1), State::Superseded);
}

#[test]
fn links_must_make_sense_for_the_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let task = create(&mut ws, Kind::Task, "t", "", &[]);
    let les = create(&mut ws, Kind::Lesson, "l", "", &[]);
    let dec = create(&mut ws, Kind::Decision, "d", "", &[]);
    let attempt = |ws: &mut Workspace, kind: Kind, rel: Rel, to: &str| {
        ws.execute(
            Command::Create {
                kind,
                title: "x".into(),
                body: String::new(),
                priority: None,
                links: vec![(rel, to.to_owned())],
            },
            &CLAUDE,
        )
    };
    assert!(attempt(&mut ws, Kind::Lesson, Rel::DependsOn, &task).is_err());
    assert!(attempt(&mut ws, Kind::Lesson, Rel::Supersedes, &dec).is_err());
    assert!(attempt(&mut ws, Kind::Task, Rel::DependsOn, &les).is_err());
    assert!(attempt(&mut ws, Kind::Lesson, Rel::LearnedFrom, &task).is_ok());
    assert!(attempt(&mut ws, Kind::Finding, Rel::LearnedFrom, &task).is_ok());
    let with_priority = ws.execute(
        Command::Create {
            kind: Kind::Lesson,
            title: "x".into(),
            body: String::new(),
            priority: Some(1),
            links: vec![],
        },
        &CLAUDE,
    );
    assert!(with_priority.is_err(), "only tasks have a priority");
}

#[test]
fn findings_are_recorded_and_stay_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let f = create(&mut ws, Kind::Finding, "winit panics on resize", "", &[]);
    assert_eq!(state(&ws, &f), State::Recorded);
    for to in State::ALL {
        assert!(mv(&mut ws, &f, to, &HUMAN).is_err(), "{to}");
    }
}

#[test]
fn new_kinds_carry_format_3_and_tasks_keep_format_1() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let t = create(&mut ws, Kind::Task, "t", "", &[]);
    let l = create(&mut ws, Kind::Lesson, "l", "", &[]);
    ratify(&mut ws, &l, true, &HUMAN).unwrap();
    let versions: Vec<u32> = ws.history(None).unwrap().iter().map(|e| e.v).collect();
    assert_eq!(versions, [1, 3, 3]);
    let _ = t;
}

#[test]
fn standing_rules_are_marked_by_the_human_and_leave_recall() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let always = create(
        &mut ws,
        Kind::Lesson,
        "Never push release tags to the remote",
        "",
        &[],
    );
    let recalled = create(
        &mut ws,
        Kind::Lesson,
        "Push tags only after the release notes",
        "",
        &[],
    );
    ratify(&mut ws, &always, true, &HUMAN).unwrap();
    ratify(&mut ws, &recalled, false, &HUMAN).unwrap();
    let rules: Vec<String> = ws
        .always_rules()
        .unwrap()
        .into_iter()
        .map(|b| b.key)
        .collect();
    assert_eq!(rules, std::slice::from_ref(&always));

    let found: Vec<String> = ws
        .recall("push the release tags")
        .unwrap()
        .into_iter()
        .map(|h| h.key)
        .collect();
    assert_eq!(
        found,
        std::slice::from_ref(&recalled),
        "a standing rule is already in context"
    );

    assert!(
        ratify(&mut ws, &always, false, &CLAUDE).is_err(),
        "Claude cannot demote it"
    );
    ratify(&mut ws, &always, false, &HUMAN).unwrap();
    assert!(ws.always_rules().unwrap().is_empty());
    assert!(
        ws.execute(
            Command::Ratify {
                node: always.clone(),
                always: false
            },
            &HUMAN
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn the_inbox_holds_everything_waiting_for_the_human() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let t = create(&mut ws, Kind::Task, "t", "", &[]);
    mv(&mut ws, &t, State::Planned, &CLAUDE).unwrap();
    create(&mut ws, Kind::Task, "still backlog", "", &[]);
    let l = create(&mut ws, Kind::Lesson, "l", "", &[]);
    let d = create(&mut ws, Kind::Decision, "d", "", &[]);
    create(&mut ws, Kind::Finding, "f", "", &[]);
    let keys: Vec<String> = ws.inbox().unwrap().into_iter().map(|s| s.key).collect();
    assert_eq!(keys, [t, l, d]);
}

// ---- keyword search -----------------------------------------------------------------------------

#[test]
fn keyword_search_stems_and_prefers_titles() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let body = create(
        &mut ws,
        Kind::Task,
        "Something else",
        "we should check the frames here",
        &[],
    );
    let title = create(&mut ws, Kind::Task, "Check dropped frames", "", &[]);
    let keys: Vec<String> = ws
        .search("checking frame", Scope::All, 5)
        .unwrap()
        .into_iter()
        .map(|h| h.key)
        .collect();
    assert_eq!(keys, [title, body]);
    assert!(
        ws.search("unrelated words entirely", Scope::All, 5)
            .unwrap()
            .is_empty()
    );
    assert!(ws.search("", Scope::All, 5).unwrap().is_empty());
    assert!(
        ws.search("\"OR NEAR(*", Scope::All, 5).is_ok(),
        "no FTS syntax leaks through"
    );
}

#[test]
fn the_index_follows_edits_and_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let t = create(&mut ws, Kind::Task, "Draw the window", "", &[]);
    ws.execute(
        Command::Edit {
            node: t.clone(),
            title: Some("Paint the surface".into()),
            body: None,
        },
        &CLAUDE,
    )
    .unwrap();
    let find = |ws: &mut Workspace, q: &str| ws.search(q, Scope::All, 5).unwrap().len();
    assert_eq!(
        find(&mut ws, "window"),
        0,
        "the old title is gone from the index"
    );
    assert_eq!(find(&mut ws, "surface"), 1);
    ws.rebuild().unwrap();
    assert_eq!(find(&mut ws, "window"), 0);
    assert_eq!(find(&mut ws, "surface"), 1);
}

#[test]
fn recall_without_the_model_needs_two_shared_words() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = keywords_only(dir.path());
    let les = create(
        &mut ws,
        Kind::Lesson,
        "Run cargo check before reporting a task done",
        "",
        &[],
    );
    let proposed = create(
        &mut ws,
        Kind::Lesson,
        "Run cargo fmt before every commit",
        "",
        &[],
    );
    ratify(&mut ws, &les, false, &HUMAN).unwrap();
    let keys = |ws: &mut Workspace, q: &str| -> Vec<String> {
        ws.recall(q).unwrap().into_iter().map(|h| h.key).collect()
    };
    assert_eq!(
        keys(&mut ws, "is the task done? did cargo check pass"),
        std::slice::from_ref(&les)
    );
    assert!(
        keys(&mut ws, "the task is about colours").is_empty(),
        "one shared word is not enough"
    );
    assert!(
        !keys(&mut ws, "cargo fmt before commit").contains(&proposed),
        "unratified lessons never bind"
    );
}

// ---- with the model -----------------------------------------------------------------------------

/// The directory holding `potion-retrieval-32M`, from `LIVTK_TEST_MODELS`.
fn models() -> PathBuf {
    let dir = PathBuf::from(std::env::var("LIVTK_TEST_MODELS").expect(
        "set LIVTK_TEST_MODELS to a directory holding potion-retrieval-32M (see `livtk embed setup`)",
    ));
    assert!(
        dir.join("potion-retrieval-32M/model.safetensors").is_file(),
        "no model in {}",
        dir.display()
    );
    dir
}

/// Twelve rules with realistic reasons, and prompts that should recall them. Measured
/// (2026-10-16, potion-retrieval-32M): 9 of 12 prompts recall their rule first, true matches
/// score 0.19 to 0.48, unrelated prompts at most 0.17.
const RULES: &[(&str, &str)] = &[
    (
        "Run cargo check before reporting a task done",
        "A change that does not compile is not done; claiming done without the check wasted a review round.",
    ),
    (
        "Never push to the remote; the developer pushes",
        "Pushing is outward-facing; the developer decides when work leaves the machine.",
    ),
    (
        "Commit messages use Conventional Commits with the crate as scope",
        "The history stays searchable by crate and kind of change.",
    ),
    (
        "Keep the compositor's frame callback under one millisecond",
        "Anything slower drops frames at 144 Hz.",
    ),
    (
        "Tests check behaviour from the outside and never re-implement the code",
        "A test that mirrors the code proves nothing.",
    ),
    (
        "Ask before deleting files you did not create",
        "Deleted work cannot always be recovered.",
    ),
    (
        "Animations follow macOS timing, slightly faster",
        "The developer wants the feel of macOS with a quicker pace.",
    ),
    (
        "Never store secrets in the repository",
        "Anything committed may be published.",
    ),
    (
        "Prefer SQLite over a server database for local tools",
        "One file, no daemon, decades of reliability.",
    ),
    (
        "Measure before claiming something is fast",
        "Fast is a number from a benchmark, not a feeling.",
    ),
    (
        "Explain the risks before acting on anything outward-facing",
        "The developer decides with the risks in front of them.",
    ),
    (
        "Keep the design system's tokens as the single source of colour",
        "Hard-coded colours drift from the design.",
    ),
];

const MATCHING: &[&str] = &[
    "ok I think the task is finished, let's wrap it up",
    "can you push these commits to the remote",
    "write the commit for this change",
    "the window resize feels laggy, frames are dropping",
    "add a unit test for the parser",
    "clean up the old files in the build folder",
    "make the dock open faster, like on a mac",
    "put the api key in the repo so CI can use it",
    "should we use a postgres server for the task store",
    "I made it quicker, it's definitely faster now",
    "send the release announcement email to the users",
    "change the accent colour of the buttons",
];

const UNRELATED: &[&str] = &[
    "what's the weather like today",
    "rename the variable x to count",
    "refactor the parser into two functions",
    "explain how the event log works",
    "summarise this article for me",
    "fix the typo in the readme",
];

fn with_rules(dir: &Path) -> (Workspace, Vec<String>) {
    let mut ws = Workspace::init(dir, Some("LIV")).unwrap();
    ws.set_models_dir(Some(models()));
    let keys = RULES
        .iter()
        .map(|(t, b)| {
            let k = create(&mut ws, Kind::Lesson, t, b, &[]);
            ratify(&mut ws, &k, false, &HUMAN).unwrap();
            k
        })
        .collect();
    (ws, keys)
}

#[test]
#[ignore = "needs the model: LIVTK_TEST_MODELS=<dir> cargo test -- --ignored"]
fn with_model_recall_finds_rules_by_meaning_and_ignores_the_unrelated() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, keys) = with_rules(dir.path());
    assert!(ws.model_status().2, "the model loads");
    let mut first = 0;
    for (i, prompt) in MATCHING.iter().enumerate() {
        let found = ws.recall(prompt).unwrap();
        if found.first().map(|h| h.key.as_str()) == Some(keys[i].as_str()) {
            first += 1;
        }
    }
    // A broken embedding scores about 1 in 12; the model measured 9.
    assert!(
        first >= 8,
        "only {first} of 12 prompts recalled their rule first"
    );
    for prompt in UNRELATED {
        assert!(ws.recall(prompt).unwrap().is_empty(), "{prompt}");
    }
}

#[test]
#[ignore = "needs the model: LIVTK_TEST_MODELS=<dir> cargo test -- --ignored"]
fn with_model_vectors_survive_a_rebuild_and_follow_edits() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, keys) = with_rules(dir.path());
    ws.recall("anything at all").unwrap();
    assert_eq!(
        ws.embedded(),
        RULES.len(),
        "every recallable rule embedded once"
    );
    ws.recall("something else").unwrap();
    assert_eq!(ws.embedded(), RULES.len(), "stored vectors are reused");

    ws.rebuild().unwrap();
    ws.recall("anything at all").unwrap();
    assert_eq!(ws.embedded(), RULES.len(), "a rebuild never re-embeds");
    let mut reopened = Workspace::open(dir.path()).unwrap();
    reopened.set_models_dir(Some(models()));
    reopened.recall("anything at all").unwrap();
    assert_eq!(reopened.embedded(), 0, "nor does a new process");

    ws.execute(
        Command::Edit {
            node: keys[0].clone(),
            title: None,
            body: Some("a new reason".into()),
        },
        &CLAUDE,
    )
    .unwrap();
    ratify(&mut ws, &keys[0], false, &HUMAN).unwrap();
    ws.recall("anything at all").unwrap();
    assert_eq!(
        ws.embedded(),
        RULES.len() + 1,
        "only the edited rule re-embeds"
    );
}

#[test]
#[ignore = "needs the model: LIVTK_TEST_MODELS=<dir> cargo test -- --ignored"]
fn with_a_missing_model_search_falls_back_to_keywords() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, keys) = with_rules(dir.path());
    let empty = tempfile::tempdir().unwrap();
    ws.set_models_dir(Some(empty.path().to_path_buf()));
    assert!(!ws.model_status().2);
    let found = ws
        .recall("did cargo check pass before the task was reported done")
        .unwrap();
    assert_eq!(
        found.first().map(|h| h.key.as_str()),
        Some(keys[0].as_str())
    );
    assert!(found[0].cosine.is_none());
    assert!(
        ws.take_warnings().is_empty(),
        "a missing model is not a problem, only an absence"
    );
}

#[test]
#[ignore = "needs the model: LIVTK_TEST_MODELS=<dir> cargo test -- --ignored"]
fn with_model_a_write_by_another_process_refreshes_the_kept_vectors() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, keys) = with_rules(dir.path());
    ws.search("anything at all", Scope::All, 5).unwrap();
    let before = ws.embedded();
    ws.search("something else", Scope::All, 5).unwrap();
    assert_eq!(ws.embedded(), before, "kept vectors serve the next search");

    let mut other = Workspace::open(dir.path()).unwrap();
    other.set_models_dir(None);
    other
        .execute(
            Command::Edit {
                node: keys[0].clone(),
                title: None,
                body: Some("a reason written by another process".into()),
            },
            &CLAUDE,
        )
        .unwrap();
    ws.search("anything at all", Scope::All, 5).unwrap();
    assert_eq!(
        ws.embedded(),
        before + 1,
        "the other process's edit is seen and only that text re-embeds"
    );
}

#[test]
#[ignore = "needs the model: LIVTK_TEST_MODELS=<dir> cargo test -- --ignored"]
fn with_model_find_brings_no_noise_for_a_half_typed_word() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _) = with_rules(dir.path());
    assert!(
        !ws.search("zq", Scope::All, 5).unwrap().is_empty(),
        "search ranks everything by meaning, however weakly"
    );
    assert!(
        ws.find("zq", 5).unwrap().is_empty(),
        "find keeps only what clears the relevance bar"
    );
}
