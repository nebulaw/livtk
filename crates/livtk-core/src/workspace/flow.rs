//! Flow numbers: how long tasks spend in each state, how long what waits on the human has
//! waited, and a project's lead time. Measured from the log, never estimated.

use std::collections::HashMap;

use jiff::Timestamp;
use serde::Serialize;
use uuid::Uuid;

use super::Workspace;
use crate::model::{Kind, State};
use crate::{Error, Result};

/// Flow over a set of tasks, as of a moment.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Flow {
    /// For each state tasks have been in, in working order: how many tasks were there, and the
    /// median and total of the time each spent there, in seconds. Time in a state a task is
    /// still in counts up to now; time once done or canceled does not count.
    pub states: Vec<StateTime>,
    /// What waits on the human now, by the state it waits in: how many, and the longest wait.
    pub queues: Vec<Queue>,
    /// A project's: from the first of its tasks starting to its being done (or now).
    pub lead: Option<Lead>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StateTime {
    pub state: State,
    pub tasks: usize,
    pub median_secs: i64,
    pub total_secs: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Queue {
    pub state: State,
    pub count: usize,
    pub oldest: String,
    pub oldest_secs: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Lead {
    pub started: String,
    /// When the project was done; `None` while it is not.
    pub finished: Option<String>,
    pub secs: i64,
}

impl Workspace {
    /// Flow over every task, or over the tasks under project `project` (nested ones too).
    pub fn flow(&self, project: Option<&str>, now: Timestamp) -> Result<Flow> {
        let (scope, lead) = match project {
            Some(key) => {
                let p = self.node(key)?;
                if p.kind != Kind::Project {
                    return Err(Error::Invalid(format!(
                        "{} is a {}, not a project",
                        p.key, p.kind
                    )));
                }
                let tasks: Vec<Uuid> = self
                    .store
                    .parts_under(p.id)?
                    .into_iter()
                    .filter(|s| s.kind == Kind::Task)
                    .map(|s| s.id)
                    .collect();
                (Some(tasks), Some(p))
            }
            None => (None, None),
        };
        let changes = self.store.state_changes(Kind::Task)?;
        // Each task's states in order, with when it got there.
        let mut by_task: HashMap<Uuid, Vec<(State, Timestamp)>> = HashMap::new();
        for (node, state, at) in changes {
            if scope.as_ref().is_some_and(|s| !s.contains(&node)) {
                continue;
            }
            let Ok(at) = at.parse::<Timestamp>() else {
                continue;
            };
            by_task.entry(node).or_default().push((state, at));
        }
        // Time per task per state.
        let mut spent: HashMap<State, Vec<i64>> = HashMap::new();
        let mut started: Option<Timestamp> = None;
        for steps in by_task.values() {
            let mut per: HashMap<State, i64> = HashMap::new();
            for (i, &(state, at)) in steps.iter().enumerate() {
                if state == State::Doing && started.is_none_or(|s| at < s) {
                    started = Some(at);
                }
                if Kind::Task.is_closed(state) {
                    continue;
                }
                let until = steps.get(i + 1).map_or(now, |&(_, t)| t);
                *per.entry(state).or_default() += (until.as_second() - at.as_second()).max(0);
            }
            for (state, secs) in per {
                spent.entry(state).or_default().push(secs);
            }
        }
        let states: Vec<StateTime> = Kind::Task
            .states()
            .iter()
            .filter_map(|&s| {
                let mut times = spent.remove(&s)?;
                times.sort_unstable();
                Some(StateTime {
                    state: s,
                    tasks: times.len(),
                    median_secs: median(&times),
                    total_secs: times.iter().sum(),
                })
            })
            .collect();
        // What waits on the human now.
        let mut queues = Vec::new();
        for state in self.waits_on_human(Kind::Task) {
            let mut oldest: Option<(i64, Uuid)> = None;
            let mut count = 0;
            for (id, steps) in &by_task {
                let Some(&(at_state, at)) = steps.last() else {
                    continue;
                };
                if at_state != state {
                    continue;
                }
                count += 1;
                let secs = now.as_second() - at.as_second();
                if oldest.is_none_or(|(s, _)| secs > s) {
                    oldest = Some((secs, *id));
                }
            }
            if let Some((secs, id)) = oldest {
                let key = self.store.key_of(&id.to_string())?.unwrap_or_default();
                queues.push(Queue {
                    state,
                    count,
                    oldest: key,
                    oldest_secs: secs,
                });
            }
        }
        let lead = match (lead, started) {
            (Some(p), Some(start)) => {
                let finished = if p.state == State::Done {
                    self.store
                        .entered(p.id)?
                        .and_then(|a| a.parse::<Timestamp>().ok())
                } else {
                    None
                };
                let end = finished.unwrap_or(now);
                Some(Lead {
                    started: start.to_string(),
                    finished: finished.map(|f| f.to_string()),
                    secs: (end.as_second() - start.as_second()).max(0),
                })
            }
            _ => None,
        };
        Ok(Flow {
            states,
            queues,
            lead,
        })
    }
}

/// The middle of `sorted`, or the mean of its two middle values when its count is even.
fn median(sorted: &[i64]) -> i64 {
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2
    }
}
