//! Who Liv is to the system, and where it keeps what it writes. Each is its own, so Liv and the
//! app in use (`livtk.app`) install side by side and never read or overwrite each other's files.
//!
//! Two layers, both pure, so tests fix every path exactly:
//! - [`Platform`]: Liv's folders by each platform's convention, from that platform's base folders
//!   (`etcetera` finds them on this machine: Apple's under the home folder, XDG's, Windows' known
//!   folders).
//! - [`Folders`]: Liv's own overrides (`LIV_CONFIG_DIR`, `LIV_STATE_DIR`) over them, in a given
//!   environment.

use std::path::{Path, PathBuf};

use etcetera::BaseStrategy;
use etcetera::app_strategy::AppStrategyArgs;
use livtk_update::paths::{Env, process_env};

/// The bundle identifier, and the app identity GPUI gives notifications. It is also the name of
/// Liv's folders on macOS, as Apple's convention has it (`identifier_is_the_platform_s_args`).
pub const APP_ID: &str = "com.github.nebulaw.liv";
/// The name shown in the Dock, the menu bar and the window's title.
pub const NAME: &str = "Liv";
/// Liv's name in folders and file names where a short one is the convention (Linux).
pub const SHORT: &str = "liv";
/// Liv's publisher: in its identifier, and its folder on Windows (`<author>\<app>`).
pub const AUTHOR: &str = "nebulaw";

/// Who Liv is, as `etcetera` names an app: its identifier is `top_level_domain.author.app_name`.
/// Liv's folders are named as etcetera's app strategies name them (`tests/it/identity.rs` holds
/// the two together).
pub fn strategy_args() -> AppStrategyArgs {
    AppStrategyArgs {
        top_level_domain: "com.github".into(),
        author: AUTHOR.into(),
        app_name: SHORT.into(),
    }
}

/// The kind of system Liv runs on, as far as its folders go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Windows,
    /// Linux and the other Unix systems, which follow XDG.
    Unix,
}

impl Os {
    /// The one Liv was built for.
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Unix
        }
    }
}

/// A machine's base folders, before any app's name: the home folder, and the system's folders for
/// settings, data, what can be made again, and state. On Windows `data` is the roaming folder
/// (`%APPDATA%`) and `cache` the local one (`%LOCALAPPDATA%`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bases {
    pub home: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub state: Option<PathBuf>,
}

impl Bases {
    /// This machine's, as etcetera finds them (XDG's on macOS and Linux, the known folders on
    /// Windows). None without a home folder.
    pub fn here() -> Option<Bases> {
        let b = etcetera::choose_base_strategy().ok()?;
        Some(Bases {
            home: b.home_dir().to_path_buf(),
            config: b.config_dir(),
            data: b.data_dir(),
            cache: b.cache_dir(),
            state: b.state_dir(),
        })
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
    /// This machine's: its base folders, as etcetera finds them, named the way this OS names an
    /// app's.
    pub fn here() -> Option<Platform> {
        Some(Platform::on(Os::current(), &Bases::here()?))
    }

    /// Liv's folders on `os`, from its base folders `b`.
    pub fn on(os: Os, b: &Bases) -> Platform {
        match os {
            Os::MacOs => Platform::apple(&b.home),
            Os::Windows => Platform::windows(&b.data, &b.cache),
            Os::Unix => Platform::xdg(&b.config, b.state.as_ref().unwrap_or(&b.data), &b.cache),
        }
    }

    /// macOS, under the home folder `home`: everything Liv keeps in
    /// `~/Library/Application Support/<identifier>` (settings included: `~/Library/Preferences` is
    /// the system's, for its defaults files), what can be made again in
    /// `~/Library/Caches/<identifier>`, logs in `~/Library/Logs/<identifier>`.
    pub fn apple(home: &Path) -> Platform {
        let library = home.join("Library");
        let support = library.join("Application Support").join(APP_ID);
        Platform {
            config: support.clone(),
            state: support,
            cache: library.join("Caches").join(APP_ID),
            logs: library.join("Logs").join(APP_ID),
        }
    }

    /// Linux and other Unix, from the XDG base folders (`$XDG_CONFIG_HOME`, `$XDG_STATE_HOME`,
    /// `$XDG_CACHE_HOME`): `liv` in each, logs in its state folder.
    pub fn xdg(config: &Path, state: &Path, cache: &Path) -> Platform {
        let state = state.join(SHORT);
        Platform {
            config: config.join(SHORT),
            logs: state.join("logs"),
            state,
            cache: cache.join(SHORT),
        }
    }

    /// Windows, from the roaming (`%APPDATA%`) and local (`%LOCALAPPDATA%`) folders: preferences
    /// roam (`nebulaw\liv\config`); what Liv remembers, its updates, logs and cache stay on this
    /// machine (`nebulaw\liv\…` in the local folder).
    pub fn windows(roaming: &Path, local: &Path) -> Platform {
        let local = local.join(AUTHOR).join(SHORT);
        Platform {
            config: roaming.join(AUTHOR).join(SHORT).join("config"),
            state: local.join("state"),
            logs: local.join("logs"),
            cache: local.join("cache"),
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
