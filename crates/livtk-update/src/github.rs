//! Updates from a GitHub repository's releases: no server of our own, no token for a public
//! repository. One release holds every platform's build; each platform's manifest is an asset
//! named `<prefix>-<platform>-<arch>.json` (`manifest::release_name`), its signature beside it,
//! and the artifact it names.
//!
//! - Stable: the latest release (GitHub's "latest" is never a prerelease or a draft).
//! - Preview: the newest release that isn't a draft, prerelease or not.

use serde::Deserialize;

use crate::fetch::Fetch;
use crate::manifest::release_name;
use crate::sign::{Keys, signature_name};
use crate::source::{Artifact, Found, Source};
use crate::{Arch, Error, Manifest, Platform, Result};

/// Which releases a source follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    Stable,
    Preview,
}

/// A repository's releases, as a source.
pub struct GitHubReleases {
    /// `owner/name`.
    pub repo: String,
    pub track: Track,
    /// The app's manifests' prefix (`liv`).
    pub prefix: String,
    pub keys: Keys,
    pub fetch: Box<dyn Fetch>,
    /// The API's address: GitHub's, or a test's.
    pub api: String,
}

impl GitHubReleases {
    pub fn new(
        repo: impl Into<String>,
        track: Track,
        prefix: impl Into<String>,
        keys: Keys,
        fetch: Box<dyn Fetch>,
    ) -> GitHubReleases {
        GitHubReleases {
            repo: repo.into(),
            track,
            prefix: prefix.into(),
            keys,
            fetch,
            api: "https://api.github.com".into(),
        }
    }
}

#[derive(Deserialize)]
struct Release {
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

impl Source for GitHubReleases {
    fn newest(&self, app: &str) -> Result<Option<Found>> {
        let (Some(platform), Some(arch)) = (Platform::this(), Arch::this()) else {
            return Ok(None);
        };
        let name = release_name(&self.prefix, platform, arch);
        let releases: Vec<Release> = match self.track {
            Track::Stable => {
                let url = format!("{}/repos/{}/releases/latest", self.api, self.repo);
                match self.fetch.get(&url)? {
                    Some(body) => vec![json(&url, &body)?],
                    None => Vec::new(),
                }
            }
            Track::Preview => {
                let url = format!("{}/repos/{}/releases?per_page=30", self.api, self.repo);
                match self.fetch.get(&url)? {
                    Some(body) => json(&url, &body)?,
                    None => Vec::new(),
                }
            }
        };
        // Newest first, as GitHub lists them: the first with this platform's manifest.
        let Some(release) = releases
            .into_iter()
            .filter(|r| !r.draft)
            .find(|r| r.assets.iter().any(|a| a.name == name))
        else {
            return Ok(None);
        };
        let url_of = |n: &str| {
            release
                .assets
                .iter()
                .find(|a| a.name == n)
                .map(|a| a.browser_download_url.clone())
        };
        let manifest_url = url_of(&name).unwrap_or_default();
        let bytes = self
            .fetch
            .get(&manifest_url)?
            .ok_or_else(|| Error::Failed(format!("{manifest_url}: not found")))?;
        let sig_url = url_of(&signature_name(&name))
            .ok_or_else(|| Error::Signature(format!("{name} has no signature beside it")))?;
        let sig = self
            .fetch
            .get(&sig_url)?
            .ok_or_else(|| Error::Signature(format!("{sig_url}: not found")))?;
        self.keys.verify(&bytes, &String::from_utf8_lossy(&sig))?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Error::Failed(format!("{manifest_url}: not text")))?;
        let manifest = match Manifest::parse(text) {
            Ok(Some(m)) => m,
            // A newer format than this app reads: offered to a newer app, not this one.
            Ok(None) => return Ok(None),
            Err(why) => return Err(Error::Failed(format!("{manifest_url}: {why}"))),
        };
        if !manifest.fits(app) {
            return Ok(None);
        }
        let artifact = url_of(&manifest.artifact).ok_or_else(|| {
            Error::Mismatch(format!(
                "the release has no {} for its manifest",
                manifest.artifact
            ))
        })?;
        Ok(Some(Found {
            manifest,
            artifact: Artifact::Url(artifact),
        }))
    }
}

fn json<T: for<'de> Deserialize<'de>>(url: &str, body: &[u8]) -> Result<T> {
    serde_json::from_slice(body).map_err(|e| Error::Failed(format!("{url}: {e}")))
}
