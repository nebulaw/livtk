//! Releases of Liv on GitHub (TAS-140): signed with the release key, built by the release
//! workflow (`.github/workflows/release.yml`) from a version tag, found by Liv's stable and
//! preview channels.
//!
//!   cargo xtask keygen                  make the release key: its public half into
//!                                       `crates/liv/release.pub` (committed, built into Liv), its
//!                                       secret half into `.liv-dev/release.key` (never committed),
//!                                       for the repository's `LIV_SIGNING_KEY` secret
//!   cargo xtask release VERSION         set the workspace's version, commit, tag `vVERSION` and push:
//!                                       the release workflow does the rest. A version with a
//!                                       pre-release part (`0.5.0-pre.1`) is a preview release
//!   cargo xtask release-assets DIR [NOTES]
//!                                       the release workflow's step: a release build of Liv, packed
//!                                       and signed (`$LIV_SIGNING_KEY`) into DIR, named as a
//!                                       release's assets are

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::liv::bundle::bundle;
use crate::liv::package::{Naming, package};
use crate::liv::signer::{Signer, make};
use crate::liv::{output, root, run_ok, target_dir};

/// Where Liv's built-in release key lives.
pub const PUBLIC_KEY: &str = "crates/liv/release.pub";
/// The release workflow's secret holding the release key's secret half.
pub const SECRET_VAR: &str = "LIV_SIGNING_KEY";

pub fn keygen(args: &[String]) -> Result<()> {
    if !args.is_empty() {
        bail!("usage: cargo xtask keygen");
    }
    let root = root();
    let public = root.join(PUBLIC_KEY);
    if std::fs::read_to_string(&public).is_ok_and(|k| !k.trim().is_empty()) {
        bail!(
            "{PUBLIC_KEY} holds a release key already: making another would leave every installed \
             Liv unable to trust new releases"
        );
    }
    let secret = root.join(".liv-dev/release.key");
    make(
        &public,
        &secret,
        "Liv's release key: the release workflow only",
    )?;
    println!("made Liv's release key:");
    println!("  public: {PUBLIC_KEY} (commit it: Liv builds it in)");
    println!("  secret: {} (never commit it)", secret.display());
    println!("next:");
    println!("  gh secret set {SECRET_VAR} < {}", secret.display());
    println!("  then keep a copy somewhere safe (a password manager) and delete the file");
    Ok(())
}

pub fn assets(args: &[String]) -> Result<()> {
    let (Some(out), notes) = (args.first(), args.get(1)) else {
        bail!("usage: cargo xtask release-assets DIR [NOTES]");
    };
    let root = root();
    let target = target_dir(&root)?;
    let signer = Signer::from_env(SECRET_VAR, &root.join(PUBLIC_KEY))?;
    // A release trusts the release key only (built in from `release.pub`), and follows the
    // stable or preview channel: no machine's key, no local channel.
    let built = bundle(&root, &target, "release", &[])?;
    if built.id.modified {
        bail!(
            "a release is built from a clean checkout: {}",
            built.describe
        );
    }
    let manifest = package(
        &built,
        notes.map_or("", String::as_str),
        &signer,
        Path::new(out),
        Naming::Release,
    )?;
    println!("packed {} into {}", built.describe, manifest.display());
    Ok(())
}

pub fn release(args: &[String]) -> Result<()> {
    let [version] = args else {
        bail!("usage: cargo xtask release VERSION (e.g. 0.5.0, or 0.5.0-pre.1 for a preview)");
    };
    if !valid_version(version) {
        bail!("{version} isn't a version: MAJOR.MINOR.PATCH, optionally -PRE (0.5.0-pre.1)");
    }
    let root = root();
    if std::fs::read_to_string(root.join(PUBLIC_KEY)).is_ok_and(|k| k.trim().is_empty()) {
        bail!("no release key yet: cargo xtask keygen first");
    }
    // Only the version moves in a release commit: anything else staged or changed first.
    let dirty = output(Command::new("git").current_dir(&root).args([
        "status",
        "--porcelain",
        "--untracked-files=no",
    ]))?;
    let others: Vec<&str> = dirty
        .lines()
        .filter(|l| !l.ends_with(".livtk/events.jsonl") && !l.ends_with(".livtk/config.toml"))
        .collect();
    if !others.is_empty() {
        bail!("commit or put aside these first:\n{}", others.join("\n"));
    }
    let manifest = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)?;
    let set = set_version(&text, version)?;
    std::fs::write(&manifest, set)?;
    run_ok(Command::new("cargo").current_dir(&root).args([
        "update",
        "--workspace",
        "--offline",
        "-q",
    ]))?;
    let tag = format!("v{version}");
    run_ok(
        Command::new("git")
            .current_dir(&root)
            .args(["commit", "-q", "-m"])
            .arg(format!("release {version}"))
            .args(["Cargo.toml", "Cargo.lock"]),
    )?;
    run_ok(Command::new("git").current_dir(&root).args(["tag", &tag]))?;
    run_ok(
        Command::new("git")
            .current_dir(&root)
            .args(["push", "origin", "HEAD", &tag]),
    )?;
    println!("pushed {tag}: the release workflow builds, signs and publishes it");
    Ok(())
}

/// `MAJOR.MINOR.PATCH`, with an optional `-PRE` of letters, digits, dots and dashes.
fn valid_version(v: &str) -> bool {
    let (core, pre) = v.split_once('-').map_or((v, None), |(c, p)| (c, Some(p)));
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && pre.is_none_or(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        })
}

/// The workspace manifest `text` with `[workspace.package] version` set to `version`.
fn set_version(text: &str, version: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("the workspace's Cargo.toml")?;
    let package = doc["workspace"]["package"]
        .as_table_mut()
        .context("[workspace.package]")?;
    package["version"] = toml_edit::value(version);
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_three_numbers_with_an_optional_pre_release() {
        for ok in ["0.5.0", "1.0.0", "0.5.0-pre.1", "2.3.4-rc-2"] {
            assert!(valid_version(ok), "{ok}");
        }
        for bad in ["0.5", "v0.5.0", "0.5.0-", "0.5.x", "0.5.0-pre 1", ""] {
            assert!(!valid_version(bad), "{bad}");
        }
    }

    #[test]
    fn a_release_sets_only_the_workspace_s_version() {
        let text = "[workspace]\nmembers = []\n\n[workspace.package]\n# kept\nversion = \"0.4.0\"\nedition = \"2024\"\n";
        let set = set_version(text, "0.5.0").unwrap();
        assert!(set.contains("version = \"0.5.0\""));
        assert!(
            set.contains("# kept") && set.contains("edition = \"2024\""),
            "{set}"
        );
    }
}
