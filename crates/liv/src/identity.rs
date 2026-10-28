//! Who Liv is to the system, and where it keeps what it writes. Each is its own, so Liv and the
//! app in use (`livtk.app`) install side by side and never read or overwrite each other's files.
//!
//! Two layers:
//! - [`Platform`]: Liv's folders by each platform's own convention, worked out by `etcetera`.
//! - [`Folders`]: Liv's own overrides (`LIV_CONFIG_DIR`, `LIV_STATE_DIR`) over them, in a given
//!   environment, so tests never depend on the machine.

use std::path::PathBuf;

use etcetera::AppStrategy;
use etcetera::app_strategy::{AppStrategyArgs, Apple, Windows, Xdg};
use livtk_update::paths::{Env, process_env};

/// The bundle identifier, and the app identity GPUI gives notifications. It is also the name of
/// Liv's folders on macOS, as Apple's convention has it (`identifier_is_the_platform_s_args`).
pub const APP_ID: &str = "com.github.nebulaw.liv";
/// The name shown in the Dock, the menu bar and the window's title.
pub const NAME: &str = "Liv";
/// Liv's name in folders and file names where a short one is the convention (Linux).
pub const SHORT: &str = "liv";

/// Who Liv is, as `etcetera` names an app: its identifier is `top_level_domain.author.app_name`.
pub fn strategy_args() -> AppStrategyArgs {
    AppStrategyArgs {
        top_level_domain: "com.github".into(),
        author: "nebulaw".into(),
        app_name: SHORT.into(),
    }
}

/// Liv's folders by the platform's convention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Platform {
    /// Preferences.
    pub config: PathBuf,
    /// What Liv remembers (windows, sections) and its update channel.
    pub state: PathBuf,
    /// What can be made again.
    pub cache: PathBuf,
    /// The log, and crash reports beside it.
    pub logs: PathBuf,
}

impl Platform {
    /// This machine's.
    pub fn here() -> Option<Platform> {
        let args = strategy_args();
        if cfg!(target_os = "macos") {
            Apple::new(args).ok().map(|s| Platform::apple(&s))
        } else if cfg!(target_os = "windows") {
            Windows::new(args).ok().map(|s| Platform::windows(&s))
        } else {
            Xdg::new(args).ok().map(|s| Platform::xdg(&s))
        }
    }

    /// macOS: everything Liv keeps in `~/Library/Application Support/<identifier>` (settings
    /// included: `~/Library/Preferences` is the system's, for its defaults files), what can be
    /// made again in `~/Library/Caches/<identifier>`, logs in `~/Library/Logs/<identifier>`.
    pub fn apple(s: &Apple) -> Platform {
        let data = s.data_dir();
        Platform {
            config: data.clone(),
            state: data,
            cache: s.cache_dir(),
            logs: s.home_dir().join("Library/Logs").join(APP_ID),
        }
    }

    /// Linux and other Unix: XDG, `~/.config/liv`, `~/.local/state/liv` (logs in it), `~/.cache/liv`.
    pub fn xdg(s: &Xdg) -> Platform {
        let state = s.state_dir().unwrap_or_else(|| s.data_dir());
        Platform {
            config: s.config_dir(),
            logs: state.join("logs"),
            state,
            cache: s.cache_dir(),
        }
    }

    /// Windows: preferences roam (`%APPDATA%\nebulaw\liv\config`); what Liv remembers, its
    /// updates and logs stay on this machine, beside its cache in `%LOCALAPPDATA%\nebulaw\liv`.
    pub fn windows(s: &Windows) -> Platform {
        let cache = s.cache_dir();
        let local = cache.parent().map_or_else(|| cache.clone(), PathBuf::from);
        Platform {
            config: s.config_dir(),
            state: local.join("state"),
            logs: local.join("logs"),
            cache,
        }
    }
}

/// Every folder and file Liv writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folders {
    /// Its preferences: `$LIV_CONFIG_DIR`, else the platform's.
    pub config: PathBuf,
    /// What it remembers: `$LIV_STATE_DIR`, else the platform's.
    pub state: PathBuf,
    /// What can be made again: in `$LIV_STATE_DIR` when that is set, else the platform's.
    pub cache: PathBuf,
    /// Its update channel, where its releases are put: in its state folder, never livtk.app's.
    pub updates: PathBuf,
    /// Its log: in `$LIV_STATE_DIR` when that is set, else in the platform's logs folder.
    pub log: PathBuf,
}

impl Folders {
    /// Liv's folders on this machine. None without a home folder.
    pub fn here() -> Option<Folders> {
        Some(Folders::of(&process_env, Platform::here()?))
    }

    /// Liv's folders in the environment `env`, over the platform's `base`.
    pub fn of(env: Env, base: Platform) -> Folders {
        let config = env("LIV_CONFIG_DIR").unwrap_or(base.config);
        let (state, cache, logs) = match env("LIV_STATE_DIR") {
            Some(state) => (state.clone(), state.join("cache"), state),
            None => (base.state, base.cache, base.logs),
        };
        Folders {
            updates: state.join("updates"),
            log: logs.join("liv.log"),
            config,
            state,
            cache,
        }
    }
}
