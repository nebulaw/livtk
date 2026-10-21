//! The Agents place: the Claude sessions working in the project, and the agents they can run
//! as. The sessions come from outside (the window asks Claude Code for them off its thread);
//! the agents are files, read here.

use std::path::PathBuf;

use livtk_core::Activity;

use super::Model;
use crate::agents::{self, Agent, AgentFile, Place, Session};

/// What the Agents place holds.
#[derive(Debug, Default)]
pub struct AgentsPlace {
    /// The sessions working under the project's folder, Needs you first; `None` until asked.
    pub sessions: Option<Vec<Session>>,
    /// Why the sessions could not be read, said where they would be.
    pub sessions_error: Option<String>,
    /// The agent files, by place and name.
    pub files: Vec<AgentFile>,
    /// What the panel shows.
    pub open: Option<Pick>,
    /// Remove was pressed once: the next press removes.
    pub confirm_remove: bool,
}

/// What is open in the Agents place's panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// A session, by its key.
    Session(String),
    /// An agent, by its file.
    Agent(PathBuf),
    /// A new agent, from blank or a copy, to be saved in a place.
    New(Agent),
}

impl Model {
    /// Read the agent files again.
    pub fn load_agents(&mut self) {
        let Some(root) = self.project_root() else {
            self.agents.files.clear();
            return;
        };
        self.agents.files = agents::agents(&root);
        // An agent removed elsewhere: its page closes.
        if let Some(Pick::Agent(path)) = &self.agents.open
            && !self.agents.files.iter().any(|f| &f.path == path)
        {
            self.agents.open = None;
        }
    }

    /// The sessions as Claude Code listed them, or why it could not.
    pub fn set_sessions(&mut self, read: Result<Vec<Session>, String>) {
        match read {
            Ok(s) => {
                self.agents.sessions = Some(s);
                self.agents.sessions_error = None;
            }
            Err(e) => self.agents.sessions_error = Some(e),
        }
    }

    /// The project's folder, where its sessions work and its agents live.
    pub fn project_root(&self) -> Option<PathBuf> {
        self.project().map(|p| p.ws.root().to_path_buf())
    }

    pub fn pick_agent(&mut self, pick: Option<Pick>) {
        self.agents.open = pick;
        self.agents.confirm_remove = false;
    }

    /// The session open in the panel.
    pub fn picked_session(&self) -> Option<&Session> {
        let Some(Pick::Session(key)) = &self.agents.open else {
            return None;
        };
        self.agents
            .sessions
            .as_ref()?
            .iter()
            .find(|s| &s.key() == key)
    }

    /// The agent file open in the panel.
    pub fn picked_agent(&self) -> Option<&AgentFile> {
        let Some(Pick::Agent(path)) = &self.agents.open else {
            return None;
        };
        self.agents.files.iter().find(|f| &f.path == path)
    }

    /// Save `agent` in `place`, as new or `replacing` its file; the page then shows it. A refusal
    /// is said and nothing is written.
    pub fn save_agent(&mut self, place: Place, agent: &Agent, replacing: Option<PathBuf>) -> bool {
        let Some(root) = self.project_root() else {
            return false;
        };
        match agents::save(&root, place, agent, replacing.as_deref()) {
            Ok(path) => {
                self.load_agents();
                // Shown in the list at once, its page open: no toast needed.
                self.pick_agent(Some(Pick::Agent(path)));
                true
            }
            Err(e) => {
                self.warn(e);
                false
            }
        }
    }

    /// Remove the agent open in the panel: the first press asks, the second removes.
    pub fn remove_agent(&mut self) -> bool {
        let Some(file) = self.picked_agent().cloned() else {
            return false;
        };
        if !self.agents.confirm_remove {
            self.agents.confirm_remove = true;
            return false;
        }
        match agents::remove(&file) {
            Ok(()) => {
                self.load_agents();
                self.pick_agent(None);
                self.inform(format!("Removed {}.", file.agent.name));
                true
            }
            Err(e) => {
                self.agents.confirm_remove = false;
                self.warn(e);
                false
            }
        }
    }

    /// What livtk saw Claude do in this project since `session` started, newest first.
    pub fn session_activity(&self, session: &Session) -> Vec<Activity> {
        let Some(p) = self.project() else {
            return Vec::new();
        };
        let Ok(since) = jiff::Timestamp::from_millisecond(session.started_at) else {
            return Vec::new();
        };
        p.ws.activity(None, 200)
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.event.by.starts_with("claude"))
            .filter(|a| {
                a.event
                    .at
                    .parse::<jiff::Timestamp>()
                    .is_ok_and(|at| at >= since)
            })
            .take(20)
            .collect()
    }
}
