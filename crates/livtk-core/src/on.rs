//! Hooks on signals: a project's `[[on]]` entries in `.livtk/config.toml` run a command for
//! each signal they name, with the signal's JSON on stdin. They run after the event is in the
//! log, from a dispatcher apart from the write, so a hook never blocks or undoes one.
//!
//! Each hook keeps a cursor (`.livtk/on-state.json`, local to this machine): it moves past an
//! event only once the hook's command succeeded for every signal of it, so a failing hook is
//! tried again on the next dispatch, in order. A hook seen for the first time starts at the end
//! of the log: it hears what comes, not the project's history. One dispatcher at a time
//! (`.livtk/on.lock`); another that finds it taken leaves the work to it.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Result;
use crate::model::Kind;
use crate::signal::{Signal, Signals};
use crate::workspace::Workspace;

/// One `[[on]]` entry: run `run` for each `signal` (about a `kind`, if given).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct On {
    pub signal: String,
    pub run: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
}

impl On {
    /// What tells this hook apart in the cursors: its whole entry.
    fn id(&self) -> String {
        let kind = self.kind.map_or("", |k| k.as_str());
        format!("{} {} {}", self.signal, kind, self.run.join(" "))
    }

    fn hears(&self, s: &Signal) -> bool {
        s.signal == self.signal && self.kind.is_none_or(|k| k == s.kind)
    }
}

/// How long one run of a hook's command may take before it is stopped (and counted failed).
pub const RUN_FOR: Duration = Duration::from_secs(30);

/// What a dispatch did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dispatched {
    /// Commands run and succeeded.
    pub ran: usize,
    /// Hooks whose command failed, to be tried again: each with why.
    pub failed: Vec<String>,
    /// Another dispatcher was at work; this one did nothing.
    pub busy: bool,
}

const STATE: &str = "on-state.json";
const LOCK: &str = "on.lock";

impl Workspace {
    /// Run each hook for the signals it has not heard yet. Nothing to do without `[[on]]`.
    pub fn dispatch(&mut self) -> Result<Dispatched> {
        self.dispatch_from(None)
    }

    /// `dispatch`, a hook seen for the first time starting after `new_from` (the newest event
    /// before the write that started this dispatch, so it hears that write), else at the end.
    pub fn dispatch_from(&mut self, new_from: Option<Uuid>) -> Result<Dispatched> {
        let hooks = self.config().on.clone();
        let mut done = Dispatched::default();
        if hooks.is_empty() {
            return Ok(done);
        }
        let dir = self.dir().to_path_buf();
        keep_local(&dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LOCK))?;
        if lock.try_lock().is_err() {
            done.busy = true;
            return Ok(done);
        }
        let mut cursors = read_state(&dir);
        let events = self.events_after(None)?;
        let last = events.last().map(|e| e.id);
        let mut signals = Signals::new(self.gates());
        let by_event: Vec<(Uuid, Vec<Signal>)> =
            events.iter().map(|e| (e.id, signals.feed(e))).collect();
        for hook in &hooks {
            let id = hook.id();
            let cursor = match cursors.get(&id).copied() {
                Some(c) => c,
                None => {
                    // New: it hears from now on (in an empty log, from its start).
                    let start = new_from.or(last).unwrap_or(Uuid::nil());
                    cursors.insert(id.clone(), start);
                    start
                }
            };
            'events: for (event, sigs) in by_event.iter().filter(|(e, _)| *e > cursor) {
                for s in sigs.iter().filter(|s| hook.hears(s)) {
                    match run(hook, s, &dir) {
                        Ok(()) => done.ran += 1,
                        Err(why) => {
                            done.failed.push(format!("{}: {why}", hook.run.join(" ")));
                            break 'events;
                        }
                    }
                }
                cursors.insert(id.clone(), *event);
            }
        }
        write_state(&dir, &cursors)?;
        drop(lock);
        Ok(done)
    }
}

/// Run `hook` for `s` in the project's folder, the signal on stdin.
fn run(hook: &On, s: &Signal, dir: &Path) -> std::result::Result<(), String> {
    let Some((program, args)) = hook.run.split_first() else {
        return Err("nothing to run".into());
    };
    let mut child = Command::new(program)
        .args(args)
        .current_dir(dir.parent().unwrap_or(dir))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let line = serde_json::to_string(s).map_err(|e| e.to_string())?;
        // A command that does not read its input is no failure.
        let _ = writeln!(stdin, "{line}");
    }
    let until = Instant::now() + RUN_FOR;
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => return Err(format!("exited with {status}")),
            None if Instant::now() >= until => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("still running after {}s", RUN_FOR.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn read_state(dir: &Path) -> BTreeMap<String, Uuid> {
    fs::read_to_string(dir.join(STATE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_state(dir: &Path, cursors: &BTreeMap<String, Uuid>) -> Result<()> {
    // Written beside and moved into place: a reader never sees half of it.
    let tmp = dir.join(format!("{STATE}.tmp"));
    let mut f = File::create(&tmp)?;
    f.write_all(serde_json::to_string_pretty(cursors)?.as_bytes())?;
    f.sync_all()?;
    fs::rename(tmp, dir.join(STATE))?;
    Ok(())
}

/// The cursors and the lock are this machine's: kept out of git, also in a project made before
/// they existed.
fn keep_local(dir: &Path) -> Result<()> {
    let path = dir.join(".gitignore");
    let have = fs::read_to_string(&path).unwrap_or_default();
    let missing: Vec<&str> = [LOCK, "on-state.json*"]
        .into_iter()
        .filter(|l| !have.lines().any(|h| h == *l))
        .collect();
    if !missing.is_empty() {
        let mut f = OpenOptions::new().create(true).append(true).open(path)?;
        if !have.is_empty() && !have.ends_with('\n') {
            writeln!(f)?;
        }
        for l in missing {
            writeln!(f, "{l}")?;
        }
    }
    Ok(())
}
