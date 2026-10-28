//! The manifest: one JSON file describing one build of the app for one platform. Its `format`
//! is the contract's version: an app reads format 1 and skips any newer one rather than
//! misreading it; fields it does not know are ignored, so a newer manifest never breaks an older
//! app. Builds are ordered by their build number, never by file times.

use serde::{Deserialize, Serialize};

/// The manifest format this app reads and writes.
pub const FORMAT: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

impl Platform {
    /// Its name, as manifests write it.
    pub fn name(self) -> &'static str {
        match self {
            Platform::Macos => "macos",
            Platform::Windows => "windows",
            Platform::Linux => "linux",
        }
    }

    /// The platform this program was built for.
    pub fn this() -> Option<Platform> {
        if cfg!(target_os = "macos") {
            Some(Platform::Macos)
        } else if cfg!(target_os = "windows") {
            Some(Platform::Windows)
        } else if cfg!(target_os = "linux") {
            Some(Platform::Linux)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    Aarch64,
    X86_64,
}

impl Arch {
    /// Its name, as manifests write it.
    pub fn name(self) -> &'static str {
        match self {
            Arch::Aarch64 => "aarch64",
            Arch::X86_64 => "x86_64",
        }
    }

    /// The architecture this program was built for.
    pub fn this() -> Option<Arch> {
        if cfg!(target_arch = "aarch64") {
            Some(Arch::Aarch64)
        } else if cfg!(target_arch = "x86_64") {
            Some(Arch::X86_64)
        } else {
            None
        }
    }
}

/// The name of an app's manifest for `platform` and `arch` among a release's assets, where one
/// release holds every platform's: `<prefix>-<platform>-<arch>.json` (`liv-macos-aarch64.json`).
/// Its signature is beside it (`sign::signature_name`).
pub fn release_name(prefix: &str, platform: Platform, arch: Arch) -> String {
    format!("{prefix}-{}-{}.json", platform.name(), arch.name())
}

/// One build of the app, as an update describes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// The app's bundle identifier, so an update for another app is never taken.
    pub app: String,
    pub version: String,
    pub build: u64,
    pub commit: String,
    pub platform: Platform,
    pub arch: Arch,
    /// The packed app: its file name, beside the manifest (in a folder, or among a release's
    /// assets).
    pub artifact: String,
    /// The artifact's SHA-256, lowercase hex, and its size in bytes.
    pub sha256: String,
    pub size: u64,
    /// What changed, shown once after the update.
    #[serde(default)]
    pub notes: String,
}

impl Manifest {
    /// Read a manifest: `Ok(None)` for a newer format this app does not read, an error for one
    /// that is not a manifest at all.
    pub fn parse(json: &str) -> std::result::Result<Option<Manifest>, String> {
        #[derive(Deserialize)]
        struct Head {
            format: u32,
        }
        let head: Head = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if head.format != FORMAT {
            return Ok(None);
        }
        let m: Manifest = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if m.sha256.len() != 64 || !m.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("sha256 is not 64 hex digits".into());
        }
        let path = std::path::Path::new(&m.artifact);
        if m.artifact.is_empty() || path.is_absolute() || m.artifact.contains("..") {
            return Err("artifact is not a plain path beside the manifest".into());
        }
        Ok(Some(m))
    }

    /// Whether this build is for `app` on this platform and architecture.
    pub fn fits(&self, app: &str) -> bool {
        self.app == app
            && Some(self.platform) == Platform::this()
            && Some(self.arch) == Arch::this()
    }
}
