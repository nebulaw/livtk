//! Where the app runs, and how its command line tools are installed. On macOS only an app bundle
//! may post notifications, and one bundle holds the app and the CLI: `~/Applications/livtk.app`,
//! with `livtk` and `livtk-app` on PATH as links into it, so one update covers them all.
//! `livtk-app` started through such a link opens the bundle; started inside it, it runs; anything
//! else (a development build, a copy) runs as itself. GPU-free, so tested as plain Rust.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the bundle is installed, under the home directory.
pub const IN_HOME: &str = "Applications/livtk.app";
/// Where the command line tools' links go, under the home directory.
pub const BIN_IN_HOME: &str = ".local/bin";
/// The programs a bundle holds, each linked onto PATH.
pub const TOOLS: [&str; 2] = ["livtk", "livtk-app"];
/// Where a bundle keeps them.
const MACOS: &str = "Contents/MacOS";

/// The folder a handed-off bundle treats as where it was started: the bundle itself starts
/// elsewhere, and this one is opened without being registered, as from a shell.
pub const STARTED_IN: &str = "LIVTK_APP_STARTED_IN";

/// How a `livtk-app` starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// Through a link into this bundle, from a shell: open the bundle and step aside.
    Open(PathBuf),
    /// Inside a bundle, as macOS starts it: run, with notifications.
    Bundled,
    /// Anything else: run as itself, without notifications on macOS.
    Bare,
}

/// Whether `exe` is inside an app bundle: `….app/Contents/MacOS/<exe>`.
pub fn in_bundle(exe: &Path) -> bool {
    bundle_of(exe).is_some()
}

/// The app bundle `exe` is inside, if any.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.ends_with("MacOS")
        && contents.ends_with("Contents")
        && app.extension().is_some_and(|e| e == "app"))
    .then(|| app.to_path_buf())
}

/// How the `livtk-app` started as `invoked`, which is `resolved` once links are followed, runs.
pub fn start(invoked: &Path, resolved: &Path) -> Start {
    if in_bundle(invoked) {
        Start::Bundled
    } else if let Some(bundle) = bundle_of(resolved) {
        Start::Open(bundle)
    } else {
        Start::Bare
    }
}

/// What of the environment the bundle needs to see the same livtk: where it keeps its config,
/// its models and the like. The bundle starts from the system, not from this shell.
pub fn passed_on(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Vec<OsString> {
    let mut out: Vec<OsString> = vars
        .into_iter()
        .filter(|(k, _)| {
            let k = k.to_string_lossy();
            (k.starts_with("LIVTK_") || k.starts_with("XDG_")) && k != STARTED_IN
        })
        .map(|(k, v)| {
            let mut kv = k;
            kv.push("=");
            kv.push(v);
            kv
        })
        .collect();
    out.sort();
    out
}

/// Whether a bundle of the app is running already: then `open` only brings it forward, and the
/// projects given to it are not opened.
pub fn bundle_running() -> bool {
    Command::new("pgrep")
        .arg("-f")
        .arg(format!("livtk.app/{MACOS}/livtk-app"))
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Open `bundle` with `projects` (absolute paths), the folder it was `started_in` and the
/// environment `env` (`NAME=VALUE`). Whether macOS took it.
pub fn open_bundle(
    bundle: &Path,
    projects: &[PathBuf],
    started_in: Option<&Path>,
    env: &[OsString],
) -> bool {
    let mut open = Command::new("open");
    for kv in env {
        open.arg("--env").arg(kv);
    }
    if let Some(dir) = started_in {
        let mut kv = OsString::from(STARTED_IN);
        kv.push("=");
        kv.push(dir);
        open.arg("--env").arg(kv);
    }
    open.arg("-a")
        .arg(bundle)
        .arg("--args")
        .args(projects)
        .status()
        .is_ok_and(|s| s.success())
}

/// What installing the command line tools did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Installed {
    /// The links made (or made again).
    pub linked: Vec<PathBuf>,
    /// Files in the way that are not links, left as they are.
    pub refused: Vec<PathBuf>,
    /// Whether the links' folder is on PATH.
    pub on_path: bool,
    /// Other copies of the tools that come earlier on PATH, and so would run instead.
    pub shadowed: Vec<PathBuf>,
    /// Other copies later on PATH: never run, left over from an earlier install (cargo's, say).
    pub leftover: Vec<PathBuf>,
}

/// Link each of the `bundle`'s tools into `bin`, replacing a link already there (so it is safe to
/// run again) but never a file, and see how `path` (a PATH value) finds them. A bundle without
/// one of its tools is refused, rather than linked to nothing.
#[cfg(unix)]
pub fn install_cli(bundle: &Path, bin: &Path, path: &OsStr) -> io::Result<Installed> {
    let targets: Vec<PathBuf> = TOOLS.iter().map(|t| bundle.join(MACOS).join(t)).collect();
    if let Some(missing) = targets.iter().find(|t| !t.is_file()) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} has no {}", bundle.display(), missing.display()),
        ));
    }
    std::fs::create_dir_all(bin)?;
    let mut done = Installed::default();
    for (tool, target) in TOOLS.iter().zip(&targets) {
        let link = bin.join(tool);
        match std::fs::symlink_metadata(&link) {
            Ok(m) if m.file_type().is_symlink() => std::fs::remove_file(&link)?,
            Ok(_) => {
                done.refused.push(link);
                continue;
            }
            Err(_) => {}
        }
        std::os::unix::fs::symlink(target, &link)?;
        done.linked.push(link);
    }
    // Folders as they really are (a linked ~/.local/bin is still itself), and no empty entries,
    // which a shell reads as the current folder.
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let bin_real = real(bin);
    let dirs: Vec<PathBuf> = std::env::split_paths(path)
        .filter(|d| !d.as_os_str().is_empty())
        .collect();
    let at = dirs.iter().position(|d| real(d) == bin_real);
    done.on_path = at.is_some();
    for (i, dir) in dirs.iter().enumerate() {
        if Some(i) == at || real(dir) == bin_real {
            continue;
        }
        for (tool, target) in TOOLS.iter().zip(&targets) {
            let other = dir.join(tool);
            if other.exists() && real(&other) != real(target) {
                if at.is_none_or(|a| i < a) {
                    done.shadowed.push(other);
                } else {
                    done.leftover.push(other);
                }
            }
        }
    }
    Ok(done)
}

/// Linking the tools is for macOS and Linux for now; Windows installs its own way.
#[cfg(not(unix))]
pub fn install_cli(_bundle: &Path, _bin: &Path, _path: &OsStr) -> io::Result<Installed> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the command line tools are installed with livtk's installer on this system",
    ))
}
