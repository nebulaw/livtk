//! Where the app runs, and its command line tools: on macOS one bundle holds the app and the CLI,
//! with links on PATH; started through such a link, `livtk-app` opens the bundle.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use livtk_app::launch::{Start, bundle_of, in_bundle, passed_on, start};

const BUNDLED: &str = "/Users/k/Applications/livtk.app/Contents/MacOS/livtk-app";

#[test]
fn a_bundle_is_told_by_its_path() {
    assert!(in_bundle(Path::new(BUNDLED)));
    assert_eq!(
        bundle_of(Path::new(BUNDLED)),
        Some(PathBuf::from("/Users/k/Applications/livtk.app"))
    );
    assert!(!in_bundle(Path::new("/Users/k/.cargo/bin/livtk-app")));
    assert!(
        !in_bundle(Path::new("/x/Contents/MacOS/livtk-app")),
        "not an .app"
    );
    assert!(!in_bundle(Path::new("livtk-app")));
}

#[test]
fn how_it_starts_follows_from_where_it_was_started_and_where_that_leads() {
    let link = Path::new("/Users/k/.local/bin/livtk-app");
    assert_eq!(
        start(link, Path::new(BUNDLED)),
        Start::Open(PathBuf::from("/Users/k/Applications/livtk.app")),
        "a link into the bundle opens it"
    );
    assert_eq!(
        start(Path::new(BUNDLED), Path::new(BUNDLED)),
        Start::Bundled,
        "inside the bundle it runs, as macOS starts it"
    );
    for bare in [
        "/w/tasks/target/debug/livtk-app",
        "/Users/k/.cargo/bin/livtk-app",
        "/w/custom-target/release/livtk-app",
    ] {
        assert_eq!(
            start(Path::new(bare), Path::new(bare)),
            Start::Bare,
            "{bare} runs as itself, whatever its folder is called"
        );
    }
}

#[test]
fn the_bundle_gets_what_of_the_environment_finds_the_same_livtk() {
    let vars = [
        ("LIVTK_CONFIG_DIR", "/tmp/conf"),
        ("XDG_CACHE_HOME", "/tmp/cache"),
        ("LIVTK_APP_STARTED_IN", "/w"),
        ("PATH", "/bin"),
        ("HOME", "/Users/k"),
    ]
    .map(|(k, v)| (OsString::from(k), OsString::from(v)));
    assert_eq!(
        passed_on(vars),
        ["LIVTK_CONFIG_DIR=/tmp/conf", "XDG_CACHE_HOME=/tmp/cache"]
    );
}

#[cfg(unix)]
#[test]
fn the_command_line_tools_are_linked_into_the_bundle_again_safely() {
    use livtk_app::launch::install_cli;
    let home = tempfile::tempdir().unwrap();
    let bundle = home.path().join("Applications/livtk.app");
    let macos = bundle.join("Contents/MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    for tool in ["livtk", "livtk-app"] {
        std::fs::write(macos.join(tool), "#!/bin/sh\n").unwrap();
    }
    let bin = home.path().join(".local/bin");
    let cargo = home.path().join(".cargo/bin");
    std::fs::create_dir_all(&cargo).unwrap();
    std::fs::write(cargo.join("livtk"), "an older copy").unwrap();

    // ~/.cargo/bin comes first: its livtk would run instead.
    let path = std::env::join_paths([&cargo, &bin]).unwrap();
    let done = install_cli(&bundle, &bin, &path).unwrap();
    assert_eq!(done.linked, [bin.join("livtk"), bin.join("livtk-app")]);
    assert_eq!(
        std::fs::read_link(bin.join("livtk")).unwrap(),
        macos.join("livtk")
    );
    assert!(done.on_path);
    assert_eq!(
        done.shadowed,
        [cargo.join("livtk")],
        "the older copy is named"
    );

    // Again: the links are made anew, nothing else changes.
    let again = install_cli(&bundle, &bin, &path).unwrap();
    assert_eq!(again.linked.len(), 2);

    // A file in the way is left alone; a folder not on PATH is said.
    std::fs::remove_file(bin.join("livtk-app")).unwrap();
    std::fs::write(bin.join("livtk-app"), "mine").unwrap();
    let odd = install_cli(&bundle, &bin, &std::env::join_paths([&cargo]).unwrap()).unwrap();
    assert_eq!(odd.refused, [bin.join("livtk-app")]);
    assert_eq!(
        std::fs::read_to_string(bin.join("livtk-app")).unwrap(),
        "mine"
    );
    assert!(!odd.on_path);

    // Our own link earlier on PATH is no shadow; a copy later on it is left over.
    std::fs::remove_file(bin.join("livtk-app")).unwrap();
    let ours = install_cli(
        &bundle,
        &bin,
        &std::env::join_paths([&bin, &cargo]).unwrap(),
    )
    .unwrap();
    assert!(ours.shadowed.is_empty());
    assert_eq!(ours.leftover, [cargo.join("livtk")], "named, to be removed");

    // A linked bin folder is still on PATH; an empty PATH entry is no folder of its own.
    let linked_bin = home.path().join("bin-link");
    std::os::unix::fs::symlink(&bin, &linked_bin).unwrap();
    let path = std::ffi::OsString::from(format!(":{}", linked_bin.display()));
    let via_link = install_cli(&bundle, &bin, &path).unwrap();
    assert!(via_link.on_path, "{via_link:?}");
    assert!(via_link.shadowed.is_empty() && via_link.leftover.is_empty());

    // A bundle without one of its tools is refused, not linked to nothing.
    std::fs::remove_file(macos.join("livtk")).unwrap();
    assert!(install_cli(&bundle, &bin, &path).is_err());
}

#[test]
fn livtk_app_says_which_build_it_is() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_livtk-app"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        format!("livtk-app {}", livtk_build::DESCRIBE)
    );
}
