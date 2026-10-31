//! `liv`: Liv as an app bundle, for the build-and-try loop on this machine (proposal 0003 §5).
//!
//!   cargo xtask liv                     bundle Liv
//!   cargo xtask liv --install           then move Liv.app to ~/Applications
//!   cargo xtask liv --publish [NOTES]   then put it in Liv's local update channel, where an
//!                                       installed Liv finds it (Check for Updates)
//!   --profile release                   build with the release profile (default: quick, release
//!                                       speed in seconds)
//!
//! Builds made here trust this machine's own signing key as well as the release key, follow the
//! local channel, and are signed with the machine's key when published. Releases on GitHub are
//! made by `cargo xtask release` (see `release.rs`).

pub mod bundle;
pub mod package;
pub mod signer;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use bundle::bundle;
use package::{KEEP, Naming, package, prune};
use signer::Signer;

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
    let root = root();
    let target = target_dir(&root)?;
    let key = Signer::dev(&root)?;
    // What a build made here trusts, and where it looks for updates.
    let built = bundle(
        &root,
        &target,
        &profile,
        &[
            ("LIV_DEV_KEY", key.public.as_str()),
            ("LIV_DEFAULT_CHANNEL", "local"),
        ],
    )?;
    println!("built {}: {}", built.app.display(), built.describe);

    if let Some(notes) = publish_notes {
        if built.id.modified {
            eprintln!(
                "liv: the sources have uncommitted changes; build {} is HEAD's number",
                built.id.build
            );
        }
        let channel = built.id.updates.clone().context("Liv's update channel")?;
        package(&built, &notes, &key, &channel, Naming::Local)?;
        prune(&channel, KEEP)?;
        println!(
            "published Liv build {} ({}) to {}",
            built.id.build,
            built.id.commit,
            channel.display()
        );
    }
    if install {
        let to = PathBuf::from(std::env::var("HOME").context("a home folder")?)
            .join("Applications/Liv.app");
        let _ = std::fs::remove_dir_all(&to);
        std::fs::create_dir_all(to.parent().context("a parent")?)?;
        // Moved, not copied: two bundles with one identifier leave macOS to pick either.
        std::fs::rename(&built.app, &to)?;
        println!("installed {}", to.display());
    }
    Ok(())
}

/// The repository's root.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Where cargo builds, as cargo itself says, whatever sets it.
pub fn target_dir(root: &Path) -> Result<PathBuf> {
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

pub fn run_ok(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("running {cmd:?}"))?;
    if !status.success() {
        bail!("{cmd:?} failed: {status}");
    }
    Ok(())
}

pub fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    if !out.status.success() {
        bail!("{cmd:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A build published to a channel stages as the updater stages it: its signature checked,
    /// found, verified, unpacked, and its identity matching its manifest. Run after `--publish`,
    /// with `LIV_CHANNEL` naming the channel and `LIV_CHANNEL_KEY` the public key it was signed
    /// with (default: this machine's): `cargo test -p xtask -- --ignored a_published_build`.
    #[test]
    #[ignore = "needs a published build: LIV_CHANNEL"]
    fn a_published_build_stages() {
        use livtk_update::Source;
        let channel = PathBuf::from(std::env::var_os("LIV_CHANNEL").expect("LIV_CHANNEL"));
        let public = match std::env::var("LIV_CHANNEL_KEY") {
            Ok(k) => k,
            Err(_) => std::fs::read_to_string(root().join(".liv-dev/dev.pub"))
                .expect("this machine's signing key: cargo xtask liv makes it"),
        };
        let keys = livtk_update::sign::Keys::new([public.as_str()]).unwrap();
        // Liv's identifier, from its bundle metadata (held equal to its code by crates/liv's tests).
        let manifest = std::fs::read_to_string(root().join("crates/liv/Cargo.toml")).unwrap();
        let doc: toml::Table = toml::from_str(&manifest).unwrap();
        let app = doc["package"]["metadata"]["bundle"]["identifier"]
            .as_str()
            .unwrap();
        let found = livtk_update::LocalFolder::trusting(&channel, keys)
            .newest(app)
            .unwrap()
            .expect("a build in the channel");
        let staging = tempfile::tempdir().unwrap();
        let staged = livtk_update::stage::stage(&found, staging.path()).unwrap();
        assert!(staged.app.join("Contents/MacOS/liv").exists());
    }
}
