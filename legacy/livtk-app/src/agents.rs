//! Agents, outside the window: the Claude sessions working in a project (as Claude Code's own
//! `claude agents --json` lists them) and the agent files they can run as (livtk's, the
//! project's, the person's), read, written and removed. Plain IO, so it is tested without GPUI.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// How long `claude agents --json` may take before it is given up on.
const LIST_TIMEOUT: Duration = Duration::from_secs(10);

/// A Claude session, as Claude Code lists it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    #[serde(default)]
    pub pid: Option<u32>,
    pub cwd: PathBuf,
    /// `interactive` or `background`.
    #[serde(default)]
    pub kind: String,
    /// Milliseconds since the epoch.
    #[serde(default)]
    pub started_at: i64,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// While its process lives: `busy`, `waiting` or `idle`.
    #[serde(default)]
    pub status: Option<String>,
    /// A background session's id, for `claude stop`.
    #[serde(default)]
    pub id: Option<String>,
    /// A background session's state: working, blocked, done, failed or stopped.
    #[serde(default)]
    pub state: Option<String>,
}

/// Where a session stands, as people say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Standing {
    NeedsYou,
    Working,
    Idle,
    Finished,
}

impl Standing {
    pub const fn label(self) -> &'static str {
        match self {
            Standing::NeedsYou => "Needs you",
            Standing::Working => "Working",
            Standing::Idle => "Idle",
            Standing::Finished => "Finished",
        }
    }
}

impl Session {
    pub fn standing(&self) -> Standing {
        match (self.status.as_deref(), self.state.as_deref()) {
            (Some("waiting"), _) | (_, Some("blocked")) => Standing::NeedsYou,
            (Some("busy"), _) | (_, Some("working")) => Standing::Working,
            (_, Some("done" | "failed" | "stopped")) => Standing::Finished,
            _ => Standing::Idle,
        }
    }

    pub fn is_background(&self) -> bool {
        self.kind == "background"
    }

    /// What it is called: its name, else the start of its session id.
    pub fn title(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            self.session_id.as_deref().map_or_else(
                || "A session".to_owned(),
                |s| format!("Session {}", &s[..8.min(s.len())]),
            )
        })
    }

    /// A stable key for the list and the selection.
    pub fn key(&self) -> String {
        self.session_id
            .clone()
            .or_else(|| self.id.clone())
            .or_else(|| self.pid.map(|p| p.to_string()))
            .unwrap_or_default()
    }
}

/// Where to find `claude` and Claude Code's folder instead of looking: for tests, which cannot
/// set the environment safely.
static PATHS: std::sync::RwLock<Option<(PathBuf, PathBuf)>> = std::sync::RwLock::new(None);

/// Use `claude` as the command and `home` as Claude Code's folder from now on.
pub fn use_paths(claude: PathBuf, home: PathBuf) {
    *PATHS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((claude, home));
}

fn paths() -> Option<(PathBuf, PathBuf)> {
    PATHS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// The `claude` command: as `use_paths` said, else as livtk finds it.
pub fn claude() -> Option<PathBuf> {
    paths()
        .map(|(claude, _)| claude)
        .or_else(livtk_core::claude::command)
}

/// The sessions working under `root` (the project's folder, its worktrees included), Needs you
/// first, then the newest.
pub fn sessions(root: &Path) -> Result<Vec<Session>, String> {
    let claude =
        claude().ok_or("The claude command is not installed, or not where livtk looks.")?;
    let mut child = Command::new(&claude)
        .args(["agents", "--json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("claude agents would not start: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > LIST_TIMEOUT => {
                let _ = child.kill();
                return Err("claude agents took too long to answer.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut out = String::new();
    child
        .stdout
        .take()
        .ok_or("claude agents said nothing")?
        .read_to_string(&mut out)
        .map_err(|e| e.to_string())?;
    let mut all: Vec<Session> = serde_json::from_str(&out)
        .map_err(|e| format!("claude agents said something unexpected: {e}"))?;
    // Claude Code reports real paths; the project's may go through a link (/var, /private/var).
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let root = real(root);
    all.retain(|s| real(&s.cwd).starts_with(&root));
    all.sort_by(|a, b| {
        a.standing()
            .cmp(&b.standing())
            .then(b.started_at.cmp(&a.started_at))
    });
    Ok(all)
}

/// Stop background session `id`, from the project's folder `root` as its sessions are listed.
pub fn stop(root: &Path, id: &str) -> Result<(), String> {
    let claude = claude().ok_or("The claude command is not installed.")?;
    let out = Command::new(claude)
        .args(["stop", id])
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

// ---- agent files --------------------------------------------------------------------------------

pub use livtk_core::agents::{
    Agent, AgentFile, MODELS, Place, TOOLS, parse, remove, render, valid_name,
};

/// Claude Code's own folder: as `use_paths` said, else as livtk finds it.
pub fn claude_home() -> Option<PathBuf> {
    paths()
        .map(|(_, home)| home)
        .or_else(livtk_core::claude::home)
}

/// The folder of a place's agents, for the project at `root`.
pub fn folder(place: Place, root: &Path) -> Option<PathBuf> {
    livtk_core::agents::folder(place, root, &claude_home()?)
}

/// Every agent the project at `root` can run as, by place, then by name.
pub fn agents(root: &Path) -> Vec<AgentFile> {
    claude_home()
        .map(|home| livtk_core::agents::agents(root, &home))
        .unwrap_or_default()
}

/// Write `agent` to `place` for the project at `root`: a new file, or `replacing` the one it was.
pub fn save(
    root: &Path,
    place: Place,
    agent: &Agent,
    replacing: Option<&Path>,
) -> Result<PathBuf, String> {
    let home = claude_home().ok_or("There is no home folder to put it in.")?;
    livtk_core::agents::save(root, &home, place, agent, replacing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_reads_as_people_say_it() {
        let s = |status: Option<&str>, state: Option<&str>| Session {
            pid: None,
            cwd: PathBuf::new(),
            kind: String::new(),
            started_at: 0,
            session_id: None,
            name: None,
            status: status.map(str::to_owned),
            id: None,
            state: state.map(str::to_owned),
        };
        assert_eq!(s(Some("waiting"), None).standing(), Standing::NeedsYou);
        assert_eq!(s(None, Some("blocked")).standing(), Standing::NeedsYou);
        assert_eq!(s(Some("busy"), None).standing(), Standing::Working);
        assert_eq!(s(Some("idle"), None).standing(), Standing::Idle);
        assert_eq!(s(None, Some("done")).standing(), Standing::Finished);
    }
}
