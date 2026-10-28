//! Liv updating itself: its settings, its channel, what it trusts, how it hands over to a staged
//! build, and the notes it shows once. Offline, in temporary folders.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use liv::identity::{self, Folders, Platform};
use liv::settings::{Channel, Settings, path_in};
use liv::updates::{self, Finish};
use livtk_update::sign::{Keys, signature_name};
use livtk_update::source::LocalFolder;

/// Liv's folders, all under `root`.
fn folders(root: &Path) -> Folders {
    Folders::of(
        &|_: &str| None::<PathBuf>,
        Platform {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
            logs: root.join("logs"),
        },
    )
}

#[test]
fn settings_are_the_defaults_until_set_and_kept_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = path_in(dir.path());
    assert_eq!(Settings::load(&path), Settings::default(), "no file");
    std::fs::write(&path, "updates = [not toml").unwrap();
    assert_eq!(
        Settings::load(&path),
        Settings::default(),
        "a file that can't be read"
    );

    let mut s = Settings::default();
    s.updates.channel = Some(Channel::Preview);
    s.save(&path).unwrap();
    assert_eq!(Settings::load(&path), s);
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("channel = \"preview\"")
    );
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        names,
        vec![OsString::from("settings.toml")],
        "no temporary file left"
    );
}

#[test]
fn the_channel_is_the_settings_else_the_build_s_default() {
    assert_eq!(
        updates::channel(&Settings::default()),
        updates::default_channel()
    );
    let mut s = Settings::default();
    s.updates.channel = Some(Channel::Local);
    assert_eq!(updates::channel(&s), Channel::Local);
    for (name, c) in [
        ("stable", Channel::Stable),
        ("preview", Channel::Preview),
        ("local", Channel::Local),
    ] {
        assert_eq!(Channel::parse(name), Some(c));
    }
    assert_eq!(Channel::parse("nightly"), None);
}

#[test]
fn releases_come_from_the_workspace_s_repository() {
    assert_eq!(updates::repo(), "nebulaw/tasks");
}

/// A key pair, and a way to sign a manifest with it.
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

    fn keys(&self) -> Keys {
        Keys::new([self.public.as_str()]).unwrap()
    }

    /// Write manifest `m` as `name` in `dir`, signed.
    fn publish(&self, dir: &Path, name: &str, m: &livtk_update::Manifest) {
        std::fs::create_dir_all(dir).unwrap();
        let body = serde_json::to_string(m).unwrap();
        let sig = minisign::sign(
            None,
            &self.secret,
            std::io::Cursor::new(body.as_bytes()),
            None,
            None,
        )
        .unwrap()
        .into_string();
        std::fs::write(dir.join(signature_name(name)), sig).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }
}

fn manifest(build: u64, artifact: &str, sha256: &str, size: u64) -> livtk_update::Manifest {
    livtk_update::Manifest {
        format: livtk_update::FORMAT,
        app: identity::APP_ID.into(),
        version: "0.4.0".into(),
        build,
        commit: "abc1234".into(),
        platform: livtk_update::Platform::this().unwrap(),
        arch: livtk_update::Arch::this().unwrap(),
        artifact: artifact.into(),
        sha256: sha256.into(),
        size,
        notes: "What changed.".into(),
    }
}

const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[test]
fn the_local_channel_offers_only_what_a_trusted_key_signed() {
    let dir = tempfile::tempdir().unwrap();
    let f = folders(dir.path());
    let ours = Signer::new();
    let theirs = Signer::new();
    ours.publish(&f.updates, "liv-5.json", &manifest(5, "liv-5.zip", HASH, 1));
    theirs.publish(&f.updates, "liv-6.json", &manifest(6, "liv-6.zip", HASH, 1));

    let source = updates::source(Channel::Local, &f, ours.keys());
    assert_eq!(
        source
            .newest(identity::APP_ID)
            .unwrap()
            .unwrap()
            .manifest
            .build,
        5
    );
    // Trusting no key at all, nothing is offered.
    let none = updates::source(Channel::Local, &f, Keys::default());
    assert!(none.newest(identity::APP_ID).unwrap().is_none());
}

#[test]
fn a_hand_over_is_asked_and_read_back_whole() {
    let f = Finish {
        pid: 42,
        installed: "/Applications/Liv.app".into(),
        staged: "/c/staging/build-7/Liv.app".into(),
        health: "/s/healthy".into(),
        build: 7,
    };
    assert_eq!(Finish::parse(&f.args()), Some(f.clone()));
    assert_eq!(
        Finish::parse(&[OsString::from("--version")]),
        None,
        "not a hand-over"
    );
    let mut cut = f.args();
    cut.truncate(5);
    assert_eq!(Finish::parse(&cut), None, "missing what it needs");
}

#[test]
fn an_update_installs_only_into_an_app_bundle() {
    assert_eq!(
        updates::bundle_of(Path::new("/Applications/Liv.app/Contents/MacOS/liv")),
        Some(PathBuf::from("/Applications/Liv.app"))
    );
    assert_eq!(
        updates::bundle_of(Path::new("/repo/target/quick/liv")),
        None
    );
    assert_eq!(
        updates::bundle_of(Path::new("/x/Contents/MacOS/liv")),
        None,
        "no .app"
    );
}

#[test]
fn the_notes_of_an_update_show_once_and_only_for_its_build() {
    let dir = tempfile::tempdir().unwrap();
    let f = folders(dir.path());
    std::fs::create_dir_all(&f.state).unwrap();
    std::fs::write(f.state.join("updated.txt"), "7\nWhat changed.").unwrap();
    assert_eq!(
        updates::notes_after_update(&f, 7),
        Some("What changed.".into())
    );
    assert_eq!(updates::notes_after_update(&f, 7), None, "once");
    std::fs::write(f.state.join("updated.txt"), "7\nWhat changed.").unwrap();
    assert_eq!(updates::notes_after_update(&f, 8), None, "another build's");
    assert!(!f.state.join("updated.txt").exists(), "and gone");
}

/// From a signed release on the local channel to a staged build, as an installed Liv checks.
#[cfg(target_os = "macos")]
#[test]
fn a_newer_signed_build_on_the_channel_is_found_checked_and_staged() {
    use std::process::Command;
    let dir = tempfile::tempdir().unwrap();
    let f = folders(dir.path());
    let ours = Signer::new();
    // A Liv.app of build 9, zipped as a release zips it.
    let app = dir.path().join("src/Liv.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{}</string>
<key>CFBundleVersion</key><string>9</string>
</dict></plist>"#,
            identity::APP_ID
        ),
    )
    .unwrap();
    std::fs::create_dir_all(&f.updates).unwrap();
    let zip = f.updates.join("liv-9.zip");
    assert!(
        Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&app)
            .arg(&zip)
            .status()
            .unwrap()
            .success()
    );
    let size = std::fs::metadata(&zip).unwrap().len();
    let sha = livtk_update::stage::sha256_of(&zip).unwrap();
    ours.publish(
        &f.updates,
        "liv-9.json",
        &manifest(9, "liv-9.zip", &sha, size),
    );

    let source = LocalFolder::trusting(&f.updates, ours.keys());
    let staged = updates::check(&source, &NoNetwork, &f, 8).unwrap().unwrap();
    assert_eq!(staged.manifest.build, 9);
    assert!(staged.app.starts_with(f.cache.join("staging")));
    assert!(
        updates::check(&source, &NoNetwork, &f, 9)
            .unwrap()
            .is_none(),
        "already the newest"
    );
}

/// The local channel never goes to the network.
struct NoNetwork;

impl livtk_update::fetch::Fetch for NoNetwork {
    fn get(&self, url: &str) -> livtk_update::Result<Option<Vec<u8>>> {
        panic!("no network on the local channel: {url}")
    }
}
