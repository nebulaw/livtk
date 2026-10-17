//! Where updates come from. A [`Source`] answers one question: the newest build for this app
//! here, if any. [`LocalFolder`] is a folder of manifests and their artifacts, filled by
//! `scripts/release`; a feed over HTTPS will answer the same question later.

use std::path::{Path, PathBuf};

use crate::{Manifest, Result};

/// A build found: its manifest and where its artifact is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub manifest: Manifest,
    pub artifact: PathBuf,
}

pub trait Source {
    /// The newest build of `app` for this platform, if any.
    fn newest(&self, app: &str) -> Result<Option<Found>>;
}

/// A folder of `*.json` manifests, each beside its artifact.
pub struct LocalFolder {
    pub dir: PathBuf,
}

impl LocalFolder {
    pub fn new(dir: impl Into<PathBuf>) -> LocalFolder {
        LocalFolder { dir: dir.into() }
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
            // A manifest that cannot be read is passed over: one bad file never hides the rest.
            let Some(m) = read(&path) else { continue };
            if !m.fits(app) || best.as_ref().is_some_and(|b| b.manifest.build >= m.build) {
                continue;
            }
            let artifact = beside(&path, &m.artifact);
            best = Some(Found {
                manifest: m,
                artifact,
            });
        }
        Ok(best)
    }
}

fn read(path: &Path) -> Option<Manifest> {
    let text = std::fs::read_to_string(path).ok()?;
    Manifest::parse(&text).ok().flatten()
}

fn beside(manifest: &Path, artifact: &str) -> PathBuf {
    manifest
        .parent()
        .map_or_else(|| PathBuf::from(artifact), |d| d.join(artifact))
}
