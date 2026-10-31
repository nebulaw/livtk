//! The app's log: what GPUI and the app tell of themselves, information and up, to a file, as a
//! GUI app has no terminal to say it in. On macOS `~/Library/Logs/livtk/livtk-app.log`, elsewhere
//! `app.log` in livtk's config directory. Past a megabyte at start, it moves aside to `.old`
//! (one generation kept) and a new one begins. Every line is one append, whole, so two processes
//! writing at once (a handoff) never tear each other's lines.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// A file grows to this, then the next run moves it aside.
const KEEP: u64 = 1 << 20;

struct FileLog(Mutex<File>);

impl log::Log for FileLog {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {} {}: {}\n",
            jiff::Timestamp::now(),
            record.level(),
            record.target(),
            record.args()
        );
        if let Ok(mut f) = self.0.lock() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut f) = self.0.lock() {
            let _ = f.flush();
        }
    }
}

/// Where the log goes.
pub fn path() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Logs/livtk/livtk-app.log"))
    } else {
        livtk_core::registry::config_dir().map(|d| d.join("app.log"))
    }
}

/// Log to the file from now on. Without one (no home, no room) the app runs as it would
/// without a log: nothing it does depends on it.
pub fn init() {
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(file) = open(&path)
        && log::set_boxed_logger(Box::new(FileLog(Mutex::new(file)))).is_ok()
    {
        log::set_max_level(log::LevelFilter::Info);
    }
}

/// The log file at `path`, to append to: moved aside to `.old` first if it is past a megabyte.
/// Never truncated, as another process may be writing it too.
pub fn open(path: &std::path::Path) -> std::io::Result<File> {
    if fs::metadata(path).is_ok_and(|m| m.len() > KEEP) {
        let _ = fs::rename(path, path.with_extension("log.old"));
    }
    OpenOptions::new().create(true).append(true).open(path)
}
