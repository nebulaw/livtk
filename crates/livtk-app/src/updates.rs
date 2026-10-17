//! The app's side of updates (see `livtk_update`): look for a newer build and stage it, start
//! the helper that installs it once the app has quit, and, started by that helper, say that it
//! started well. GPU-free; the UI decides when to look and offers "Restart to Update".

use std::path::{Path, PathBuf};
use std::process::Command;

use livtk_update::stage::{Staged, stage};
use livtk_update::{LocalFolder, Source, paths};

/// Where the helper asks a freshly installed app to write its build number once it started.
pub const HEALTH_VAR: &str = "LIVTK_UPDATE_HEALTH";

/// Look for a build of `app` newer than `this_build`, and stage it: `None` when this is the
/// newest. Slow (it hashes and unpacks), so run off the main thread.
pub fn check(app: &str, this_build: u64) -> livtk_update::Result<Option<Staged>> {
    let (Some(source), Some(staging)) = (paths::source(), paths::staging()) else {
        return Ok(None);
    };
    match LocalFolder::new(source).newest(app)? {
        Some(found) if found.manifest.build > this_build => stage(&found, &staging).map(Some),
        _ => Ok(None),
    }
}

/// Start the helper that installs `staged` over the bundle `exe` runs from, once this process
/// has exited. The app quits right after.
pub fn install(staged: &Staged, exe: &Path) -> Result<(), String> {
    let bundle = crate::launch::bundle_of(exe)
        .ok_or("updates install into livtk.app: this livtk-app runs outside it")?;
    let helper = exe.with_file_name("livtk-update");
    if !helper.is_file() {
        return Err(format!("{} has no livtk-update", bundle.display()));
    }
    let health = paths::base()
        .ok_or("no folder for updates on this system")?
        .join("healthy");
    Command::new(helper)
        .arg("--pid")
        .arg(std::process::id().to_string())
        .arg("--installed")
        .arg(&bundle)
        .arg("--staged")
        .arg(&staged.app)
        .arg("--health")
        .arg(&health)
        .arg("--build")
        .arg(staged.manifest.build.to_string())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start livtk-update: {e}"))
}

/// Started by the helper after an update: write this build's number where it asked, so it
/// keeps this build. Whether this start followed an update.
pub fn report_started(build: u64) -> bool {
    let Some(path) = std::env::var_os(HEALTH_VAR).map(PathBuf::from) else {
        return false;
    };
    std::fs::write(path, build.to_string()).is_ok()
}

/// What changed in build `build` of `app`, from its manifest in the local source.
pub fn notes_of(app: &str, build: u64) -> Option<String> {
    let found = LocalFolder::new(paths::source()?).newest(app).ok()??;
    (found.manifest.build == build && !found.manifest.notes.trim().is_empty())
        .then_some(found.manifest.notes)
}
