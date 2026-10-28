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
/// docs.
pub fn folders(root: &Path, docs: &Path) -> Vec<(PathBuf, RecursiveMode)> {
    let livtk = root.join(livtk_core::DIR_NAME);
    [
        (livtk.clone(), RecursiveMode::NonRecursive),
        (livtk.join(livtk_core::run::RUNS), RecursiveMode::Recursive),
        (docs.to_path_buf(), RecursiveMode::Recursive),
    ]
    .into_iter()
    .filter(|(p, _)| p.is_dir())
    .collect()
}

/// The watches, and the channel they wake the app on: one wake however many files changed, as
/// the channel holds one.
pub struct Watch {
    watcher: Option<RecommendedWatcher>,
    watched: Vec<(PathBuf, RecursiveMode)>,
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
                log::warn!("watch: cannot watch files, the app will look every minute: {e}");
                None
            }
        }
    }

    /// Watch exactly `folders`: new ones are added, gone ones dropped. Cheap when unchanged.
    pub fn follow(&mut self, folders: Vec<(PathBuf, RecursiveMode)>) {
        if folders == self.watched {
            return;
        }
        let Some(w) = self.watcher.as_mut() else {
            return;
        };
        for (p, _) in self.watched.iter().filter(|f| !folders.contains(f)) {
            let _ = w.unwatch(p);
        }
        for (p, mode) in folders.iter().filter(|f| !self.watched.contains(f)) {
            if let Err(e) = w.watch(p, *mode) {
                log::warn!("watch: {}: {e}", p.display());
            }
        }
        self.watched = folders;
    }

    /// Whether the OS tells it of changes at all; if not, the app looks every minute.
    pub fn working(&self) -> bool {
        self.watcher.is_some()
    }
}
