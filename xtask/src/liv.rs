//! `liv`: Liv as an app bundle, beside livtk.app and never in its place (proposal 0003 §5).
//!
//!   cargo xtask liv                     bundle Liv
//!   cargo xtask liv --install           then move Liv.app to ~/Applications
//!   cargo xtask liv --publish [NOTES]   then put it in Liv's local update channel, where an
//!                                       installed Liv finds it (Check for Updates)
//!   --profile release                   build with the release profile (default: quick, release
//!                                       speed in seconds, for the build-and-try loop)
//!
//! Builds made here trust this machine's own signing key, made once in `.liv-dev/` at the
//! repository's root (never committed), and follow the local channel; releases made here are
//! signed with it. Releases on GitHub are signed with the release key instead (TAS-140).
//!
//! The bundle is made by `cargo bundle` from `[package.metadata.bundle]` in `crates/liv`: no
//! Info.plist or file copying of ours. What cargo-bundle leaves to us is here: the ad-hoc
//! signature (until Developer ID signing, TAS-143), installing, and publishing. Liv's identity
//! and channel are read from the built `liv --identity`, so they are written in one place.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use livtk_update::{Arch, FORMAT, Manifest, Platform};
use sha2::{Digest, Sha256};

pub fn run(args: &[String]) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("an app bundle is for macOS; elsewhere run `cargo run -p liv`");
    }
    let usage = "usage: cargo xtask liv [--install] [--publish [NOTES]] [--profile quick|release]";
    let mut install = false;
    let mut publish_notes: Option<String> = None;
    let mut profile = "quick".to_owned();
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--install" => install = true,
            "--publish" => {
                // Notes are the next word unless it is the next option.
                let notes = it.next_if(|n| !n.starts_with("--")).cloned();
                publish_notes = Some(notes.unwrap_or_default());
            }
            "--profile" => match it.next().map(String::as_str) {
                Some(p @ ("quick" | "release")) => profile = p.to_owned(),
                _ => bail!("{usage}"),
            },
            _ => bail!("{usage}"),
        }
    }
    let profile = profile.as_str();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let target = target_dir(&root)?;
    // Built here, locked to Cargo.lock; cargo-bundle then bundles that very binary.
    let key = DevKey::here(&root)?;
    run_ok(
        Command::new("cargo")
            .current_dir(&root)
            .args(["build", "-q", "--locked", "--profile", profile, "-p", "liv"])
            // What a build made here trusts, and where it looks for updates.
            .env("LIV_DEV_KEY", &key.public)
            .env("LIV_DEFAULT_CHANNEL", "local"),
    )?;
    let built = target.join(profile).join("liv");
    let identity = output(Command::new(&built).arg("--version"))?;
    let mut id = Identity::parse(&identity)?;
    id.who(&output(Command::new(&built).arg("--identity"))?)?;
    // From the crate's folder: cargo-bundle reads the metadata's paths (the icon) from where it
    // runs, and skips what it can't find without a word.
    run_ok(
        Command::new("cargo")
            .current_dir(root.join("crates/liv"))
            .args([
                "bundle",
                "--package",
                "liv",
                "--profile",
                profile,
                "--format",
                "osx",
                "--binary-path",
            ])
            .arg(&built),
    )?;
    let app = target.join(profile).join("bundle/osx/Liv.app");
    let plist = app.join("Contents/Info.plist");
    if !std::fs::read_to_string(&plist)?.contains("<key>CFBundleIconFile</key>") {
        bail!(
            "{} has no icon: check `icon` in crates/liv's bundle metadata",
            app.display()
        );
    }
    // cargo-bundle stamps CFBundleVersion with the time; the updater orders and checks builds
    // by their number, so the bundle carries the binary's.
    run_ok(
        Command::new("plutil")
            .args(["-replace", "CFBundleVersion", "-string"])
            .arg(id.build.to_string())
            .arg(&plist),
    )?;
    // The bundle as the updater will read it: what it would refuse is never signed or published.
    let (app_id, build) = livtk_update::stage::identity(&app)?;
    if app_id != id.app_id || build != id.build {
        bail!(
            "the bundle says {app_id} build {build}, the binary {} build {}",
            id.app_id,
            id.build
        );
    }
    // Signed ad hoc, as macOS asks of every app on Apple silicon; nothing leaves this machine.
    run_ok(
        Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(&app),
    )?;
    println!("built {}: {identity}", app.display());

    if let Some(notes) = publish_notes {
        publish(&app, &id, notes, &key)?;
    }
    if install {
        let to = PathBuf::from(std::env::var("HOME").context("a home folder")?)
            .join("Applications/Liv.app");
        let _ = std::fs::remove_dir_all(&to);
        std::fs::create_dir_all(to.parent().context("a parent")?)?;
        // Moved, not copied: two bundles with one identifier leave macOS to pick either.
        std::fs::rename(&app, &to)?;
        println!("installed {}", to.display());
    }
    Ok(())
}

/// The build's identity, from `liv --version`: "liv 0.4.0 (build 87, abc1234)", with
/// ", modified" when its sources had changes.
struct Identity {
    version: String,
    build: u64,
    commit: String,
    modified: bool,
    /// From `liv --identity`: who Liv is to the system, and its update channel.
    app_id: String,
    name: String,
    updates: Option<PathBuf>,
}

impl Identity {
    fn parse(s: &str) -> Result<Identity> {
        let rest = s
            .strip_prefix("liv ")
            .context("`liv --version` names Liv")?;
        let (version, paren) = rest.split_once(" (").context("a build in parentheses")?;
        let inner = paren.trim_end_matches(')');
        let mut parts = inner.split(", ");
        let build = parts
            .next()
            .and_then(|b| b.strip_prefix("build "))
            .and_then(|b| b.parse().ok())
            .context("a build number")?;
        let commit = parts.next().context("a commit")?.to_owned();
        Ok(Identity {
            version: version.to_owned(),
            build,
            commit,
            modified: parts.any(|p| p == "modified"),
            app_id: String::new(),
            name: String::new(),
            updates: None,
        })
    }

    /// Take who Liv is from `liv --identity`: `key=value`, a line each.
    fn who(&mut self, said: &str) -> Result<()> {
        for line in said.lines() {
            match line.split_once('=') {
                Some(("app_id", v)) => self.app_id = v.to_owned(),
                Some(("name", v)) => self.name = v.to_owned(),
                Some(("updates", v)) => self.updates = Some(PathBuf::from(v)),
                _ => {}
            }
        }
        if self.app_id.is_empty() || self.name.is_empty() {
            bail!("`liv --identity` names no app id or name: {said}");
        }
        Ok(())
    }
}

/// Put `app` in Liv's update channel: the bundle zipped as macOS zips apps, and its manifest
/// beside it, last, so the channel never names an artifact that is not all there.
fn publish(app: &Path, id: &Identity, notes: String, key: &DevKey) -> Result<()> {
    if id.modified {
        eprintln!(
            "liv: the sources have uncommitted changes; build {} is HEAD's number",
            id.build
        );
    }
    let channel = id.updates.clone().context("Liv's update channel")?;
    std::fs::create_dir_all(&channel)?;
    let zip = format!("liv-{}.zip", id.build);
    let _ = std::fs::remove_file(channel.join(&zip));
    run_ok(
        Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(app)
            .arg(channel.join(&zip)),
    )?;
    let bytes = std::fs::read(channel.join(&zip))?;
    let manifest = Manifest {
        format: FORMAT,
        app: id.app_id.clone(),
        version: id.version.clone(),
        build: id.build,
        commit: id.commit.clone(),
        platform: Platform::this().context("a platform updates know")?,
        arch: Arch::this().context("an architecture updates know")?,
        artifact: zip,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        size: bytes.len() as u64,
        notes,
    };
    let name = format!("liv-{}.json", id.build);
    let body = serde_json::to_string_pretty(&manifest)?;
    // The signature first, then the manifest: a channel never names a build it can't trust.
    std::fs::write(
        channel.join(livtk_update::sign::signature_name(&name)),
        key.sign(body.as_bytes())?,
    )?;
    std::fs::write(channel.join(&name), body)?;
    println!(
        "published Liv build {} ({}) to {}",
        id.build,
        id.commit,
        channel.display()
    );
    Ok(())
}

/// This machine's own signing key for Liv's local releases: made once, in `.liv-dev/` at the
/// repository's root, which git ignores. Its public key is built into the builds made here.
struct DevKey {
    public: String,
    secret: minisign::SecretKey,
}

impl DevKey {
    fn here(root: &Path) -> Result<DevKey> {
        let dir = root.join(".liv-dev");
        let (public_path, secret_path) = (dir.join("dev.pub"), dir.join("dev.key"));
        if !secret_path.exists() {
            std::fs::create_dir_all(&dir)?;
            let kp = minisign::KeyPair::generate_unencrypted_keypair()
                .map_err(|e| anyhow::anyhow!("making Liv's local signing key: {e}"))?;
            let secret = kp
                .sk
                .to_box(Some("Liv's local signing key: this machine only"))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            std::fs::write(&public_path, kp.pk.to_base64())?;
            // Readable by its owner only, from the moment it exists.
            let mut file = std::fs::OpenOptions::new();
            file.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut file, 0o600);
            std::io::Write::write_all(
                &mut file.open(&secret_path)?,
                secret.to_string().as_bytes(),
            )?;
            println!("made Liv's local signing key in {}", dir.display());
        }
        let secret = minisign::SecretKeyBox::from_string(&std::fs::read_to_string(&secret_path)?)
            .and_then(|b| b.into_unencrypted_secret_key())
            .map_err(|e| anyhow::anyhow!("{}: {e}", secret_path.display()))?;
        Ok(DevKey {
            public: std::fs::read_to_string(&public_path)?.trim().to_owned(),
            secret,
        })
    }

    /// A signature of `bytes`, as a `.minisig` file holds it.
    fn sign(&self, bytes: &[u8]) -> Result<String> {
        Ok(
            minisign::sign(None, &self.secret, std::io::Cursor::new(bytes), None, None)
                .map_err(|e| anyhow::anyhow!("signing: {e}"))?
                .into_string(),
        )
    }
}

/// Where cargo builds, as cargo itself says, whatever sets it.
fn target_dir(root: &Path) -> Result<PathBuf> {
    let meta = output(Command::new("cargo").current_dir(root).args([
        "metadata",
        "--no-deps",
        "--format-version",
        "1",
    ]))?;
    let v: serde_json::Value = serde_json::from_str(&meta)?;
    Ok(PathBuf::from(
        v["target_directory"]
            .as_str()
            .context("cargo's target folder")?,
    ))
}

fn run_ok(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("running {cmd:?}"))?;
    if !status.success() {
        bail!("{cmd:?} failed: {status}");
    }
    Ok(())
}

fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    if !out.status.success() {
        bail!("{cmd:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_line_gives_the_build_its_commit_and_whether_it_was_modified() {
        let mut id = Identity::parse("liv 0.4.0 (build 87, abc1234)").unwrap();
        assert_eq!(
            (id.version.as_str(), id.build, id.commit.as_str()),
            ("0.4.0", 87, "abc1234")
        );
        assert!(!id.modified);
        assert!(
            Identity::parse("liv 0.4.0 (build 88, def5678, modified)")
                .unwrap()
                .modified
        );
        assert!(
            Identity::parse("livtk-app 0.4.0 (build 1, a)").is_err(),
            "only Liv's"
        );

        // Who it is, and its channel, come from the binary.
        assert!(id.who("updates=/x").is_err(), "an id and a name are needed");
        id.who("app_id=com.example.liv\nname=Liv\nupdates=/s/liv/updates")
            .unwrap();
        assert_eq!(id.app_id, "com.example.liv");
        assert_eq!(id.updates.as_deref(), Some(Path::new("/s/liv/updates")));
    }

    /// A build published to a channel stages as the updater stages it: its signature checked,
    /// found, verified, unpacked, and its identity matching its manifest. Run after `--publish`, with `LIV_CHANNEL`
    /// naming the channel: `LIV_CHANNEL=… cargo test -p xtask -- --ignored a_published_build`.
    #[test]
    #[ignore = "needs a published build: LIV_CHANNEL"]
    fn a_published_build_stages() {
        use livtk_update::Source;
        let channel = PathBuf::from(std::env::var_os("LIV_CHANNEL").expect("LIV_CHANNEL"));
        // Liv's identifier, from its bundle metadata (held equal to its code by crates/liv's tests).
        let manifest = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/liv/Cargo.toml"),
        )
        .unwrap();
        let doc: toml::Table = toml::from_str(&manifest).unwrap();
        let app = doc["package"]["metadata"]["bundle"]["identifier"]
            .as_str()
            .unwrap();
        let public = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.liv-dev/dev.pub"),
        )
        .expect("this machine's signing key: cargo xtask liv makes it");
        let keys = livtk_update::sign::Keys::new([public.as_str()]).unwrap();
        let found = livtk_update::LocalFolder::trusting(&channel, keys)
            .newest(app)
            .unwrap()
            .expect("a build in the channel");
        let staging = tempfile::tempdir().unwrap();
        let staged = livtk_update::stage::stage(&found, staging.path()).unwrap();
        assert!(staged.app.join("Contents/MacOS/liv").exists());
    }
}
