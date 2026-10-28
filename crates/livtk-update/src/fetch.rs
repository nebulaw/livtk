//! Reading from the network, behind one small trait so every source and the download are tested
//! offline with a fake. The real one (`Http`, the `http` feature) is a blocking HTTPS client,
//! run off the app's main thread like the rest of updating.

use std::path::Path;

use crate::source::{Artifact, Found};
use crate::{Error, Result};

pub trait Fetch: Send + Sync {
    /// The body at `url`; `None` when there is nothing there (404).
    fn get(&self, url: &str) -> Result<Option<Vec<u8>>>;
}

/// `found` with its artifact here: downloaded into `dir` when it is a URL, as it is when it is a
/// file. Nothing is trusted yet: staging checks its size and hash against the signed manifest.
pub fn download(found: &Found, fetch: &dyn Fetch, dir: &Path) -> Result<Found> {
    let url = match &found.artifact {
        Artifact::File(_) => return Ok(found.clone()),
        Artifact::Url(url) => url,
    };
    let body = fetch
        .get(url)?
        .ok_or_else(|| Error::Failed(format!("{url}: not found")))?;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(&found.manifest.artifact);
    std::fs::write(&path, body)?;
    Ok(Found {
        manifest: found.manifest.clone(),
        artifact: Artifact::File(path),
    })
}

/// HTTPS, blocking: one agent for every request, GitHub's `User-Agent` rule met.
#[cfg(feature = "http")]
pub struct Http {
    agent: ureq::Agent,
    user_agent: String,
}

#[cfg(feature = "http")]
impl Http {
    /// A client that names itself `user_agent` (an app's name and version), as GitHub asks.
    pub fn new(user_agent: impl Into<String>) -> Http {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            .http_status_as_error(false)
            .build()
            .into();
        Http {
            agent,
            user_agent: user_agent.into(),
        }
    }
}

#[cfg(feature = "http")]
impl Fetch for Http {
    fn get(&self, url: &str) -> Result<Option<Vec<u8>>> {
        let mut res = self
            .agent
            .get(url)
            .header("User-Agent", &self.user_agent)
            .header(
                "Accept",
                "application/vnd.github+json, application/octet-stream",
            )
            .call()
            .map_err(|e| Error::Failed(format!("{url}: {e}")))?;
        match res.status().as_u16() {
            200 => res
                .body_mut()
                .with_config()
                .limit(512 * 1024 * 1024)
                .read_to_vec()
                .map(Some)
                .map_err(|e| Error::Failed(format!("{url}: {e}"))),
            404 => Ok(None),
            code => Err(Error::Failed(format!("{url}: HTTP {code}"))),
        }
    }
}
