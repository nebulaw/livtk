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
    /// livtk's runs writing now: a turn going.
    pub going: Vec<RunRow>,
    /// The tasks Claude is writing on now: livtk's runs', and those of sessions livtk did not
    /// start (marked by their hooks), on the tasks in progress they worked on.
    pub writing: Vec<RunRow>,
    /// The tasks whose run waits for you.
    pub needs_you: Vec<RunRow>,
    /// Stopped when the app last quit, not continued or left since.
    pub quit: Vec<RunRow>,
    /// A survey is going.
    pub surveying: bool,
    /// When the last one ended.
    pub surveyed: Option<Instant>,
}

/// What a survey found.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub going: Vec<RunRow>,
    pub quit: Vec<RunRow>,
    pub writing: Vec<RunRow>,
    pub needs_you: Vec<RunRow>,
}

/// The runs of the projects at `roots`, each settled first when `runner` is given: those writing
/// now, those waiting for you, and those the last quit stopped; and the tasks being written on.
pub fn survey(roots: &[PathBuf], runner: Option<&Runner>) -> Found {
    let mut found = Found::default();
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
                found.going.push(row());
                found.writing.push(row());
            } else if latest && r.needs_you() {
                found.needs_you.push(row());
            } else if latest && r.ended() == Some(Ending::Quit) {
                found.quit.push(row());
            }
        }
        // Sessions livtk did not start, writing: on the tasks in progress they worked on.
        for session in writing
            .iter()
            .filter(|s| !runs.iter().any(|r| &r.session == *s))
        {
            for b in ws.session_nodes(session).unwrap_or_default() {
                let doing = ws
                    .node(&b.key)
                    .is_ok_and(|n| n.state == livtk_core::State::Doing);
                if doing
                    && !found
                        .writing
                        .iter()
                        .any(|w| w.root == *root && w.key == b.key)
                {
                    found.writing.push(RunRow {
                        root: root.clone(),
                        key: b.key.clone(),
                        title: b.title.clone(),
                        agent: None,
                        stage: Stage::Build,
                    });
                }
            }
        }
    }
    found
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

    pub fn runs_surveyed(&mut self, found: Found, now: Instant) {
        self.runs.going = found.going;
        self.runs.quit = found.quit;
        self.runs.writing = found.writing;
        self.runs.needs_you = found.needs_you;
        self.runs.surveying = false;
        self.runs.surveyed = Some(now);
    }

    /// Survey the runs at the next tick, not a beat later: something just changed them.
    pub fn survey_runs_soon(&mut self) {
        self.runs.surveyed = None;
    }

    /// Whether Claude is writing on task `key` of the open project.
    pub fn writing_on(&self, key: &str) -> bool {
        self.in_project(&self.runs.writing, key)
    }

    /// Whether task `key` of the open project has a run waiting for you.
    pub fn run_needs_you(&self, key: &str) -> bool {
        self.in_project(&self.runs.needs_you, key)
    }

    fn in_project(&self, rows: &[RunRow], key: &str) -> bool {
        let Some(root) = self.project().map(|p| p.ws.root()) else {
            return false;
        };
        rows.iter().any(|r| r.key == key && r.root == root)
    }
}
