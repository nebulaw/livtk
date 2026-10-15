//! `events.jsonl`: the append-only log that is the durable truth.
//!
//! A line is committed when its newline is on disk. Anything after the last newline is a write
//! that never finished, and the next writer drops it.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use crate::event::{Event, Line, parse_line};

/// Bytes hashed at each end of the applied prefix to notice a rewritten log.
const WINDOW: u64 = 4096;

pub struct Log {
    path: PathBuf,
}

/// A fingerprint of the log's first `len` bytes: their length and a hash of each end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mark {
    pub len: u64,
    pub head: u64,
    pub tail: u64,
}

/// The events read from a stretch of the log, and what had to be skipped.
#[derive(Debug, Default)]
pub struct Parsed {
    pub events: Vec<Event>,
    pub warnings: Vec<String>,
}

impl Log {
    pub fn new(path: PathBuf) -> Log {
        Log { path }
    }

    /// The current length in bytes; a missing log is empty.
    pub fn len(&self) -> io::Result<u64> {
        match std::fs::metadata(&self.path) {
            Ok(m) => Ok(m.len()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(e),
        }
    }

    /// The fingerprint of the first `len` bytes.
    pub fn mark(&self, len: u64) -> io::Result<Mark> {
        if len == 0 {
            return Ok(Mark::default());
        }
        let mut f = File::open(&self.path)?;
        let w = WINDOW.min(len);
        let head = read_at(&mut f, 0, w)?;
        let tail = read_at(&mut f, len - w, w)?;
        Ok(Mark {
            len,
            head: fnv1a(&head),
            tail: fnv1a(&tail),
        })
    }

    /// Everything from `offset` to the end.
    pub fn read_from(&self, offset: u64) -> io::Result<Vec<u8>> {
        let mut f = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        f.seek(SeekFrom::Start(offset))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// Append whole lines in one write and wait until they are on disk. Returns the new length.
    /// The caller holds the workspace lock.
    pub fn append(&self, lines: &[String]) -> io::Result<u64> {
        let mut buf = String::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
        for line in lines {
            buf.push_str(line);
            buf.push('\n');
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(buf.as_bytes())?;
        f.sync_data()?;
        Ok(f.metadata()?.len())
    }

    /// Drop a last line that has no newline: a write that never finished. Returns a warning that
    /// carries the dropped text, so nothing disappears silently. The caller holds the lock.
    pub fn repair_tail(&self) -> io::Result<Option<String>> {
        let len = self.len()?;
        if len == 0 {
            return Ok(None);
        }
        let mut f = OpenOptions::new().read(true).write(true).open(&self.path)?;
        if read_at(&mut f, len - 1, 1)? == b"\n" {
            return Ok(None);
        }
        let mut end = len;
        let cut = loop {
            let start = end.saturating_sub(WINDOW);
            let chunk = read_at(&mut f, start, end - start)?;
            if let Some(i) = chunk.iter().rposition(|&b| b == b'\n') {
                break start + i as u64 + 1;
            }
            if start == 0 {
                break 0;
            }
            end = start;
        };
        let dropped = read_at(&mut f, cut, len - cut)?;
        f.set_len(cut)?;
        f.sync_data()?;
        let text = String::from_utf8_lossy(&dropped);
        let shown: String = text.chars().take(160).collect();
        Ok(Some(format!(
            "dropped an unfinished last line of {} ({} bytes): {shown}",
            self.path.display(),
            dropped.len()
        )))
    }
}

/// Parse the complete lines in `bytes`, which start at byte `offset` of the log. A trailing piece
/// without a newline is left alone: it is not committed yet.
pub fn parse(bytes: &[u8], offset: u64) -> Parsed {
    let mut parsed = Parsed::default();
    let mut pos = 0usize;
    while let Some(n) = bytes[pos..].iter().position(|&b| b == b'\n') {
        let raw = &bytes[pos..pos + n];
        let at = offset + pos as u64;
        pos += n + 1;
        let text = match std::str::from_utf8(raw) {
            Ok(t) => t.trim(),
            Err(_) => {
                parsed
                    .warnings
                    .push(format!("skipped a line at byte {at}: not UTF-8"));
                continue;
            }
        };
        if text.is_empty() {
            continue;
        }
        match parse_line(text) {
            Line::Event(e) => parsed.events.push(*e),
            Line::Newer { v, .. } => parsed.warnings.push(format!(
                "skipped an event at byte {at} written by a newer livtk (format {v}); update livtk"
            )),
            Line::Garbage(why) => parsed.warnings.push(format!(
                "skipped a line at byte {at} that is not an event: {why}"
            )),
        }
    }
    parsed
}

fn read_at(f: &mut File, offset: u64, n: u64) -> io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    Read::by_ref(f).take(n).read_to_end(&mut buf)?;
    Ok(buf)
}

/// FNV-1a, 64 bit: stable across Rust versions, which `std`'s hasher does not promise.
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_in(dir: &tempfile::TempDir) -> (Log, PathBuf) {
        let path = dir.path().join("events.jsonl");
        (Log::new(path.clone()), path)
    }

    #[test]
    fn missing_log_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (log, _) = log_in(&dir);
        assert_eq!(log.len().unwrap(), 0);
        assert!(log.read_from(0).unwrap().is_empty());
        assert_eq!(log.mark(0).unwrap(), Mark::default());
        assert_eq!(log.repair_tail().unwrap(), None);
    }

    #[test]
    fn append_returns_the_new_length() {
        let dir = tempfile::tempdir().unwrap();
        let (log, _) = log_in(&dir);
        let len = log.append(&["a".into(), "bc".into()]).unwrap();
        assert_eq!(len, 5);
        assert_eq!(log.read_from(2).unwrap(), b"bc\n");
    }

    #[test]
    fn an_unfinished_tail_is_dropped_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (log, path) = log_in(&dir);
        std::fs::write(&path, b"one\ntwo\n{\"v\":1,\"id\"").unwrap();
        let warning = log.repair_tail().unwrap().unwrap();
        assert!(warning.contains("{\"v\":1,\"id\""), "{warning}");
        assert_eq!(std::fs::read(&path).unwrap(), b"one\ntwo\n");
        assert_eq!(log.repair_tail().unwrap(), None);
    }

    #[test]
    fn a_tail_longer_than_the_window_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let (log, path) = log_in(&dir);
        let mut content = b"keep\n".to_vec();
        content.extend(std::iter::repeat_n(b'x', 10_000));
        std::fs::write(&path, &content).unwrap();
        assert!(log.repair_tail().unwrap().is_some());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep\n");
    }

    #[test]
    fn marks_notice_a_rewrite_of_either_end() {
        let dir = tempfile::tempdir().unwrap();
        let (log, path) = log_in(&dir);
        std::fs::write(&path, b"aaaa\nbbbb\n").unwrap();
        let before = log.mark(10).unwrap();
        std::fs::write(&path, b"aaaa\nbbbc\n").unwrap();
        assert_ne!(log.mark(10).unwrap(), before);
        std::fs::write(&path, b"aaab\nbbbb\n").unwrap();
        assert_ne!(log.mark(10).unwrap(), before);
        std::fs::write(&path, b"aaaa\nbbbb\nmore\n").unwrap();
        assert_eq!(
            log.mark(10).unwrap(),
            before,
            "an append keeps the old prefix's mark"
        );
    }

    #[test]
    fn parse_skips_noise_and_leaves_the_unfinished_tail() {
        let parsed = parse(b"\nnot json\n\xff\n{\"v\":1", 0);
        assert!(parsed.events.is_empty());
        assert_eq!(parsed.warnings.len(), 2, "{:?}", parsed.warnings);
    }
}
