//! Where updates live on this machine: the local source `scripts/release` fills, the staging
//! area, and the file a freshly updated app writes to say it started. `$LIVTK_UPDATES_DIR`
//! overrides the base, for tests and for trying a release by hand.

use std::path::PathBuf;

/// The environment the folders are worked out from: the process's ([`process_env`]), or a fixed
/// one in tests, so a test never depends on the machine it runs on.
pub type Env<'a> = &'a dyn Fn(&str) -> Option<PathBuf>;

/// The process's environment. An empty value counts as unset.
pub fn process_env(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The base: on macOS `~/Library/Application Support/livtk`, on Linux `$XDG_DATA_HOME/livtk`
/// (else `~/.local/share/livtk`), on Windows `%LOCALAPPDATA%\livtk`.
pub fn base() -> Option<PathBuf> {
    base_with(&process_env)
}

/// [`base`], in the environment `env`.
pub fn base_with(env: Env) -> Option<PathBuf> {
    env("LIVTK_UPDATES_DIR").or_else(|| data_dir_with("livtk", env))
}

/// The folder an app named `app` keeps its own data in, beside livtk's and never inside it: on
/// macOS `~/Library/Application Support/<app>`, on Linux `$XDG_DATA_HOME/<app>` (else
/// `~/.local/share/<app>`), on Windows `%LOCALAPPDATA%\<app>`.
pub fn data_dir(app: &str) -> Option<PathBuf> {
    data_dir_with(app, &process_env)
}

/// [`data_dir`], in the environment `env`.
pub fn data_dir_with(app: &str, env: Env) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support").join(app))
    } else if cfg!(target_os = "windows") {
        env("LOCALAPPDATA").map(|d| d.join(app))
    } else {
        env("XDG_DATA_HOME")
            .map(|d| d.join(app))
            .or_else(|| env("HOME").map(|h| h.join(".local/share").join(app)))
    }
}

/// The local source: manifests and their artifacts.
pub fn source() -> Option<PathBuf> {
    source_with(&process_env)
}

/// [`source`], in the environment `env`.
pub fn source_with(env: Env) -> Option<PathBuf> {
    base_with(env).map(|b| b.join("updates"))
}

/// Where a build is unpacked and verified before it is installed.
pub fn staging() -> Option<PathBuf> {
    base().map(|b| b.join("staging"))
}
