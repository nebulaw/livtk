//! Who Liv is to the system, and where it keeps what it writes. Each is its own, so Liv and the
//! app in use (`livtk.app`) install side by side and never read or overwrite each other's files.

use std::path::PathBuf;

use livtk_update::paths::{Env, process_env};

/// The bundle identifier, and the app identity GPUI gives notifications.
pub const APP_ID: &str = "com.github.nebulaw.liv";
/// The name shown in the Dock, the menu bar and the window's title.
pub const NAME: &str = "Liv";
/// The name of Liv's folders.
pub const DIR: &str = "liv";

/// Every folder and file Liv writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folders {
    /// Its preferences: `$LIV_CONFIG_DIR`, else `$XDG_CONFIG_HOME/liv`, else `~/.config/liv`.
    pub config: PathBuf,
    /// What it remembers (windows, sections): `$LIV_STATE_DIR`, else the system's folder for app
    /// data (on macOS `~/Library/Application Support/liv`).
    pub state: PathBuf,
    /// Its update channel, where its releases are put: in its state folder, never livtk.app's.
    pub updates: PathBuf,
    /// Its log: in `$LIV_STATE_DIR` when that is set, else on macOS `~/Library/Logs/liv/liv.log`
    /// and elsewhere in its state folder.
    pub log: PathBuf,
}

impl Folders {
    /// Liv's folders on this machine. None without a home folder.
    pub fn here() -> Option<Folders> {
        Folders::of(&process_env)
    }

    /// Liv's folders in the environment `env`.
    pub fn of(env: Env) -> Option<Folders> {
        let config = env("LIV_CONFIG_DIR")
            .or_else(|| env("XDG_CONFIG_HOME").map(|d| d.join(DIR)))
            .or_else(|| env("HOME").map(|h| h.join(".config").join(DIR)))?;
        let moved = env("LIV_STATE_DIR");
        let state = moved
            .clone()
            .or_else(|| livtk_update::paths::data_dir_with(DIR, env))?;
        let log = match (moved, cfg!(target_os = "macos")) {
            (None, true) => env("HOME")?.join("Library/Logs").join(DIR).join("liv.log"),
            _ => state.join("liv.log"),
        };
        Some(Folders {
            updates: state.join("updates"),
            config,
            state,
            log,
        })
    }
}
