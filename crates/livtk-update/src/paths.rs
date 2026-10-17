//! Where updates live on this machine: the local source `scripts/release` fills, the staging
//! area, and the file a freshly updated app writes to say it started. `$LIVTK_UPDATES_DIR`
//! overrides the base, for tests and for trying a release by hand.

use std::path::PathBuf;

/// The base: on macOS `~/Library/Application Support/livtk`, on Linux `$XDG_DATA_HOME/livtk`
/// (else `~/.local/share/livtk`), on Windows `%LOCALAPPDATA%\livtk`.
pub fn base() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = env("LIVTK_UPDATES_DIR") {
        return Some(dir);
    }
    if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support/livtk"))
    } else if cfg!(target_os = "windows") {
        env("LOCALAPPDATA").map(|d| d.join("livtk"))
    } else {
        env("XDG_DATA_HOME")
            .map(|d| d.join("livtk"))
            .or_else(|| env("HOME").map(|h| h.join(".local/share/livtk")))
    }
}

/// The local source: manifests and their artifacts.
pub fn source() -> Option<PathBuf> {
    base().map(|b| b.join("updates"))
}

/// Where a build is unpacked and verified before it is installed.
pub fn staging() -> Option<PathBuf> {
    base().map(|b| b.join("staging"))
}
