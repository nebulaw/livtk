//! What a Claude session is called: the task it was about, what it did there, and who it was.
//! `TAS-110 · Build`, `TAS-110 · Review · refuter`, `TAS-106 +2 · Chat`. One name, worked out
//! from the log and the runs each time it is asked for, the same in the app and the CLI; nothing
//! is written into Claude Code's files. A name the human gave with `/rename` is kept.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::event::Op;
use crate::run::Stage;
use crate::{Kind, Result, State, Workspace};

/// How far back a session's events are read to name it: enough for any session's work.
const READ: usize = 2000;

/// What a session did, as its name says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Doing {
    Plan,
    Build,
    Review,
    Research,
    /// Talked, noted, looked: no stage's work.
    Chat,
}

impl Doing {
    pub const fn as_str(self) -> &'static str {
        match self {
            Doing::Plan => "Plan",
            Doing::Build => "Build",
            Doing::Review => "Review",
            Doing::Research => "Research",
            Doing::Chat => "Chat",
        }
    }

    pub fn parse(s: &str) -> Option<Doing> {
        [
            Doing::Plan,
            Doing::Build,
            Doing::Review,
            Doing::Research,
            Doing::Chat,
        ]
        .into_iter()
        .find(|d| d.as_str().eq_ignore_ascii_case(s.trim()))
    }

    fn of(stage: Stage) -> Doing {
        match stage {
            Stage::Plan => Doing::Plan,
            Stage::Build => Doing::Build,
            Stage::Review => Doing::Review,
            Stage::Research => Doing::Research,
        }
    }
}

/// How sessions' names read, from `[sessions]` in the project's config.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Naming {
    /// Between the name's parts.
    pub separator: String,
    /// The agent's short name at the end, when it is not livtk's own for the stage.
    pub agent: bool,
}

impl Default for Naming {
    fn default() -> Self {
        Naming {
            separator: " · ".to_owned(),
            agent: true,
        }
    }
}

impl Naming {
    pub fn is_default(&self) -> bool {
        *self == Naming::default()
    }
}

/// A session's name, and its parts, each to filter by.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SessionName {
    pub session: String,
    /// The task it was about: its run's, or the one it worked on most.
    pub task: Option<String>,
    /// How many other nodes it worked on besides.
    pub more: usize,
    pub doing: Doing,
    /// Its agent's short name, when not livtk's own for the stage.
    pub agent: Option<String>,
    /// The name the human gave it (`/rename`), kept as given.
    pub title: Option<String>,
    /// Its first prompt's first line, for a session that touched no node.
    pub prompt: Option<String>,
}

impl SessionName {
    /// How it reads: the human's name for it; else its task, what it did and who; else its first
    /// prompt's line; else its id.
    pub fn text(&self, naming: &Naming) -> String {
        if let Some(t) = &self.title {
            return t.clone();
        }
        if let Some(task) = &self.task {
            let mut parts = vec![if self.more > 0 {
                format!("{task} +{}", self.more)
            } else {
                task.clone()
            }];
            parts.push(self.doing.as_str().to_owned());
            if naming.agent
                && let Some(a) = &self.agent
            {
                parts.push(a.clone());
            }
            return parts.join(&naming.separator);
        }
        self.prompt
            .clone()
            .unwrap_or_else(|| format!("Session {}", crate::format::session(&self.session)))
    }

    /// Whether `query` finds it: by a word of its name, its task, what it did or its agent.
    pub fn matches(&self, naming: &Naming, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || self.text(naming).to_lowercase().contains(&q)
            || self
                .task
                .as_deref()
                .is_some_and(|t| t.eq_ignore_ascii_case(&q))
            || self.doing.as_str().eq_ignore_ascii_case(&q)
            || self
                .agent
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(&q))
    }
}

/// An agent's short name: `livtk:refuter` is `refuter`.
fn short(agent: &str) -> String {
    agent.rsplit(':').next().unwrap_or(agent).to_owned()
}

impl Workspace {
    /// What session `session` is called. `title` is the name the human gave it in Claude Code,
    /// `prompt` its first prompt, both from its transcript when there is one.
    pub fn session_name(
        &self,
        session: &str,
        title: Option<&str>,
        prompt: Option<&str>,
    ) -> Result<SessionName> {
        self.name_among(&self.runs()?, session, title, prompt)
    }

    /// The names of many sessions at once, each `(session, title, prompt)` as `session_name`
    /// takes them: the runs read once for all.
    pub fn session_names(
        &self,
        sessions: &[(String, Option<String>, Option<String>)],
    ) -> Result<Vec<SessionName>> {
        let runs = self.runs()?;
        sessions
            .iter()
            .map(|(s, t, p)| self.name_among(&runs, s, t.as_deref(), p.as_deref()))
            .collect()
    }

    fn name_among(
        &self,
        runs: &[crate::run::Run],
        session: &str,
        title: Option<&str>,
        prompt: Option<&str>,
    ) -> Result<SessionName> {
        let mut name = SessionName {
            session: session.to_owned(),
            task: None,
            more: 0,
            doing: Doing::Chat,
            agent: None,
            title: title
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned),
            prompt: prompt
                .and_then(|p| p.lines().find(|l| !l.trim().is_empty()))
                .map(|l| l.trim().chars().take(80).collect()),
        };
        let events = self.session_events(session, READ)?;
        // The nodes it worked on, by how often, tasks before the rest.
        let mut count: HashMap<uuid::Uuid, usize> = HashMap::new();
        for e in &events {
            *count.entry(e.op.subjects().0).or_default() += 1;
        }
        let mut worked: Vec<(crate::Node, usize)> = count
            .into_iter()
            .filter_map(|(id, n)| self.node(&id.to_string()).ok().map(|node| (node, n)))
            .collect();
        worked.sort_by(|(a, x), (b, y)| {
            (b.kind == Kind::Task)
                .cmp(&(a.kind == Kind::Task))
                .then(y.cmp(x))
                .then(a.key.cmp(&b.key))
        });
        // A run says what it is itself.
        if let Some(run) = runs.iter().rev().find(|r| r.session == session) {
            name.task = Some(run.key.clone());
            name.more = worked.iter().filter(|(n, _)| n.key != run.key).count();
            name.doing = Doing::of(run.stage);
            name.agent = run
                .agent
                .as_deref()
                .filter(|a| Some(*a) != run.stage.default_agent())
                .map(short);
            return Ok(name);
        }
        let Some((first, _)) = worked.first() else {
            return Ok(name);
        };
        name.task = Some(first.key.clone());
        name.more = worked.len() - 1;
        // What it did, by what it moved; a refuter's session reviewed. Its agent is the one that
        // wrote all of it: a session whose subagents wrote some is the session's own.
        let by = |e: &crate::event::Event| e.by.strip_prefix("claude:").map(str::to_owned);
        let agent = by(&events[0]).filter(|a| events.iter().all(|e| by(e).as_ref() == Some(a)));
        let moved: Vec<State> = events
            .iter()
            .filter_map(|e| match &e.op {
                Op::NodeSet { data, .. } => data.state,
                _ => None,
            })
            .collect();
        name.doing = if agent.as_deref().is_some_and(|a| short(a) == "refuter") {
            Doing::Review
        } else if agent.as_deref().is_some_and(|a| short(a) == "researcher") {
            Doing::Research
        } else if moved
            .iter()
            .any(|s| matches!(s, &State::Doing | &State::Review))
        {
            Doing::Build
        } else if moved.contains(&State::Planned) {
            Doing::Plan
        } else {
            Doing::Chat
        };
        // livtk's own agent for what it did goes unsaid, as a run's does.
        let own = match name.doing {
            Doing::Build => Stage::Build.default_agent(),
            Doing::Review => Stage::Review.default_agent(),
            Doing::Research => Stage::Research.default_agent(),
            Doing::Plan | Doing::Chat => None,
        };
        name.agent = agent
            .filter(|a| Some(a.as_str()) != own)
            .map(|a: String| short(&a));
        Ok(name)
    }
}
