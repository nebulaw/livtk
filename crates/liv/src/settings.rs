//! What a person sets in Liv, in `settings.toml` in Liv's config folder. Read leniently (a file
//! that can't be read is the defaults, said in the log, never a crash), written whole and
//! atomically (a temporary file renamed over the old one), so it is never half written.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The settings file in config folder `dir`.
pub fn path_in(dir: &Path) -> PathBuf {
    dir.join("settings.toml")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub updates: Updates,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Updates {
    /// Which builds Liv updates to; unset, the build's own default (`updates::default_channel`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<Channel>,
}

/// Where updates come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// GitHub's latest release.
    Stable,
    /// The newest release, prereleases included.
    Preview,
    /// Builds released on this machine (`cargo xtask liv --release`).
    Local,
}

impl Channel {
    pub fn parse(s: &str) -> Option<Channel> {
        match s.trim() {
            "stable" => Some(Channel::Stable),
            "preview" => Some(Channel::Preview),
            "local" => Some(Channel::Local),
            _ => None,
        }
    }
}

impl Settings {
    /// The settings at `path`: the defaults when there is no file, or one that can't be read.
    pub fn load(path: &Path) -> Settings {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                log::warn!("{}: not read, the defaults are used: {e}", path.display());
                Settings::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            Err(e) => {
                log::warn!("{}: not read, the defaults are used: {e}", path.display());
                Settings::default()
            }
        }
    }

    /// Write the settings to `path`, whole: to a temporary file beside it, then renamed over it.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}
