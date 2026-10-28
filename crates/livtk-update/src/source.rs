//! Where updates come from. A [`Source`] answers one question: the newest build for this app
//! here, if any. [`LocalFolder`] is a folder of manifests and their artifacts (a release made on
//! this machine); `GitHubReleases` reads a repository's releases. A source
//! that trusts keys offers only manifests one of them signed.

use std::path::{Path, PathBuf};

use crate::sign::{Keys, signature_name};
use crate::{Manifest, Result};

/// Where a build's packed app is: a file here, or a URL it is downloaded from
/// (`fetch::download`) before staging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Artifact {
    File(PathBuf),
    Url(String),
}

/// A build found: its manifest and where its artifact is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub manifest: Manifest,
    pub artifact: Artifact,
}

pub trait Source {
    /// The newest build of `app` for this platform, if any.
    fn newest(&self, app: &str) -> Result<Option<Found>>;
}

/// The newest build of `app` in `source`, if it is newer than `this_build`.
pub fn newer(source: &dyn Source, app: &str, this_build: u64) -> Result<Option<Found>> {
    Ok(source
        .newest(app)?
        .filter(|f| f.manifest.build > this_build))
}

/// A folder of `*.json` manifests, each beside its artifact (and, when keys are trusted, its
/// signature).
pub struct LocalFolder {
    pub dir: PathBuf,
    /// Trusted keys: with them, a manifest is offered only when one of them signed it.
    keys: Option<Keys>,
}

impl LocalFolder {
    /// Every manifest in `dir`, signed or not: livtk.app's own channel, on its own machine.
    pub fn new(dir: impl Into<PathBuf>) -> LocalFolder {
        LocalFolder {
            dir: dir.into(),
            keys: None,
        }
    }

    /// Only the manifests in `dir` that one of `keys` signed.
    pub fn trusting(dir: impl Into<PathBuf>, keys: Keys) -> LocalFolder {
        LocalFolder {
            dir: dir.into(),
            keys: Some(keys),
        }
    }
}

impl Source for LocalFolder {
    fn newest(&self, app: &str) -> Result<Option<Found>> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut best: Option<Found> = None;
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            // A manifest that cannot be read, or isn't signed by a trusted key, is passed over:
            // one bad file never hides the rest.
            let Some(m) = self.read(&path) else { continue };
            if !m.fits(app) || best.as_ref().is_some_and(|b| b.manifest.build >= m.build) {
                continue;
            }
            let artifact = Artifact::File(beside(&path, &m.artifact));
            best = Some(Found {
                manifest: m,
                artifact,
            });
        }
        Ok(best)
    }
}

impl LocalFolder {
    fn read(&self, path: &Path) -> Option<Manifest> {
        let bytes = std::fs::read(path).ok()?;
        if let Some(keys) = &self.keys {
            let name = path.file_name()?.to_str()?;
            let sig = std::fs::read_to_string(beside(path, &signature_name(name))).ok()?;
            keys.verify(&bytes, &sig).ok()?;
        }
        Manifest::parse(std::str::from_utf8(&bytes).ok()?)
            .ok()
            .flatten()
    }
}

fn beside(manifest: &Path, name: &str) -> PathBuf {
    manifest
        .parent()
        .map_or_else(|| PathBuf::from(name), |d| d.join(name))
}
