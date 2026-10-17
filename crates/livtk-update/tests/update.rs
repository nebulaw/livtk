//! The updater's contract and its moves, on temporary folders.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::path::Path;
use std::time::Duration;

use livtk_update::stage::{sha256_of, stage, verify};
use livtk_update::swap::{Outcome, Swap, swap};
use livtk_update::{Arch, FORMAT, Found, LocalFolder, Manifest, Platform, Source};

const APP: &str = "com.github.nebulaw.livtk";

fn manifest(build: u64, artifact: &str, sha256: &str, size: u64) -> Manifest {
    Manifest {
        format: FORMAT,
        app: APP.into(),
        version: "0.4.0".into(),
        build,
        commit: "abc1234".into(),
        platform: Platform::this().unwrap(),
        arch: Arch::this().unwrap(),
        artifact: artifact.into(),
        sha256: sha256.into(),
        size,
        notes: String::new(),
        signature: None,
    }
}

fn write(dir: &Path, name: &str, m: &Manifest) {
    std::fs::write(dir.join(name), serde_json::to_string(m).unwrap()).unwrap();
}

const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[test]
fn a_manifest_reads_its_format_ignores_what_it_does_not_know_and_skips_a_newer_one() {
    let m = manifest(7, "livtk-7.zip", HASH, 1);
    let mut json: serde_json::Value = serde_json::to_value(&m).unwrap();
    json["coming_later"] = "anything".into();
    assert_eq!(Manifest::parse(&json.to_string()).unwrap(), Some(m.clone()));
    json["format"] = (FORMAT + 1).into();
    assert_eq!(
        Manifest::parse(&json.to_string()).unwrap(),
        None,
        "a newer format is skipped, never misread"
    );
    let mut bad = m.clone();
    bad.sha256 = "short".into();
    assert!(Manifest::parse(&serde_json::to_string(&bad).unwrap()).is_err());
    let mut away = m;
    away.artifact = "../elsewhere.zip".into();
    assert!(Manifest::parse(&serde_json::to_string(&away).unwrap()).is_err());
    assert!(Manifest::parse("not json").is_err());
}

#[test]
fn the_local_folder_offers_the_newest_build_for_this_app_here() {
    let dir = tempfile::tempdir().unwrap();
    let source = LocalFolder::new(dir.path().join("none"));
    assert_eq!(source.newest(APP).unwrap(), None, "no folder, no update");

    write(dir.path(), "7.json", &manifest(7, "livtk-7.zip", HASH, 1));
    write(dir.path(), "9.json", &manifest(9, "livtk-9.zip", HASH, 1));
    let mut other_app = manifest(20, "x.zip", HASH, 1);
    other_app.app = "com.example.other".into();
    write(dir.path(), "other.json", &other_app);
    let mut other_os = manifest(21, "y.zip", HASH, 1);
    other_os.platform = if other_os.platform == Platform::Linux {
        Platform::Macos
    } else {
        Platform::Linux
    };
    write(dir.path(), "os.json", &other_os);
    std::fs::write(dir.path().join("broken.json"), "{").unwrap();

    let found = LocalFolder::new(dir.path()).newest(APP).unwrap().unwrap();
    assert_eq!(found.manifest.build, 9, "by build number");
    assert_eq!(found.artifact, dir.path().join("livtk-9.zip"));
}

#[test]
fn an_artifact_is_checked_by_size_then_by_hash() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.zip");
    std::fs::write(&file, b"the app").unwrap();
    let hash = sha256_of(&file).unwrap();
    let found = |m: Manifest| Found {
        manifest: m,
        artifact: file.clone(),
    };
    verify(&found(manifest(1, "a.zip", &hash, 7))).unwrap();
    assert!(
        verify(&found(manifest(1, "a.zip", &hash, 8))).is_err(),
        "size"
    );
    assert!(
        verify(&found(manifest(1, "a.zip", HASH, 7))).is_err(),
        "hash"
    );
}

/// A bundle as `scripts/bundle-app` makes it, zipped as `scripts/release` does.
#[cfg(target_os = "macos")]
fn packed_app(dir: &Path, build: u64) -> std::path::PathBuf {
    use std::process::Command;
    let app = dir.join("src/livtk.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{APP}</string>
<key>CFBundleVersion</key><string>{build}</string>
</dict></plist>"#
        ),
    )
    .unwrap();
    let zip = dir.join(format!("livtk-{build}.zip"));
    let ok = Command::new("ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(&app)
        .arg(&zip)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    zip
}

#[cfg(target_os = "macos")]
#[test]
fn staging_unpacks_the_app_and_checks_it_is_the_build_the_manifest_names() {
    let dir = tempfile::tempdir().unwrap();
    let zip = packed_app(dir.path(), 12);
    let hash = sha256_of(&zip).unwrap();
    let size = std::fs::metadata(&zip).unwrap().len();
    let staging = dir.path().join("staging");
    let found = Found {
        manifest: manifest(12, "livtk-12.zip", &hash, size),
        artifact: zip.clone(),
    };
    let staged = stage(&found, &staging).unwrap();
    assert!(staged.app.ends_with("livtk.app"));
    assert!(staged.app.join("Contents/Info.plist").is_file());

    let lying = Found {
        manifest: manifest(13, "livtk-12.zip", &hash, size),
        artifact: zip,
    };
    assert!(stage(&lying, &staging).is_err(), "build 12 inside, 13 said");
}

fn app(dir: &Path, name: &str, mark: &str) -> std::path::PathBuf {
    let app = dir.join(name);
    std::fs::create_dir_all(app.join("Contents")).unwrap();
    std::fs::write(app.join("Contents/mark"), mark).unwrap();
    app
}

fn mark(app: &Path) -> String {
    std::fs::read_to_string(app.join("Contents/mark")).unwrap()
}

#[test]
fn the_swap_puts_the_new_build_in_place_once_it_starts_well() {
    let dir = tempfile::tempdir().unwrap();
    let installed = app(dir.path(), "livtk.app", "old");
    let staged = app(&dir.path().join("staging"), "livtk.app", "new");
    let health = dir.path().join("healthy");
    let s = Swap {
        installed: installed.clone(),
        staged,
        health: health.clone(),
        build: 12,
        wait: Duration::from_secs(2),
    };
    let mut started = Vec::new();
    let outcome = swap(&s, &mut |app: &Path| {
        started.push(mark(app));
        std::fs::write(&health, "12").unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(outcome, Outcome::Updated);
    assert_eq!(mark(&installed), "new");
    assert_eq!(started, ["new"]);
    let left: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect();
    assert!(left.is_empty(), "nothing left beside it: {left:?}");
}

#[test]
fn a_build_that_does_not_start_well_is_rolled_back() {
    let dir = tempfile::tempdir().unwrap();
    let installed = app(dir.path(), "livtk.app", "old");
    let staged = app(&dir.path().join("staging"), "livtk.app", "broken");
    let s = Swap {
        installed: installed.clone(),
        staged,
        health: dir.path().join("healthy"),
        build: 13,
        wait: Duration::from_millis(300),
    };
    let mut started = Vec::new();
    let outcome = swap(&s, &mut |app: &Path| {
        started.push(mark(app));
        Ok(())
    })
    .unwrap();
    assert_eq!(outcome, Outcome::RolledBack);
    assert_eq!(mark(&installed), "old", "the old app is back");
    assert_eq!(started, ["broken", "old"], "and started again");
}
