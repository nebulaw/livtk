//! The swap the helper makes once the app has quit: the installed app moved aside, the staged
//! one moved into its place, started, and given a while to say it started well. If it does not,
//! it is moved out and the old one back, and started again. Each move is a rename within one
//! folder, so the installed app is never half there.

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::Result;

/// One swap: the app installed now, the staged build to put in its place, the file the new app
/// writes its build number to once it has started, that build, and how long to wait for it.
#[derive(Clone, Debug)]
pub struct Swap {
    pub installed: PathBuf,
    pub staged: PathBuf,
    pub health: PathBuf,
    pub build: u64,
    pub wait: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The new build runs.
    Updated,
    /// The new build did not start well; the old one is back and started.
    RolledBack,
}

/// A sibling of the installed app, so a rename never crosses volumes: `.<name>-<what>`.
fn beside(installed: &Path, what: &str) -> PathBuf {
    let name = installed
        .file_name()
        .map_or_else(|| "app".into(), |n| n.to_string_lossy().into_owned());
    installed.with_file_name(format!(".{name}-{what}"))
}

/// What the swap needs of the system: start an app (asking it, when `health` is given, to say
/// there that it started), and stop an app that runs.
pub trait Launcher {
    fn start(&mut self, app: &Path, health: Option<&Path>) -> Result<()>;
    fn stop(&mut self, app: &Path);
}

/// Make the swap. Once the installed app has been moved aside, any failure (the new build will
/// not start, or does not say it started in time) puts it back and starts it again: the old app
/// is never lost.
pub fn swap(s: &Swap, launcher: &mut dyn Launcher) -> Result<Outcome> {
    let previous = beside(&s.installed, "previous");
    let incoming = beside(&s.installed, "incoming");
    remove(&incoming)?;
    if !s.installed.exists() && previous.exists() {
        // An earlier swap was interrupted with the old app moved aside: it goes back first.
        std::fs::rename(&previous, &s.installed)?;
    }
    remove(&previous)?;
    // The staged build next to the installed one first: staging may be on another volume.
    move_or_copy(&s.staged, &incoming)?;
    std::fs::rename(&s.installed, &previous)?;
    if let Err(e) = std::fs::rename(&incoming, &s.installed) {
        std::fs::rename(&previous, &s.installed)?;
        return Err(e.into());
    }
    remove_file(&s.health)?;
    let started_well = launcher.start(&s.installed, Some(&s.health)).is_ok()
        && started(&s.health, s.build, s.wait);
    if started_well {
        remove(&previous)?;
        return Ok(Outcome::Updated);
    }
    // It did not start well: stopped if it runs, moved out, the old app back and started as
    // it always is (not asked to report: it is no update).
    launcher.stop(&s.installed);
    let failed = beside(&s.installed, "failed");
    remove(&failed)?;
    std::fs::rename(&s.installed, &failed)?;
    std::fs::rename(&previous, &s.installed)?;
    launcher.start(&s.installed, None)?;
    Ok(Outcome::RolledBack)
}

/// Whether the new app wrote `build` to `health` within `wait`.
fn started(health: &Path, build: u64, wait: Duration) -> bool {
    let until = Instant::now() + wait;
    loop {
        if std::fs::read_to_string(health).is_ok_and(|s| s.trim() == build.to_string()) {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        sleep(Duration::from_millis(100));
    }
}

fn move_or_copy(from: &Path, to: &Path) -> Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        // Across volumes a rename cannot be made: copy, then the rename above is local.
        Err(_) => copy_dir(from, to),
    }
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir(&src, &dst)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(&src)?, &dst)?;
            #[cfg(not(unix))]
            return Err(crate::Error::Unsupported("links inside an app"));
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn remove(dir: &Path) -> Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

fn remove_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}
