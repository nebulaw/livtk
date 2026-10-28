//! Who Liv is to the system, and where it keeps what it writes. Each is its own, so Liv and the
//! app in use (`livtk.app`) install side by side and never read or overwrite each other's files.

use std::path::PathBuf;

/// The bundle identifier, and the app identity GPUI gives notifications.
pub const APP_ID: &str = "com.github.nebulaw.liv";
/// The name shown in the Dock, the menu bar and the window's title.
pub const NAME: &str = "Liv";
/// The name of Liv's folders.
pub const DIR: &str = "liv";

fn env(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Liv's preferences: `$LIV_CONFIG_DIR`, else `$XDG_CONFIG_HOME/liv`, else `~/.config/liv`.
pub fn config_dir() -> Option<PathBuf> {
    env("LIV_CONFIG_DIR")
        .or_else(|| env("XDG_CONFIG_HOME").map(|d| d.join(DIR)))
        .or_else(|| env("HOME").map(|h| h.join(".config").join(DIR)))
}

/// What Liv remembers (its windows, its sections) and its update channel: `$LIV_STATE_DIR`, else
/// the system's folder for app data (on macOS `~/Library/Application Support/liv`).
pub fn state_dir() -> Option<PathBuf> {
    env("LIV_STATE_DIR").or_else(|| livtk_update::paths::data_dir(DIR))
}

/// Where Liv's releases are put for it to update from: its own channel, never livtk.app's.
pub fn updates_dir() -> Option<PathBuf> {
    state_dir().map(|d| d.join("updates"))
}

/// Liv's log: on macOS `~/Library/Logs/liv/liv.log`, elsewhere `liv.log` in its state folder.
pub fn log_file() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Logs").join(DIR).join("liv.log"))
    } else {
        state_dir().map(|d| d.join("liv.log"))
    }
}
