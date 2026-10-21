//! Projects: an outcome above tasks. Its progress is counted from the tasks under it, never
//! typed in; how it stands is written, in updates.

use std::collections::HashMap;

use jiff::Timestamp;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::Workspace;
use crate::model::{Health, Kind, State};
use crate::store::{Brief, Summary};
use crate::{Error, Result};

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

    /// The nearest project each node is part of; empty, and free, while there is none.
    pub fn projects_of(&self) -> Result<HashMap<Uuid, Brief>> {
        Ok(self.store.projects_of()?.into_iter().collect())
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
