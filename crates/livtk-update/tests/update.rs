//! The updater's contract and its moves, on temporary folders.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::path::Path;
use std::time::Duration;

use livtk_update::stage::{sha256_of, stage, verify};
use livtk_update::swap::{Outcome, Swap, swap};
use livtk_update::{Arch, Artifact, FORMAT, Found, LocalFolder, Manifest, Platform, Source};

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
    assert_eq!(
        found.artifact,
        Artifact::File(dir.path().join("livtk-9.zip"))
    );
}

#[test]
fn an_artifact_is_checked_by_size_then_by_hash() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.zip");
    std::fs::write(&file, b"the app").unwrap();
    let hash = sha256_of(&file).unwrap();
    let found = |m: Manifest| Found {
        manifest: m,
        artifact: Artifact::File(file.clone()),
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
        artifact: Artifact::File(zip.clone()),
    };
    let staged = stage(&found, &staging).unwrap();
    assert!(staged.app.ends_with("livtk.app"));
    assert!(staged.app.join("Contents/Info.plist").is_file());

    let lying = Found {
        manifest: manifest(13, "livtk-12.zip", &hash, size),
        artifact: Artifact::File(zip),
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

/// A launcher that records what it starts and stops; `healthy` says whether the new build
/// reports, and `fails` makes starting the new build fail outright.
struct Fake {
    started: Vec<(String, bool)>,
    stopped: usize,
    healthy: Option<(std::path::PathBuf, u64)>,
    fails: bool,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            started: Vec::new(),
            stopped: 0,
            healthy: None,
            fails: false,
        }
    }
}

impl livtk_update::swap::Launcher for Fake {
    fn start(&mut self, app: &Path, health: Option<&Path>) -> livtk_update::Result<()> {
        self.started.push((mark(app), health.is_some()));
        if health.is_some() && self.fails {
            return Err(livtk_update::Error::Failed("open failed".into()));
        }
        if let (Some(h), Some((_, build))) = (health, &self.healthy) {
            std::fs::write(h, build.to_string()).unwrap();
        }
        Ok(())
    }

    fn stop(&mut self, _: &Path) {
        self.stopped += 1;
    }
}

fn hidden(dir: &Path) -> Vec<String> {
    let mut left: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect();
    left.sort();
    left
}

fn setup(dir: &Path, build: u64, wait: Duration) -> Swap {
    Swap {
        installed: app(dir, "livtk.app", "old"),
        staged: app(&dir.join("staging"), "livtk.app", "new"),
        health: dir.join("healthy"),
        build,
        wait,
    }
}

#[test]
fn a_swap_that_fails_before_anything_starts_leaves_the_old_app_started() {
    let dir = tempfile::tempdir().unwrap();
    let s = setup(dir.path(), 12, Duration::from_secs(1));
    // The staged build is gone by the time the swap runs (cleaned away, a full disk).
    std::fs::remove_dir_all(&s.staged).unwrap();
    let mut fake = Fake::new();
    assert!(swap(&s, &mut fake).is_err());
    assert_eq!(mark(&s.installed), "old", "the old app is where it was");
    assert_eq!(
        fake.started,
        vec![("old".to_owned(), false)],
        "and running again: it had quit for the update"
    );
}

#[test]
fn a_swap_failing_once_the_new_build_is_in_place_starts_only_the_old_one() {
    let dir = tempfile::tempdir().unwrap();
    let s = setup(dir.path(), 12, Duration::from_secs(1));
    // The health file can't be cleared (a folder with something in it stands there).
    std::fs::create_dir_all(s.health.join("x")).unwrap();
    let mut fake = Fake::new();
    assert!(swap(&s, &mut fake).is_err());
    assert_eq!(mark(&s.installed), "old", "the old app is back in place");
    assert_eq!(
        fake.started,
        vec![("old".to_owned(), false)],
        "only it is started, once; the new build never unchecked"
    );
    assert_eq!(
        hidden(dir.path()),
        vec![".livtk.app-failed"],
        "the new build moved out"
    );
}

#[test]
fn a_rollback_that_can_t_move_the_failed_build_aside_still_puts_the_old_one_back() {
    let dir = tempfile::tempdir().unwrap();
    let s = setup(dir.path(), 12, Duration::from_millis(300));
    // Where the failed build would go, a file that can't be cleared as a folder.
    std::fs::write(dir.path().join(".livtk.app-failed"), "in the way").unwrap();
    let mut fake = Fake::new(); // the new build never says it started
    assert_eq!(swap(&s, &mut fake).unwrap(), Outcome::RolledBack);
    assert_eq!(mark(&s.installed), "old");
    assert_eq!(
        fake.started,
        vec![("new".to_owned(), true), ("old".to_owned(), false)],
        "the new build tried once, then the old one started once"
    );
}

/// A leftover from an earlier swap is never taken for the app this swap moved aside.
#[test]
fn a_leftover_that_can_t_be_cleared_leaves_the_current_app_in_place_and_started() {
    let dir = tempfile::tempdir().unwrap();
    let s = setup(dir.path(), 12, Duration::from_secs(1));
    // Where the old app would be moved aside: an earlier swap's leftover that can't be removed.
    let leftover = dir.path().join(".livtk.app-previous");
    std::fs::write(&leftover, "a broken leftover").unwrap();
    let mut fake = Fake::new();
    assert!(swap(&s, &mut fake).is_err());
    assert_eq!(mark(&s.installed), "old", "the current app never moved");
    assert_eq!(
        fake.started,
        vec![("old".to_owned(), false)],
        "and it alone started"
    );
    assert!(
        leftover.is_file(),
        "the leftover left alone, never put in its place"
    );
}

/// When the failed build's usual spot is blocked, it moves to a free one: the old app goes back
/// in place and starts from there, never what's left of the new build.
#[cfg(unix)]
#[test]
fn a_failed_build_whose_spot_is_blocked_still_makes_way_for_the_old_one() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let s = setup(dir.path(), 12, Duration::from_millis(300));
    // Inside the new build, a folder whose contents can't be removed; and its usual spot taken.
    let locked = s.staged.join("Contents/locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(locked.join("x"), "").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    std::fs::write(dir.path().join(".livtk.app-failed"), "in the way").unwrap();

    let mut fake = Fake::new(); // the new build never says it started
    let outcome = swap(&s, &mut fake);
    let moved = dir.path().join(".livtk.app-failed-2");
    let _ = std::fs::set_permissions(
        moved.join("Contents/locked"),
        std::fs::Permissions::from_mode(0o755),
    );
    assert_eq!(outcome.unwrap(), Outcome::RolledBack);
    assert_eq!(mark(&s.installed), "old", "the old app back in place");
    assert_eq!(mark(&moved), "new", "the failed build moved to a free spot");
    assert_eq!(
        fake.started,
        vec![("new".to_owned(), true), ("old".to_owned(), false)],
        "the new build tried once with its health check, then the old one, once"
    );
}

/// An earlier swap interrupted with the old app moved aside: it goes back first, before anything
/// that could fail, so it's started even when the rest can't go on.
#[cfg(unix)]
#[test]
fn an_interrupted_swap_s_old_app_is_put_back_before_anything_can_fail() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let s = setup(dir.path(), 12, Duration::from_secs(1));
    let aside = dir.path().join(".livtk.app-previous");
    std::fs::rename(&s.installed, &aside).unwrap();
    // The interrupted swap also left an incoming copy that can't be removed.
    let locked = dir.path().join(".livtk.app-incoming/Contents/locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(locked.join("x"), "").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();

    let mut fake = Fake::new();
    let outcome = swap(&s, &mut fake);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(outcome.is_err());
    assert_eq!(mark(&s.installed), "old", "back in place");
    assert_eq!(
        fake.started,
        vec![("old".to_owned(), false)],
        "and started, once"
    );
}

#[test]
fn the_swap_puts_the_new_build_in_place_once_it_starts_well() {
    let dir = tempfile::tempdir().unwrap();
    let s = setup(dir.path(), 12, Duration::from_secs(2));
    let mut fake = Fake::new();
    fake.healthy = Some((s.health.clone(), 12));
    assert_eq!(swap(&s, &mut fake).unwrap(), Outcome::Updated);
    assert_eq!(mark(&s.installed), "new");
    assert_eq!(fake.started, [("new".into(), true)], "asked to report");
    assert!(hidden(dir.path()).is_empty(), "nothing left beside it");
}

#[test]
fn a_build_that_does_not_start_well_is_stopped_and_rolled_back() {
    let dir = tempfile::tempdir().unwrap();
    let s = setup(dir.path(), 13, Duration::from_millis(300));
    let mut fake = Fake::new();
    assert_eq!(swap(&s, &mut fake).unwrap(), Outcome::RolledBack);
    assert_eq!(mark(&s.installed), "old", "the old app is back");
    assert_eq!(fake.stopped, 1, "the new build stopped first");
    assert_eq!(
        fake.started,
        [("new".into(), true), ("old".into(), false)],
        "the old one started again, not asked to report"
    );
}

#[test]
fn a_build_that_cannot_even_be_started_is_rolled_back_and_the_old_app_never_lost() {
    let dir = tempfile::tempdir().unwrap();
    let s = setup(dir.path(), 14, Duration::from_millis(300));
    let mut fake = Fake::new();
    fake.fails = true;
    assert_eq!(swap(&s, &mut fake).unwrap(), Outcome::RolledBack);
    assert_eq!(mark(&s.installed), "old");
    assert!(!hidden(dir.path()).contains(&".livtk.app-previous".to_owned()));
    // Another update after it still has the old app to fall back on.
    let again = Swap {
        staged: app(&dir.path().join("staging2"), "livtk.app", "newer"),
        ..s
    };
    assert_eq!(swap(&again, &mut fake).unwrap(), Outcome::RolledBack);
    assert_eq!(mark(&again.installed), "old");
}

#[test]
fn an_artifact_beyond_the_manifests_folder_is_refused() {
    let mut m = manifest(1, "/etc/hosts", HASH, 1);
    assert!(Manifest::parse(&serde_json::to_string(&m).unwrap()).is_err());
    m.artifact = "../x.zip".into();
    assert!(Manifest::parse(&serde_json::to_string(&m).unwrap()).is_err());
}

#[test]
fn an_interrupted_swap_gets_its_old_app_back_first() {
    let dir = tempfile::tempdir().unwrap();
    let s = setup(dir.path(), 15, Duration::from_millis(300));
    // As if a swap stopped after moving the old app aside.
    std::fs::rename(&s.installed, dir.path().join(".livtk.app-previous")).unwrap();
    let mut fake = Fake::new();
    assert_eq!(swap(&s, &mut fake).unwrap(), Outcome::RolledBack);
    assert_eq!(mark(&s.installed), "old");
}

// ---- Signed manifests -------------------------------------------------------------------

/// A fresh key pair: its public key as an app holds it, and a way to sign with its secret one.
struct Signer {
    public: String,
    secret: minisign::SecretKey,
}

impl Signer {
    fn new() -> Signer {
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        Signer {
            public: kp.pk.to_base64(),
            secret: kp.sk,
        }
    }

    fn sign(&self, bytes: &[u8]) -> String {
        minisign::sign(None, &self.secret, std::io::Cursor::new(bytes), None, None)
            .unwrap()
            .into_string()
    }

    fn keys(&self) -> Keys {
        Keys::new([self.public.as_str()]).unwrap()
    }
}

use livtk_update::sign::{Keys, signature_name};

#[test]
fn a_manifest_is_trusted_only_when_a_trusted_key_signed_it_as_it_is() {
    let ours = Signer::new();
    let theirs = Signer::new();
    let body = br#"{"format":1}"#;
    let sig = ours.sign(body);

    assert!(
        ours.keys().verify(body, &sig).is_ok(),
        "signed by a trusted key"
    );
    assert!(
        ours.keys().verify(br#"{"format":2}"#, &sig).is_err(),
        "changed after signing"
    );
    assert!(theirs.keys().verify(body, &sig).is_err(), "another key");
    assert!(ours.keys().verify(body, "not a signature").is_err());
    assert!(
        Keys::default().verify(body, &sig).is_err(),
        "no key is trusted"
    );
    let both = Keys::new([theirs.public.as_str(), "", ours.public.as_str()]).unwrap();
    assert!(
        both.verify(body, &sig).is_ok(),
        "any trusted key, empty slots skipped"
    );
    assert!(Keys::new(["not a key"]).is_err());
}

fn signed(dir: &Path, name: &str, m: &Manifest, by: &Signer) {
    let body = serde_json::to_string(m).unwrap();
    std::fs::write(dir.join(name), &body).unwrap();
    std::fs::write(dir.join(signature_name(name)), by.sign(body.as_bytes())).unwrap();
}

#[test]
fn a_trusting_folder_offers_the_newest_signed_build_and_passes_over_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let ours = Signer::new();
    let theirs = Signer::new();
    signed(dir.path(), "a.json", &manifest(5, "a.zip", HASH, 1), &ours);
    // Newer, but unsigned, signed by another key, or changed after signing: passed over.
    write(dir.path(), "b.json", &manifest(6, "b.zip", HASH, 1));
    signed(
        dir.path(),
        "c.json",
        &manifest(7, "c.zip", HASH, 1),
        &theirs,
    );
    signed(dir.path(), "d.json", &manifest(8, "d.zip", HASH, 1), &ours);
    write(dir.path(), "d.json", &manifest(9, "d.zip", HASH, 1));

    let folder = LocalFolder::trusting(dir.path(), ours.keys());
    let found = folder.newest(APP).unwrap().unwrap();
    assert_eq!(found.manifest.build, 5);
    // The same folder unverified (livtk.app's own channel) offers the newest of all.
    assert_eq!(
        LocalFolder::new(dir.path())
            .newest(APP)
            .unwrap()
            .unwrap()
            .manifest
            .build,
        9
    );
}

#[test]
fn only_a_newer_build_is_offered() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.json", &manifest(5, "a.zip", HASH, 1));
    let folder = LocalFolder::new(dir.path());
    let newer = |this| livtk_update::source::newer(&folder, APP, this).unwrap();
    assert_eq!(newer(4).map(|f| f.manifest.build), Some(5));
    assert!(newer(5).is_none(), "the same build");
    assert!(newer(6).is_none(), "an older one");
    let empty = tempfile::tempdir().unwrap();
    assert!(
        livtk_update::source::newer(&LocalFolder::new(empty.path()), APP, 0)
            .unwrap()
            .is_none(),
        "nothing there"
    );
}

#[test]
fn names_are_written_as_manifests_write_them() {
    for p in [Platform::Macos, Platform::Windows, Platform::Linux] {
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            format!("\"{}\"", p.name())
        );
    }
    for a in [Arch::Aarch64, Arch::X86_64] {
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            format!("\"{}\"", a.name())
        );
    }
    assert_eq!(
        livtk_update::manifest::release_name("liv", Platform::Macos, Arch::Aarch64),
        "liv-macos-aarch64.json"
    );
}

// ---- GitHub Releases, offline -----------------------------------------------------------

use livtk_update::fetch::Fetch;
use livtk_update::github::{GitHubReleases, Track};
use std::collections::HashMap;

/// The web, as a table of bodies by URL; what isn't in it is a 404.
struct Web(HashMap<String, Vec<u8>>);

impl Fetch for Web {
    fn get(&self, url: &str) -> livtk_update::Result<Option<Vec<u8>>> {
        Ok(self.0.get(url).cloned())
    }
}

const DL: &str = "https://dl.example";

/// A release with this platform's manifest (signed by `by`, naming `zip`) and its artifact.
fn release(
    web: &mut HashMap<String, Vec<u8>>,
    tag: &str,
    m: &Manifest,
    by: &Signer,
    draft: bool,
    prerelease: bool,
) -> serde_json::Value {
    let name = livtk_update::manifest::release_name(
        "liv",
        Platform::this().unwrap(),
        Arch::this().unwrap(),
    );
    let body = serde_json::to_string(m).unwrap();
    let url = |n: &str| format!("{DL}/{tag}/{n}");
    web.insert(url(&name), body.clone().into_bytes());
    web.insert(
        url(&signature_name(&name)),
        by.sign(body.as_bytes()).into_bytes(),
    );
    serde_json::json!({
        "tag_name": tag, "draft": draft, "prerelease": prerelease,
        "assets": [
            {"name": name, "browser_download_url": url(&name)},
            {"name": signature_name(&name), "browser_download_url": url(&signature_name(&name))},
            {"name": m.artifact, "browser_download_url": url(&m.artifact)},
        ]
    })
}

fn github(web: HashMap<String, Vec<u8>>, track: Track, keys: Keys) -> GitHubReleases {
    let mut g = GitHubReleases::new("o/r", track, "liv", keys, Box::new(Web(web)));
    g.api = "https://api.example".into();
    g
}

#[test]
fn stable_follows_the_latest_release_and_preview_the_newest_one() {
    let ours = Signer::new();
    let mut web = HashMap::new();
    let stable = release(
        &mut web,
        "v1",
        &manifest(10, "liv-10.zip", HASH, 1),
        &ours,
        false,
        false,
    );
    let pre = release(
        &mut web,
        "v2-pre",
        &manifest(11, "liv-11.zip", HASH, 1),
        &ours,
        false,
        true,
    );
    let draft = release(
        &mut web,
        "v3",
        &manifest(12, "liv-12.zip", HASH, 1),
        &ours,
        true,
        false,
    );
    web.insert(
        "https://api.example/repos/o/r/releases/latest".into(),
        stable.to_string().into_bytes(),
    );
    web.insert(
        "https://api.example/repos/o/r/releases?per_page=30".into(),
        serde_json::json!([draft, pre, stable])
            .to_string()
            .into_bytes(),
    );

    let s = github(web.clone(), Track::Stable, ours.keys())
        .newest(APP)
        .unwrap()
        .unwrap();
    assert_eq!(s.manifest.build, 10);
    assert_eq!(s.artifact, Artifact::Url(format!("{DL}/v1/liv-10.zip")));
    let p = github(web, Track::Preview, ours.keys())
        .newest(APP)
        .unwrap()
        .unwrap();
    assert_eq!(p.manifest.build, 11, "the newest that isn't a draft");
}

#[test]
fn a_release_signed_by_another_key_or_unsigned_is_refused() {
    let ours = Signer::new();
    let theirs = Signer::new();
    let mut web = HashMap::new();
    let r = release(
        &mut web,
        "v1",
        &manifest(10, "liv-10.zip", HASH, 1),
        &theirs,
        false,
        false,
    );
    web.insert(
        "https://api.example/repos/o/r/releases/latest".into(),
        r.to_string().into_bytes(),
    );
    assert!(
        github(web.clone(), Track::Stable, ours.keys())
            .newest(APP)
            .is_err(),
        "another key"
    );
    // Its signature gone from the release.
    let name = livtk_update::manifest::release_name(
        "liv",
        Platform::this().unwrap(),
        Arch::this().unwrap(),
    );
    web.remove(&format!("{DL}/v1/{}", signature_name(&name)));
    assert!(
        github(web, Track::Stable, theirs.keys())
            .newest(APP)
            .is_err(),
        "no signature"
    );
}

#[test]
fn no_release_or_none_for_this_platform_offers_nothing() {
    let ours = Signer::new();
    assert!(
        github(HashMap::new(), Track::Stable, ours.keys())
            .newest(APP)
            .unwrap()
            .is_none()
    );
    let mut web = HashMap::new();
    web.insert(
        "https://api.example/repos/o/r/releases/latest".into(),
        serde_json::json!({"draft": false, "assets": [
            {"name": "liv-plan9-mips.json", "browser_download_url": "x"}
        ]})
        .to_string()
        .into_bytes(),
    );
    assert!(
        github(web, Track::Stable, ours.keys())
            .newest(APP)
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_downloaded_artifact_is_checked_as_a_local_one_is() {
    let ours = Signer::new();
    let body = b"the packed app".to_vec();
    let hash = {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &body).unwrap();
        sha256_of(tmp.path()).unwrap()
    };
    let mut web = HashMap::new();
    let r = release(
        &mut web,
        "v1",
        &manifest(10, "liv-10.zip", &hash, body.len() as u64),
        &ours,
        false,
        false,
    );
    web.insert(format!("{DL}/v1/liv-10.zip"), body.clone());
    web.insert(
        "https://api.example/repos/o/r/releases/latest".into(),
        r.to_string().into_bytes(),
    );
    let source = github(web.clone(), Track::Stable, ours.keys());
    let found = source.newest(APP).unwrap().unwrap();
    let dir = tempfile::tempdir().unwrap();
    assert!(
        stage(&found, dir.path()).is_err(),
        "a URL is downloaded before staging"
    );
    let here = livtk_update::fetch::download(&found, &Web(web.clone()), dir.path()).unwrap();
    assert_eq!(here.artifact, Artifact::File(dir.path().join("liv-10.zip")));
    verify(&here).unwrap();
    // A body changed on the way is caught by the hash the signed manifest holds.
    web.insert(format!("{DL}/v1/liv-10.zip"), b"the packed apq".to_vec());
    let bent = livtk_update::fetch::download(&found, &Web(web), dir.path()).unwrap();
    assert!(verify(&bent).is_err());
}
