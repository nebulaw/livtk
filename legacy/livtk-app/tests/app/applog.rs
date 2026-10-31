//! The app's log file: appended to, moved aside when large, never truncated.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::io::Write;

use livtk_app::applog;

#[test]
fn the_log_is_appended_to_and_moved_aside_past_a_megabyte() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("livtk-app.log");

    // Two writers at once, as a handoff has them, each appending: nothing is lost.
    let mut a = applog::open(&path).unwrap();
    let mut b = applog::open(&path).unwrap();
    a.write_all(b"first\n").unwrap();
    b.write_all(b"second\n").unwrap();
    a.write_all(b"third\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "first\nsecond\nthird\n"
    );

    std::fs::write(&path, vec![b'x'; (1 << 20) + 1]).unwrap();
    let mut c = applog::open(&path).unwrap();
    c.write_all(b"fresh\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "fresh\n");
    assert_eq!(
        std::fs::metadata(path.with_extension("log.old"))
            .unwrap()
            .len(),
        (1 << 20) + 1,
        "the old one is kept, once"
    );
}
