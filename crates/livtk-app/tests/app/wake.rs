//! What wakes the app (DEC-8): the OS's file events on what it shows, and a notice's own time,
//! never a beat while nothing moves.

use std::path::Path;
use std::time::{Duration, Instant};

use async_channel::Receiver;
use livtk_app::model::{Model, Toast, Toasts};
use livtk_app::watch::{Watch, folders};
use livtk_core::Workspace;
use notify::RecursiveMode;

const FADE: Duration = Duration::from_millis(240);

#[test]
fn a_notice_wakes_the_app_when_its_time_is_up_and_one_that_stays_wakes_nothing() {
    let now = Instant::now();
    let mut t = Toasts::default();
    assert_eq!(t.next_change(now, FADE), None, "nothing shown");

    // An error stays until closed: nothing to wake for, however long it stands.
    t.raise(Toast::error("sync failed"));
    assert_eq!(t.next_change(now, FADE), None);

    // A notice leaves when its time is up, and is gone a fade later.
    let id = t.raise(Toast::info("saved"));
    let now = Instant::now();
    let due = t.next_change(now, FADE).expect("a notice's time");
    assert!(
        due <= Duration::from_secs(6) && due > Duration::from_secs(5),
        "{due:?}"
    );
    let later = now + Duration::from_secs(7);
    assert!(t.age(later, FADE), "it begins to leave");
    assert_eq!(t.next_change(later, FADE), Some(FADE));

    // One the pointer holds stays while held.
    t.hold(id, true, later);
    assert_eq!(t.next_change(later, FADE), None);
}

#[test]
fn a_notice_raised_while_idle_shortens_the_wait_and_an_error_does_not() {
    let dir = tempfile::tempdir().unwrap();
    Workspace::init(dir.path(), Some("LIV")).unwrap();
    let mut m = Model::open(&[dir.path().to_path_buf()]);
    m.set_models_dir(None);
    let now = Instant::now();
    let idle = m.next_wake(now, FADE);

    m.notify(Toast::error("could not update"));
    assert_eq!(
        m.next_wake(now, FADE),
        idle,
        "a standing error brings no beat back"
    );

    m.notify(Toast::info("linked"));
    let wait = m.next_wake(now, FADE);
    assert!(
        wait <= Duration::from_secs(6).min(idle),
        "{wait:?}, idle {idle:?}"
    );
}

#[test]
fn a_folder_not_made_yet_is_waited_for_on_the_folder_above_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let livtk = root.join(livtk_core::DIR_NAME);
    std::fs::create_dir(&livtk).unwrap();

    // No runs and no docs yet: their making shows on `.livtk/` and the project's folder.
    assert_eq!(
        folders(root, &root.join("docs")),
        vec![
            (livtk.clone(), RecursiveMode::NonRecursive),
            (root.to_path_buf(), RecursiveMode::NonRecursive),
        ]
    );

    std::fs::create_dir_all(livtk.join(livtk_core::run::RUNS)).unwrap();
    std::fs::create_dir(root.join("docs")).unwrap();
    assert_eq!(
        folders(root, &root.join("docs")),
        vec![
            (livtk.clone(), RecursiveMode::NonRecursive),
            (livtk.join(livtk_core::run::RUNS), RecursiveMode::Recursive),
            (root.join("docs"), RecursiveMode::Recursive),
        ]
    );
}

/// Whether a wake comes within `within`.
fn woken(rx: &Receiver<()>, within: Duration) -> bool {
    let end = Instant::now() + within;
    while Instant::now() < end {
        if rx.try_recv().is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// The real watcher, outside GPUI's test scheduler (which allows it no thread).
#[test]
fn a_change_from_outside_wakes_the_app_and_its_own_writes_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let livtk = root.join(livtk_core::DIR_NAME);
    std::fs::create_dir(&livtk).unwrap();
    let docs = root.join("docs");

    let (mut watch, rx) = Watch::new(true);
    watch.follow(folders(&root, &docs));
    assert!(watch.working(), "a local folder is watched");
    // Whatever the setting up stirred settles first.
    woken(&rx, Duration::from_millis(300));
    while rx.try_recv().is_ok() {}

    // The app's own projection is no news.
    std::fs::write(livtk.join("livtk.db"), b"x").unwrap();
    assert!(
        !woken(&rx, Duration::from_millis(500)),
        "its own writes wake nothing"
    );

    // The CLI appending to the log is.
    std::fs::write(livtk.join("events.jsonl"), b"{}\n").unwrap();
    assert!(
        woken(&rx, Duration::from_secs(2)),
        "a write to the log wakes it"
    );

    // A docs folder made after start wakes it, and is watched from then on.
    std::fs::create_dir(&docs).unwrap();
    assert!(
        woken(&rx, Duration::from_secs(2)),
        "a new docs folder wakes it"
    );
    watch.follow(folders(&root, &docs));
    woken(&rx, Duration::from_millis(300));
    std::fs::write(docs.join("a.md"), b"# a").unwrap();
    assert!(woken(&rx, Duration::from_secs(2)), "a doc written wakes it");
}

#[test]
fn a_folder_that_cannot_be_watched_brings_the_old_beat_back() {
    let (mut watch, _rx) = Watch::new(true);
    watch.follow(vec![(
        Path::new("/no/such/folder").to_path_buf(),
        RecursiveMode::Recursive,
    )]);
    assert!(!watch.working(), "a failed watch is not trusted to tell");
    watch.follow(Vec::new());
    assert!(watch.working(), "and once it is gone, the events are again");
}
