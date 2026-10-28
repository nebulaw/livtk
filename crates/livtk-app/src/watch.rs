//! What wakes the app: the OS telling it a file it shows changed (DEC-8). Each project's log and
//! config, its runs and its docs are watched; any change wakes the app's loop, which then looks
//! at what changed as it always has. Nothing is polled while nothing changes.

use std::path::{Path, PathBuf};

use async_channel::{Receiver, Sender};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// What the app's own reading and writing touch, never news: the projection and its WAL, the
/// lock, and the app's saved state.
fn ignored(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("livtk.db") || n == "lock" || n.starts_with(".DS_Store"))
}

/// The folders watched for each project: its `.livtk/` (the log, its config), its runs, and its
/// docs. One not made yet is waited for on the nearest folder above it that is, so its making
/// wakes the app, which then watches it.
pub fn folders(root: &Path, docs: &Path) -> Vec<(PathBuf, RecursiveMode)> {
    let livtk = root.join(livtk_core::DIR_NAME);
    let mut out: Vec<(PathBuf, RecursiveMode)> = Vec::new();
    for (p, mode) in [
        (livtk.clone(), RecursiveMode::NonRecursive),
        (livtk.join(livtk_core::run::RUNS), RecursiveMode::Recursive),
        (docs.to_path_buf(), RecursiveMode::Recursive),
    ] {
        let at = if p.is_dir() {
            (p, mode)
        } else if let Some(up) = p.ancestors().skip(1).find(|a| a.is_dir()) {
            (up.to_path_buf(), RecursiveMode::NonRecursive)
        } else {
            continue;
        };
        if !out.iter().any(|(o, _)| *o == at.0) {
            out.push(at);
        }
    }
    out
}

/// Whether the OS tells of changes in `folder`: a network or other non-local volume may take
/// the watch and never say a word.
#[cfg(target_os = "macos")]
fn local(folder: &Path) -> bool {
    // MNT_LOCAL, from <sys/mount.h>.
    const MNT_LOCAL: u32 = 0x0000_1000;
    rustix::fs::statfs(folder).map_or(true, |fs| fs.f_flags & MNT_LOCAL != 0)
}

#[cfg(not(target_os = "macos"))]
fn local(_: &Path) -> bool {
    true
}

/// The watches, and the channel they wake the app on: one wake however many files changed, as
/// the channel holds one.
pub struct Watch {
    watcher: Option<RecommendedWatcher>,
    watched: Vec<(PathBuf, RecursiveMode)>,
    /// The folders whose watch failed, tried again on each wake.
    failed: Vec<PathBuf>,
    /// A folder is on a non-local volume (a network share), which may take the watch and never
    /// say a word.
    remote: bool,
    /// Kept so the channel stays open with no watcher: a closed one would answer every wait at
    /// once, and the app's loop would spin.
    _wake: Sender<()>,
}

impl Watch {
    /// No folder watched yet; the receiver wakes when one changes. Not `on` (under GPUI's test
    /// scheduler, which allows no thread of its own), nothing is watched and the app looks on
    /// its old beat.
    pub fn new(on: bool) -> (Watch, Receiver<()>) {
        let (tx, rx) = async_channel::bounded::<()>(1);
        let watcher = if on { Self::watcher(tx.clone()) } else { None };
        (
            Watch {
                watcher,
                watched: Vec::new(),
                failed: Vec::new(),
                remote: false,
                _wake: tx,
            },
            rx,
        )
    }

    fn watcher(wake: Sender<()>) -> Option<RecommendedWatcher> {
        let made = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            if !event.paths.is_empty() && event.paths.iter().all(|p| ignored(p)) {
                return;
            }
            // Full means a wake is already waiting: this one is the same news.
            let _ = wake.try_send(());
        });
        match made {
            Ok(w) => Some(w),
            Err(e) => {
                log::warn!("watch: cannot watch files, the app will look on its old beat: {e}");
                None
            }
        }
    }

    /// Watch exactly `folders`: new ones are added, gone ones dropped, failed ones tried again.
    /// Cheap when unchanged.
    pub fn follow(&mut self, folders: Vec<(PathBuf, RecursiveMode)>) {
        if folders == self.watched && self.failed.is_empty() {
            return;
        }
        let Some(w) = self.watcher.as_mut() else {
            return;
        };
        for (p, _) in self.watched.iter().filter(|f| !folders.contains(f)) {
            let _ = w.unwatch(p);
        }
        let new = folders
            .iter()
            .filter(|f| !self.watched.contains(f) || self.failed.contains(&f.0));
        let mut failed = Vec::new();
        for (p, mode) in new {
            if let Err(e) = w.watch(p, *mode) {
                if !self.failed.contains(p) {
                    log::warn!("watch: {}: {e}; looking on the old beat", p.display());
                }
                failed.push(p.clone());
            }
        }
        self.failed = failed;
        self.remote = folders.iter().any(|(p, _)| !local(p));
        self.watched = folders;
    }

    /// Whether the OS tells it of every change; if not, the app looks on its old beat.
    pub fn working(&self) -> bool {
        self.watcher.is_some() && self.failed.is_empty() && !self.remote
    }
}
