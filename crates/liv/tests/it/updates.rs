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
    let staged = updates::check(&source, &NoNetwork, &f, 8, None)
        .unwrap()
        .unwrap();
    assert_eq!(staged.manifest.build, 9);
    assert!(staged.app.starts_with(f.cache.join("staging")));
    assert!(
        updates::check(&source, &NoNetwork, &f, 9, None)
            .unwrap()
            .is_none(),
        "already the newest"
    );
}

/// A Liv.app of `build` at `dir/name`, as far as the updater reads it.
#[cfg(target_os = "macos")]
fn liv_app(dir: &Path, name: &str, build: u64) -> PathBuf {
    let app = dir.join(name);
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{}</string>
<key>CFBundleVersion</key><string>{build}</string>
</dict></plist>"#,
            identity::APP_ID
        ),
    )
    .unwrap();
    app
}

/// A build already staged and ready is never staged again over itself: Restart to Update may
/// be handing it over.
#[cfg(target_os = "macos")]
#[test]
fn a_build_already_ready_is_kept_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let f = folders(dir.path());
    let ours = Signer::new();
    std::fs::create_dir_all(&f.updates).unwrap();
    let zip = f.updates.join("liv-9.zip");
    let app = liv_app(&dir.path().join("src"), "Liv.app", 9);
    assert!(
        std::process::Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&app)
            .arg(&zip)
            .status()
            .unwrap()
            .success()
    );
    let sha = livtk_update::stage::sha256_of(&zip).unwrap();
    let size = std::fs::metadata(&zip).unwrap().len();
    ours.publish(
        &f.updates,
        "liv-9.json",
        &manifest(9, "liv-9.zip", &sha, size),
    );
    let source = LocalFolder::trusting(&f.updates, ours.keys());
    let staged = updates::check(&source, &NoNetwork, &f, 8, None)
        .unwrap()
        .unwrap();
    let marker = staged.app.join("Contents/handed-over");
    std::fs::write(&marker, "").unwrap();

    let again = updates::check(&source, &NoNetwork, &f, 8, Some(&staged))
        .unwrap()
        .unwrap();
    assert_eq!(again, staged);
    assert!(
        marker.exists(),
        "the staged app was left as it was, not unpacked again"
    );
}

/// A source that offers one build by URL, and the web that serves it.
struct ByUrl(livtk_update::Found);

impl livtk_update::Source for ByUrl {
    fn newest(&self, _: &str) -> livtk_update::Result<Option<livtk_update::Found>> {
        Ok(Some(self.0.clone()))
    }
}

struct Serves(Vec<u8>);

impl livtk_update::fetch::Fetch for Serves {
    fn get(&self, _: &str) -> livtk_update::Result<Option<Vec<u8>>> {
        Ok(Some(self.0.clone()))
    }
}

/// A download is only the way in: once its build is staged, it is gone; and what a check left
/// is cleared at the next start.
#[cfg(target_os = "macos")]
#[test]
fn downloads_and_staged_builds_don_t_pile_up() {
    let dir = tempfile::tempdir().unwrap();
    let f = folders(dir.path());
    let app = liv_app(&dir.path().join("src"), "Liv.app", 9);
    let zip = dir.path().join("liv-9.zip");
    assert!(
        std::process::Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&app)
            .arg(&zip)
            .status()
            .unwrap()
            .success()
    );
    let body = std::fs::read(&zip).unwrap();
    let sha = livtk_update::stage::sha256_of(&zip).unwrap();
    let found = livtk_update::Found {
        manifest: manifest(9, "liv-9.zip", &sha, body.len() as u64),
        artifact: livtk_update::Artifact::Url("https://example/liv-9.zip".into()),
    };
    let staged = updates::check(&ByUrl(found), &Serves(body), &f, 8, None)
        .unwrap()
        .unwrap();
    assert!(staged.app.exists());
    let downloads = f.cache.join("downloads");
    assert_eq!(
        std::fs::read_dir(&downloads).unwrap().count(),
        0,
        "the download is gone"
    );

    updates::tidy(&f);
    assert!(
        !f.cache.join("staging").exists() && !downloads.exists(),
        "cleared at start"
    );
    updates::tidy(&f); // and nothing to clear is fine
}

/// A launcher that records what it starts.
#[derive(Default)]
struct Starts(Vec<PathBuf>);

impl livtk_update::swap::Launcher for Starts {
    fn start(&mut self, app: &Path, _: Option<&Path>) -> livtk_update::Result<()> {
        self.0.push(app.to_path_buf());
        Ok(())
    }
    fn stop(&mut self, _: &Path) {}
}

/// The pid of a process that has already exited, as the Liv that handed over has.
fn gone_pid() -> u32 {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

/// Once the old Liv has quit for an update, a Liv is left running whatever fails.
#[cfg(target_os = "macos")]
#[test]
fn a_hand_over_that_can_t_finish_starts_the_installed_liv() {
    let dir = tempfile::tempdir().unwrap();
    let installed = liv_app(&dir.path().join("Applications"), "Liv.app", 8);
    let request = |staged: PathBuf| Finish {
        pid: gone_pid(),
        installed: installed.clone(),
        staged,
        health: dir.path().join("healthy"),
        build: 9,
    };

    // The staged app is another build than the one asked for.
    let wrong = liv_app(&dir.path().join("staging"), "Liv.app", 7);
    let mut starts = Starts::default();
    assert!(request(wrong).run_with(&mut starts).is_err());
    assert_eq!(
        starts.0,
        vec![installed.clone()],
        "the installed Liv is started"
    );

    // The staged app is gone.
    let mut starts = Starts::default();
    assert!(
        request(dir.path().join("nowhere/Liv.app"))
            .run_with(&mut starts)
            .is_err()
    );
    assert_eq!(starts.0, vec![installed.clone()]);
    assert!(installed.exists(), "and left where it was");
}

#[test]
fn github_s_channels_trust_the_release_key_only() {
    for c in [Channel::Stable, Channel::Preview] {
        assert_eq!(
            updates::keys_for(c).is_empty(),
            updates::RELEASE_KEY.is_empty(),
            "no other key on {c:?}"
        );
    }
}

/// The local channel never goes to the network.
struct NoNetwork;

impl livtk_update::fetch::Fetch for NoNetwork {
    fn get(&self, url: &str) -> livtk_update::Result<Option<Vec<u8>>> {
        panic!("no network on the local channel: {url}")
    }
}
