//! Runs of Claude in the open projects, as the app keeps up with them: settled as it ticks (the
//! notes their turns earned written, answered ones started again), the ones writing now, and the
//! ones a quit stopped, to offer back. The survey is plain IO, run off the window's thread.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use livtk_core::event::Ending;
use livtk_core::run::{self, Runner, Stage};
use livtk_core::{Actor, Workspace};

use super::Model;

/// How often the runs are settled and looked at.
pub const RUNS_EVERY: Duration = Duration::from_secs(2);

/// A run, as the app names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRow {
    /// The project's folder.
    pub root: PathBuf,
    pub key: String,
    pub title: String,
    pub agent: Option<String>,
    pub stage: Stage,
}

/// What the app knows of the runs in its projects.
#[derive(Clone, Debug, Default)]
pub struct Runs {
    /// Writing now: a turn going.
    pub going: Vec<RunRow>,
    /// Stopped when the app last quit, not continued or left since.
    pub quit: Vec<RunRow>,
    /// A survey is going.
    pub surveying: bool,
    /// When the last one ended.
    pub surveyed: Option<Instant>,
}

/// The runs of the projects at `roots`, each settled first when `claude` is found.
pub fn survey(roots: &[PathBuf], runner: Option<&Runner>) -> (Vec<RunRow>, Vec<RunRow>) {
    let (mut going, mut quit) = (Vec::new(), Vec::new());
    for root in roots {
        let Ok(mut ws) = Workspace::open(root) else {
            continue;
        };
        if let Some(r) = runner {
            let _ = run::settle(&mut ws, r);
        }
        let writing = run::writing_sessions(ws.dir());
        let Ok(runs) = ws.runs() else { continue };
        for r in &runs {
            let row = || RunRow {
                root: root.clone(),
                key: r.key.clone(),
                title: ws.node(&r.key).map(|n| n.title).unwrap_or_default(),
                agent: r.agent.clone(),
                stage: r.stage,
            };
            // The latest run on its task only: a quit run since continued is not offered.
            let latest = runs.iter().rev().find(|x| x.node == r.node) == Some(r);
            if r.ended().is_none() && writing.contains(&r.session) {
                going.push(row());
            } else if latest && r.ended() == Some(Ending::Quit) {
                quit.push(row());
            }
        }
    }
    (going, quit)
}

/// Stop every run in `rows` as the app quits: each turn ends cleanly, its run noted as
/// stopped by the quit (written when its turn ends, by whichever livtk settles next).
pub fn stop_all(rows: &[RunRow], runner: &Runner) -> Vec<String> {
    let mut failed = Vec::new();
    for row in rows {
        let stopped = Workspace::open(&row.root)
            .map_err(|e| e.to_string())
            .and_then(|mut ws| {
                runner
                    .stop(&mut ws, &row.key, Ending::Quit, &Actor::Human)
                    .map_err(|e| e.to_string())
            });
        if let Err(e) = stopped {
            failed.push(format!("{}: {e}", row.key));
        }
    }
    failed
}

/// Continue each run in `rows` where it left off, or leave it stopped (noted, so it is not
/// offered again).
pub fn take_back(rows: &[RunRow], runner: &Runner, go_on: bool) -> Vec<String> {
    let mut failed = Vec::new();
    for row in rows {
        let done = Workspace::open(&row.root)
            .map_err(|e| e.to_string())
            .and_then(|mut ws| {
                if go_on {
                    runner.resume_stopped(&mut ws, &row.key, &Actor::Human)
                } else {
                    run::leave_stopped(&mut ws, &row.key, &Actor::Human)
                }
                .map_err(|e| e.to_string())
            });
        if let Err(e) = done {
            failed.push(format!("{}: {e}", row.key));
        }
    }
    failed
}

impl Model {
    /// The open projects' folders.
    pub fn project_roots(&self) -> Vec<PathBuf> {
        self.projects
            .iter()
            .map(|p| p.ws.root().to_path_buf())
            .collect()
    }

    /// Whether the runs are due to be surveyed.
    pub fn runs_due(&self, now: Instant) -> bool {
        !self.runs.surveying
            && self
                .runs
                .surveyed
                .is_none_or(|t| now.duration_since(t) >= RUNS_EVERY)
    }

    pub fn runs_surveyed(&mut self, going: Vec<RunRow>, quit: Vec<RunRow>, now: Instant) {
        self.runs.going = going;
        self.runs.quit = quit;
        self.runs.surveying = false;
        self.runs.surveyed = Some(now);
    }
}
