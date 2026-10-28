//! Liv's log: what GPUI and Liv say of themselves, information and up, to its own file, since a
//! GUI app has no terminal. Past a megabyte at start, the file moves aside to `.old` and a new one
//! begins. Each line is written whole, in one append.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

/// A file grows to this, then the next start moves it aside.
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

/// Start logging to Liv's file. Without one (no home folder), Liv runs unlogged.
pub fn init() {
    let Some(path) = crate::identity::Folders::here().map(|f| f.log) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > KEEP) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    if log::set_boxed_logger(Box::new(FileLog(Mutex::new(file)))).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}
