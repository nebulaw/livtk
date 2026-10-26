//! Projects: an outcome above tasks. Its progress is counted from the tasks under it, never
//! typed in; how it stands is written, in updates.

use std::collections::HashMap;

use jiff::Timestamp;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::Workspace;
use crate::model::{Health, Kind, State};
use crate::store::{Brief, Node, Summary};
use crate::{Error, Result};

/// What a project's outcome says "done" is: its `Done when:` line, else its body's first line.
pub fn done_when(body: &str) -> Option<&str> {
    let lines = || body.lines().map(str::trim).filter(|l| !l.is_empty());
    lines()
        .find(|l| l.to_lowercase().starts_with("done when"))
        .or_else(|| lines().next())
}

/// A target as a project takes it: empty, or a date (YYYY-MM-DD).
pub fn check_target(target: &str) -> Result<&str> {
    let t = target.trim();
    if !t.is_empty() && t.parse::<jiff::civil::Date>().is_err() {
        return Err(Error::Invalid(format!(
            "target `{t}` is not a date (YYYY-MM-DD)"
        )));
    }
    Ok(t)
}

/// An owner as a project takes it: empty, or a person; never Claude.
pub fn check_owner(owner: &str) -> Result<&str> {
    let o = owner.trim();
    let lower = o.to_lowercase();
    if lower == "claude" || lower.starts_with("claude:") {
        return Err(Error::Refused(
            "Claude never owns an outcome; name the person who does".into(),
        ));
    }
    Ok(o)
}

/// A project to make: what `livtk project new` and the app's form both ask for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewProject {
    /// The outcome, in a line.
    pub title: String,
    /// What it is for and how it gets there.
    pub body: String,
    /// What done means, in a line: the body's last line, `Done when: …`, in place of any the
    /// body has. Empty: the body says it, or nothing does.
    pub done_when: String,
    pub target: Option<String>,
    pub owner: Option<String>,
    /// The project it is part of, by key.
    pub part_of: Option<String>,
    /// Committed to at once (active), as the human may; else proposed.
    pub commit: bool,
}

impl NewProject {
    /// Its body as stored: the body, then its `Done when:` line. A done-when given apart takes
    /// the place of any line the body has for it, so it is never lost and never said twice.
    pub fn full_body(&self) -> String {
        let done = self.done_when.trim();
        if done.is_empty() {
            return self.body.trim().to_owned();
        }
        let body: Vec<&str> = self.body.lines().filter(|l| !is_done_line(l)).collect();
        let body = body.join("\n");
        let body = body.trim();
        if body.is_empty() {
            format!("Done when: {done}")
        } else {
            format!("{body}\n\nDone when: {done}")
        }
    }

    /// Whether it says what done means: a done-when given, or a line of its body.
    pub fn says_done(&self) -> bool {
        !self.done_when.trim().is_empty() || self.body.lines().any(is_done_line)
    }
}

/// A body's line saying what done means.
fn is_done_line(line: &str) -> bool {
    line.trim().to_lowercase().starts_with("done when")
}

/// An active project with no update for this long reads "update missing".
pub const UPDATE_EVERY_DAYS: i64 = 7;

/// More active projects than this, and the list says so: a nudge, not a lock.
pub const ACTIVE_AT_ONCE: usize = 3;

/// A project as its tasks and its updates tell it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Progress {
    /// Its tasks done, out of all of them but the canceled, nested projects counted through.
    pub done: usize,
    pub total: usize,
    /// The projects inside it, at any depth.
    pub projects: Vec<String>,
    /// Its tasks waiting on the human, by the state they wait in.
    pub waiting: Vec<Waiting>,
    /// Its newest update.
    pub update: Option<Update>,
    /// Active, and no update for a week (counted from when it became active, if later).
    pub update_missing: bool,
    pub target: Option<String>,
    pub owner: Option<String>,
}

/// Tasks waiting on the human in one state: how many, and the one waiting longest.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Waiting {
    pub state: State,
    pub count: usize,
    pub oldest: String,
    /// When the oldest got there.
    pub since: String,
}

/// A project's update: how it stands, what moved and what is next.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Update {
    pub at: String,
    pub health: Health,
    pub text: String,
}

impl Workspace {
    /// Make project `p`: everything it is given checked before anything is written, then made,
    /// its target and owner set, and committed to when asked (the human's move). The events
    /// written, the project's creation first.
    pub fn new_project(&mut self, p: &NewProject, by: &crate::Actor) -> Result<Vec<crate::Event>> {
        use crate::workspace::Command;
        if p.title.trim().is_empty() {
            return Err(Error::Invalid(
                "a project needs its outcome, in a line".into(),
            ));
        }
        let target = p.target.as_deref().map(check_target).transpose()?;
        let owner = p.owner.as_deref().map(check_owner).transpose()?;
        if let Some(parent) = &p.part_of {
            let n = self.node(parent)?;
            if n.kind != Kind::Project {
                return Err(Error::Invalid(format!("{} is not a project", n.key)));
            }
        }
        if p.commit && !by.is_human() {
            return Err(Error::Refused(
                "committing to a project is the human's move".into(),
            ));
        }
        let mut events = self.execute(
            Command::Create {
                kind: Kind::Project,
                title: p.title.trim().to_owned(),
                body: p.full_body(),
                priority: None,
                links: p
                    .part_of
                    .iter()
                    .map(|k| (crate::Rel::Parent, k.clone()))
                    .collect(),
            },
            by,
        )?;
        let Some(crate::Op::NodeCreate { node, .. }) = events.first().map(|e| e.op.clone()) else {
            return Ok(events);
        };
        let node = node.to_string();
        let set = |v: Option<&str>| v.filter(|s| !s.is_empty()).map(str::to_owned);
        if target.is_some_and(|t| !t.is_empty()) || owner.is_some_and(|o| !o.is_empty()) {
            events.extend(self.execute(
                Command::SetProject {
                    node: node.clone(),
                    target: set(target),
                    owner: set(owner),
                },
                by,
            )?);
        }
        if p.commit {
            events.extend(self.execute(
                Command::Move {
                    node,
                    to: State::Active,
                },
                by,
            )?);
        }
        Ok(events)
    }

    /// What project `key` has come to: counted from its tasks, as of `now`.
    pub fn progress(&self, key: &str, now: Timestamp) -> Result<Progress> {
        let p = self.node(key)?;
        if p.kind != Kind::Project {
            return Err(Error::Invalid(format!(
                "{} is a {}, not a project",
                p.key, p.kind
            )));
        }
        let parts = self.store.parts_under(p.id)?;
        let tasks: Vec<&Summary> = parts
            .iter()
            .filter(|s| s.kind == Kind::Task && s.state != State::Canceled)
            .collect();
        let mut waiting = Vec::new();
        for state in self.waits_on_human(Kind::Task) {
            let mut oldest: Option<(String, &str)> = None;
            let mut count = 0;
            for t in tasks.iter().filter(|t| t.state == state) {
                count += 1;
                let since = self.store.entered(t.id)?.unwrap_or_default();
                if oldest.as_ref().is_none_or(|(s, _)| since < *s) {
                    oldest = Some((since, &t.key));
                }
            }
            if let Some((since, key)) = oldest {
                waiting.push(Waiting {
                    state,
                    count,
                    oldest: key.to_owned(),
                    since,
                });
            }
        }
        let update = self
            .store
            .last_update(p.id)?
            .map(|(at, health, text)| -> Result<Update> {
                Ok(Update {
                    at,
                    health: health.parse()?,
                    text,
                })
            })
            .transpose()?;
        let update_missing = p.state == State::Active && {
            let since = match (&update, self.store.entered(p.id)?) {
                (Some(u), Some(entered)) => u.at.clone().max(entered),
                (Some(u), None) => u.at.clone(),
                (None, entered) => entered.unwrap_or_default(),
            };
            since.parse::<Timestamp>().map_or(true, |t| {
                now.as_second() - t.as_second() > UPDATE_EVERY_DAYS * 86_400
            })
        };
        let prop = |name: &str| p.props.get(name).and_then(Value::as_str).map(str::to_owned);
        Ok(Progress {
            done: tasks.iter().filter(|t| t.state == State::Done).count(),
            total: tasks.len(),
            projects: parts
                .iter()
                .filter(|s| s.kind == Kind::Project)
                .map(|s| s.key.clone())
                .collect(),
            waiting,
            update,
            update_missing,
            target: prop("target"),
            owner: prop("owner"),
        })
    }

    /// Every project, in any state.
    pub fn projects(&self) -> Result<Vec<Summary>> {
        self.store.list(Kind::Project, &[])
    }

    /// The nearest project each node is part of, by the project's id; empty, and free, while
    /// there is none.
    pub fn projects_of(&self) -> Result<HashMap<Uuid, (Uuid, Brief)>> {
        Ok(self
            .store
            .projects_of()?
            .into_iter()
            .map(|(node, id, brief)| (node, (id, brief)))
            .collect())
    }

    /// The nearest project node `key` is part of.
    pub fn project_of(&self, key: &str) -> Result<Option<Node>> {
        let n = self.node(key)?;
        match self
            .store
            .projects_of()?
            .into_iter()
            .find(|(id, _, _)| *id == n.id)
        {
            Some((_, project, _)) => self.store.node(project),
            None => Ok(None),
        }
    }

    /// A project's updates, newest first.
    pub fn updates(&self, key: &str) -> Result<Vec<Update>> {
        let p = self.node(key)?;
        self.store
            .updates(p.id)?
            .into_iter()
            .map(|(at, health, text)| {
                Ok(Update {
                    at,
                    health: health.parse()?,
                    text,
                })
            })
            .collect()
    }

    /// The states a node of `kind` waits on the human in: those with a move only the human may
    /// make (planned, for a task), review for a task (the human checks it and calls it done),
    /// and those a gate in the config moves from.
    pub fn waits_on_human(&self, kind: Kind) -> Vec<State> {
        let states = kind.states();
        let mut out: Vec<State> = states
            .iter()
            .copied()
            .filter(|&s| {
                states
                    .iter()
                    .any(|&to| kind.can_move(s, to) && kind.human_only(s, to))
            })
            .collect();
        if kind == Kind::Task {
            out.push(State::Review);
        }
        for g in &self.gates {
            if g.kind == kind
                && let Some(from) = g.from
                && !out.contains(&from)
            {
                out.push(from);
            }
        }
        out
    }
}
