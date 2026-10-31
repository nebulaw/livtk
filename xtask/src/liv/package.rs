//! Packing a built Liv for its updater: the bundle zipped as macOS zips apps, its manifest, and
//! the manifest's signature. Written signature first and manifest last, so a channel never names a
//! build it can't trust or an artifact that isn't all there.
//!
//! Two namings, one per kind of channel:
//! - local (`liv-<build>.json`, `liv-<build>.zip`): every build on this machine, newest kept;
//! - release (`liv-<platform>-<arch>.json`, as `livtk_update::manifest::release_name` has it, and
//!   `liv-<version>-<platform>-<arch>.zip`): one per platform among a GitHub release's assets.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use livtk_update::sign::signature_name;
use livtk_update::{Arch, FORMAT, Manifest, Platform};
use sha2::{Digest, Sha256};

use super::bundle::Built;
use super::run_ok;
use super::signer::Signer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Naming {
    Local,
    Release,
}

/// The manifest's and the artifact's names for `built` under `naming`.
pub fn names(
    naming: Naming,
    version: &str,
    build: u64,
    platform: Platform,
    arch: Arch,
) -> (String, String) {
    match naming {
        Naming::Local => (format!("liv-{build}.json"), format!("liv-{build}.zip")),
        Naming::Release => (
            livtk_update::manifest::release_name("liv", platform, arch),
            format!("liv-{version}-{}-{}.zip", platform.name(), arch.name()),
        ),
    }
}

/// Pack `built` into `dir`, signed by `signer`, with `notes`: the manifest's path.
pub fn package(
    built: &Built,
    notes: &str,
    signer: &Signer,
    dir: &Path,
    naming: Naming,
) -> Result<PathBuf> {
    let id = &built.id;
    let platform = Platform::this().context("a platform updates know")?;
    let arch = Arch::this().context("an architecture updates know")?;
    let (manifest_name, zip) = names(naming, &id.version, id.build, platform, arch);
    std::fs::create_dir_all(dir)?;
    let _ = std::fs::remove_file(dir.join(&zip));
    run_ok(
        Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&built.app)
            .arg(dir.join(&zip)),
    )?;
    let bytes = std::fs::read(dir.join(&zip))?;
    let manifest = Manifest {
        format: FORMAT,
        app: id.app_id.clone(),
        version: id.version.clone(),
        build: id.build,
        commit: id.commit.clone(),
        platform,
        arch,
        artifact: zip,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        size: bytes.len() as u64,
        notes: notes.to_owned(),
    };
    let body = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(
        dir.join(signature_name(&manifest_name)),
        signer.sign(body.as_bytes())?,
    )?;
    let path = dir.join(&manifest_name);
    std::fs::write(&path, body)?;
    Ok(path)
}

/// How many builds the local channel keeps: the newest, and a couple to go back to.
pub const KEEP: usize = 3;

/// Keep only the newest `keep` builds in a local `channel`: each older build's manifest,
/// signature and artifact go together.
pub fn prune(channel: &Path, keep: usize) -> Result<()> {
    let mut builds: Vec<u64> = std::fs::read_dir(channel)?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_prefix("liv-")?.strip_suffix(".json")?.parse().ok())
        .collect();
    builds.sort_unstable_by(|a, b| b.cmp(a));
    for build in builds.into_iter().skip(keep) {
        let (manifest, zip) = (format!("liv-{build}.json"), format!("liv-{build}.zip"));
        for name in [signature_name(&manifest), manifest, zip] {
            match std::fs::remove_file(channel.join(&name)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_channel_names_its_builds_its_own_way() {
        assert_eq!(
            names(Naming::Local, "0.5.0", 87, Platform::Macos, Arch::Aarch64),
            ("liv-87.json".to_owned(), "liv-87.zip".to_owned())
        );
        assert_eq!(
            names(Naming::Release, "0.5.0", 87, Platform::Macos, Arch::Aarch64),
            (
                "liv-macos-aarch64.json".to_owned(),
                "liv-0.5.0-macos-aarch64.zip".to_owned()
            ),
            "a release's manifest is named as the GitHub source looks for it"
        );
    }

    #[test]
    fn the_channel_keeps_its_newest_builds_and_drops_the_rest_whole() {
        let dir = tempfile::tempdir().unwrap();
        for b in [7, 9, 10, 8] {
            for ext in ["json", "json.minisig", "zip"] {
                std::fs::write(dir.path().join(format!("liv-{b}.{ext}")), "").unwrap();
            }
        }
        std::fs::write(dir.path().join("other.txt"), "").unwrap();
        prune(dir.path(), 3).unwrap();
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "liv-10.json",
                "liv-10.json.minisig",
                "liv-10.zip",
                "liv-8.json",
                "liv-8.json.minisig",
                "liv-8.zip",
                "liv-9.json",
                "liv-9.json.minisig",
                "liv-9.zip",
                "other.txt",
            ]
        );
    }
}
