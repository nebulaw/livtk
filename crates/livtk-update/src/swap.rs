//! The swap the helper makes once the app has quit: the installed app moved aside, the staged
//! one moved into its place, started, and given a while to say it started well. If it does not,
//! it is moved out and the old one back, and started again. Each move is a rename within one
//! folder, so the installed app is never half there.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::{Error, Result};

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

/// Make the swap. Whatever happens, one app is left running, started once: the new build, once it
/// says it started well; else the old one, put back (it is never lost) and started again,
/// whichever step failed, and never the new build unchecked.
pub fn swap(s: &Swap, launcher: &mut dyn Launcher) -> Result<Outcome> {
    let mut aside = false;
    let result = try_swap(s, launcher, &mut aside);
    if matches!(result, Ok(Outcome::Updated)) {
        return result;
    }
    // Rolled back, or a step failed. Only if this swap moved the old app aside is anything put
    // back: until then it stands where it was, untouched (a leftover from an earlier swap is
    // never taken for it).
    let old = if aside {
        put_back(s)
    } else {
        s.installed.clone()
    };
    let started = launcher.start(&old, None);
    match result {
        Ok(outcome) => started.map(|()| outcome),
        Err(e) => Err(e),
    }
}

/// The old app, moved aside by this swap, back in place: whatever stands there now (the new
/// build, or part of it) moved out first, or removed. Where the old app is to be started from:
/// in place, or, when the place can't be cleared, where it was moved aside.
fn put_back(s: &Swap) -> PathBuf {
    let previous = beside(&s.installed, "previous");
    if s.installed.exists() {
        clear_away(&s.installed);
    }
    if !s.installed.exists() && std::fs::rename(&previous, &s.installed).is_ok() {
        s.installed.clone()
    } else {
        previous
    }
}

/// Move what stands at `installed` (a failed new build) out of the way: to `.failed`, or, when
/// that spot is blocked, the next free `.failed-N`; removed only when it can't be moved at all.
/// A rename within one folder fails only when the folder itself can't be written.
fn clear_away(installed: &Path) {
    let failed = beside(installed, "failed");
    let _ = remove(&failed);
    if std::fs::rename(installed, &failed).is_ok() {
        return;
    }
    for n in 2..100 {
        let spot = beside(installed, &format!("failed-{n}"));
        if !spot.exists() && std::fs::rename(installed, &spot).is_ok() {
            return;
        }
    }
    let _ = remove(installed);
}

/// The swap's moves, up to the new build's health check; `aside` is set once the old app has
/// been moved aside. It never starts the old app: `swap` does, once, after putting it back.
fn try_swap(s: &Swap, launcher: &mut dyn Launcher, aside: &mut bool) -> Result<Outcome> {
    let previous = beside(&s.installed, "previous");
    let incoming = beside(&s.installed, "incoming");
    if !s.installed.exists() && previous.exists() {
        // An earlier swap was interrupted with the old app moved aside: it goes back first,
        // before anything else is touched. Until it is back, it counts as moved aside, so a
        // failure here still starts it, from where it stands.
        *aside = true;
        std::fs::rename(&previous, &s.installed)?;
        *aside = false;
    }
    remove(&incoming)?;
    remove(&previous)?;
    // The staged build next to the installed one first: staging may be on another volume.
    move_or_copy(&s.staged, &incoming)?;
    std::fs::rename(&s.installed, &previous)?;
    *aside = true;
    std::fs::rename(&incoming, &s.installed)?;
    remove_file(&s.health)?;
    let started_well = launcher.start(&s.installed, Some(&s.health)).is_ok()
        && started(&s.health, s.build, s.wait);
    if started_well {
        // Done: a leftover copy of the old app is only tidied, never a failure.
        let _ = remove(&previous);
        return Ok(Outcome::Updated);
    }
    // It did not start well: stopped if it runs; `swap` puts the old app back and starts it as it
    // always is (not asked to report: it is no update).
    launcher.stop(&s.installed);
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

/// Apps started and stopped as macOS does: `open`, and a signal to the app's own program.
pub struct Open {
    /// The app's program inside its bundle: `Contents/MacOS/<program>`.
    pub program: String,
    /// The variable a started app reads to know where to say it started.
    pub health_var: String,
}

impl Launcher for Open {
    fn start(&mut self, app: &Path, health: Option<&Path>) -> Result<()> {
        let mut open = Command::new("open");
        // A new instance: never just bring forward one that may still be running.
        open.arg("-n");
        if let Some(health) = health {
            let mut env = std::ffi::OsString::from(format!("{}=", self.health_var));
            env.push(health);
            open.arg("--env").arg(env);
        }
        if open.arg("-a").arg(app).status()?.success() {
            Ok(())
        } else {
            Err(Error::Failed(format!("open {}", app.display())))
        }
    }

    fn stop(&mut self, app: &Path) {
        let program = app.join("Contents/MacOS").join(&self.program);
        let _ = Command::new("pkill").arg("-f").arg(&program).status();
        // Given a moment to go before its bundle is moved.
        sleep(Duration::from_millis(500));
    }
}

/// Whether process `pid` has exited within `wait`.
pub fn exited(pid: u32, wait: Duration) -> bool {
    let until = Instant::now() + wait;
    loop {
        let alive = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .is_ok_and(|s| s.success());
        if !alive {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        sleep(Duration::from_millis(100));
    }
}
