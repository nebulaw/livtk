//! Hooks on signals: a project's `[[on]]` entries in `.livtk/config.toml` run a command for
//! each signal they name, with the signal's JSON on stdin. They run after the event is in the
//! log, from a dispatcher apart from the write, so a hook never blocks or undoes one.
//!
//! Each hook keeps what it has heard (`.livtk/on-state.json`, local to this machine): the events
//! it handled within a horizon, and that everything older is handled. An event counts as handled
//! for a hook only once its command succeeded for every signal of it, so a failing hook is tried
//! again on the next dispatch, in order. An event merged in from elsewhere, older than the newest
//! one handled, is still heard if it is within the horizon; signals older than that are stale
//! for waking anyone. A hook seen for the first time hears from the write that woke it on, never
//! the project's history.
//!
//! A hook runs with `LIVTK_HOOK` set: livtk run inside it acts as `claude:hook` (never as the
//! human, so a hook cannot pass the human's gates) and starts no dispatcher; and no hook hears
//! what a hook did, so hooks never wake each other in a loop.
//!
//! One dispatcher at a time (`.livtk/on.lock`). It reads on until nothing new is left, and looks
//! once more after letting go, so a write made while it ran is not left unheard.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Result;
use crate::event::Event;
use crate::model::Kind;
use crate::signal::{BY_HOOK, Signal, Signals};
use crate::workspace::Workspace;

/// The variable a hook runs with: livtk inside it acts as `claude:hook`.
pub const HOOK_VAR: &str = "LIVTK_HOOK";

/// One `[[on]]` entry: run `run` for each `signal` (about a `kind`, if given).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct On {
    pub signal: String,
    pub run: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
}

impl On {
    /// What tells this hook apart in its state: its whole entry.
    fn id(&self) -> String {
        let kind = self.kind.map_or("", |k| k.as_str());
        format!("{} {} {}", self.signal, kind, self.run.join(" "))
    }

    fn hears(&self, s: &Signal) -> bool {
        s.signal == self.signal && self.kind.is_none_or(|k| k == s.kind) && s.by != BY_HOOK
    }
}

/// How long one run of a hook's command may take before it is stopped (and counted failed).
pub const RUN_FOR: Duration = Duration::from_secs(30);
/// How far back a late event (merged in from elsewhere) is still heard.
pub const HORIZON: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// What a dispatch did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dispatched {
    /// Commands run and succeeded.
    pub ran: usize,
    /// Hooks whose command failed, to be tried again, and anything else worth saying: each a
    /// line.
    pub failed: Vec<String>,
    /// Another dispatcher was at work; this one left it to it.
    pub busy: bool,
}

/// What one hook has heard: every event older than `before` (milliseconds since the epoch, by
/// the event's own time), and these newer ones.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Heard {
    before: u64,
    seen: BTreeSet<Uuid>,
}

impl Heard {
    fn has(&self, e: Uuid) -> bool {
        millis(e) < self.before || self.seen.contains(&e)
    }

    /// Forget what lies beyond the horizon, holding it as heard.
    fn compact(&mut self, now: u64) {
        let horizon = now.saturating_sub(HORIZON.as_millis() as u64);
        if horizon > self.before {
            self.before = horizon;
            self.seen.retain(|e| millis(*e) >= horizon);
        }
    }
}

/// An event's own time, in milliseconds since the epoch (uuidv7).
fn millis(e: Uuid) -> u64 {
    e.get_timestamp().map_or(0, |t| {
        let (s, n) = t.to_unix();
        s * 1000 + u64::from(n) / 1_000_000
    })
}

const STATE: &str = "on-state.json";
const LOCK: &str = "on.lock";

impl Workspace {
    /// Run each hook for the signals it has not heard yet. Nothing to do without `[[on]]`.
    pub fn dispatch(&mut self) -> Result<Dispatched> {
        self.dispatch_from(None)
    }

    /// `dispatch`; a hook seen for the first time has heard everything up to `new_from` (the
    /// newest event before the write that started this dispatch, so it hears that write), else
    /// everything there is.
    pub fn dispatch_from(&mut self, new_from: Option<Uuid>) -> Result<Dispatched> {
        let mut done = Dispatched::default();
        if self.config().on.is_empty() {
            return Ok(done);
        }
        let dir = self.dir().to_path_buf();
        keep_local(&dir)?;
        loop {
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(dir.join(LOCK))?;
            if lock.try_lock().is_err() {
                done.busy = true;
                return Ok(done);
            }
            // One pass at least (it also takes in a hook new to the state), then on until
            // nothing new is left: a write made meanwhile is heard in this same run.
            let mut handled = None;
            let mut passes = 0;
            loop {
                self.refresh()?;
                let newest = self.newest_event()?;
                if passes > 0 && newest == handled {
                    break;
                }
                self.dispatch_once(&dir, new_from, &mut done)?;
                handled = newest;
                passes += 1;
            }
            drop(lock);
            // A write between the last look and letting go found the lock taken: look again.
            self.refresh()?;
            if self.newest_event()? == handled {
                return Ok(done);
            }
        }
    }

    /// One pass over the log: every hook, every signal it has not heard.
    fn dispatch_once(
        &mut self,
        dir: &Path,
        new_from: Option<Uuid>,
        done: &mut Dispatched,
    ) -> Result<()> {
        let hooks = self.config().on.clone();
        let (mut state, unreadable) = read_state(dir);
        if unreadable {
            done.failed.push(format!(
                "{STATE} could not be read: each hook hears from now on"
            ));
        }
        let events = self.events_after(None)?;
        let mut signals = Signals::new(self.gates());
        let by_event: Vec<(&Event, Vec<Signal>)> =
            events.iter().map(|e| (e, signals.feed(e))).collect();
        let mut keys: BTreeMap<Uuid, Option<String>> = BTreeMap::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let mut kept = BTreeMap::new();
        for hook in &hooks {
            let id = hook.id();
            let mut heard = state.remove(&id).unwrap_or_else(|| {
                // New: everything up to `new_from` (or all there is) counts as heard.
                let upto = new_from.or_else(|| events.last().map(|e| e.id));
                Heard {
                    before: 0,
                    seen: events
                        .iter()
                        .map(|e| e.id)
                        .filter(|e| upto.is_some_and(|u| *e <= u))
                        .collect(),
                }
            });
            let unheard: Vec<usize> = (0..by_event.len())
                .filter(|i| !heard.has(by_event[*i].0.id))
                .collect();
            'events: for (event, sigs) in unheard.into_iter().map(|i| &by_event[i]) {
                for s in sigs.iter().filter(|s| hook.hears(s)) {
                    // The key as it stands now: a merge may have renumbered the node.
                    let key = match keys.get(&s.node) {
                        Some(k) => k.clone(),
                        None => {
                            let k = self.key_of(s.node)?;
                            keys.insert(s.node, k.clone());
                            k
                        }
                    };
                    let mut s = s.clone();
                    if let Some(k) = key {
                        s.key = k;
                    }
                    match run(hook, &s, dir) {
                        Ok(()) => done.ran += 1,
                        Err(why) => {
                            done.failed.push(format!("{}: {why}", hook.run.join(" ")));
                            break 'events;
                        }
                    }
                }
                heard.seen.insert(event.id);
            }
            heard.compact(now);
            kept.insert(id, heard);
        }
        // What is kept is what the hooks there now have heard: a changed entry starts afresh.
        write_state(dir, &kept)
    }
}

/// Run `hook` for `s` in the project's folder, the signal on stdin.
fn run(hook: &On, s: &Signal, dir: &Path) -> std::result::Result<(), String> {
    let Some((program, args)) = hook.run.split_first() else {
        return Err("nothing to run".into());
    };
    let mut child = Command::new(program)
        .args(args)
        .env(HOOK_VAR, "1")
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

/// The hooks' state, and whether a file there could not be read.
fn read_state(dir: &Path) -> (BTreeMap<String, Heard>, bool) {
    match fs::read_to_string(dir.join(STATE)) {
        Err(_) => (BTreeMap::new(), false),
        Ok(text) => match serde_json::from_str(&text) {
            Ok(state) => (state, false),
            Err(_) => (BTreeMap::new(), true),
        },
    }
}

fn write_state(dir: &Path, state: &BTreeMap<String, Heard>) -> Result<()> {
    // Written beside and moved into place: a reader never sees half of it.
    let tmp = dir.join(format!("{STATE}.tmp"));
    let mut f = File::create(&tmp)?;
    f.write_all(serde_json::to_string_pretty(state)?.as_bytes())?;
    f.sync_all()?;
    fs::rename(tmp, dir.join(STATE))?;
    Ok(())
}

/// The state and the lock are this machine's: kept out of git, also in a project made before
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
