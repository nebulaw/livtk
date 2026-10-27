//! Agents and Sessions: the agents Claude sessions can run as, and the Claude sessions of the
//! project. The sessions come from outside (the window asks Claude Code for them off its
//! thread); the agents are files, read here.

use std::collections::HashSet;
use std::path::PathBuf;

use livtk_core::Activity;
use livtk_core::claude::{Entry, Transcript};

use super::{Model, View};
use crate::agents::{self, Agent, AgentFile, Place, Session};

/// What Agents and Sessions hold.
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
    /// The project's session transcripts, the latest first (Claude Code's own files).
    pub transcripts: Vec<Transcript>,
    /// The session being read, in the middle.
    pub reader: Option<Reader>,
    /// The session rows as last worked out: kept until the log, the transcripts or Claude
    /// Code's live list change, so a draw never queries the store (TAS-119).
    rows: std::cell::RefCell<Option<Vec<SessionRow>>>,
}

impl AgentsPlace {
    /// What the session rows were worked out from has changed: work them out again when asked.
    pub fn forget_rows(&self) {
        self.rows.borrow_mut().take();
    }
}

/// A session as the list shows it: from its transcript, Claude Code's live list and the log,
/// whichever know it.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionRow {
    pub id: String,
    pub name: String,
    /// While it runs: where it stands.
    pub live: Option<agents::Standing>,
    pub background: bool,
    pub started: Option<String>,
    pub ended: Option<String>,
    /// Who wrote to livtk in it: `claude`, and its subagents by name.
    pub by: Vec<String>,
    /// How many nodes it wrote to.
    pub nodes: usize,
    /// Its transcript, while Claude Code keeps it.
    pub transcript: Option<PathBuf>,
    /// Its name's parts: its task, what it did, its agent, to find it by.
    pub parts: Option<livtk_core::sessions::SessionName>,
}

/// A session being read: its entries so far, where the next page starts, the calls opened.
#[derive(Clone, Debug, Default)]
pub struct Reader {
    pub session: String,
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub next: Option<u64>,
    /// A page is being read.
    pub loading: bool,
    /// The tool calls opened, by id; the rest show as one line.
    pub open: HashSet<String>,
    /// Read on to this time, and show it: a task's first event in the session.
    pub focus: Option<String>,
    /// The next page was asked for ("Read on").
    pub want: bool,
    /// The session this one was opened from (a subagent's, from its parent's call): Esc goes
    /// back to it, where it was.
    pub parent: Option<Box<Reader>>,
    /// Read from a little before `focus`, not from the start: found before the first page.
    pub seek: bool,
    /// Where the reading began, in bytes: past 0, what came before can be read too.
    pub begun: u64,
}

impl Reader {
    /// The entry `focus` points at, among those drawn (results are drawn under their calls):
    /// the last one at or before it, which is the call that wrote the event.
    pub fn focus_index(&self) -> Option<usize> {
        let f = self.focus.as_deref()?;
        self.drawn()
            .enumerate()
            .take_while(|(_, e)| e.at() <= f)
            .last()
            .map(|(i, _)| i)
    }

    /// The entries drawn as rows: all but the results, which show under their calls.
    pub fn drawn(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|e| !matches!(e, Entry::Result { .. }))
    }
}

/// How many entries a page of a session holds.
pub const PAGE: usize = 200;

/// What is open in the panel: an agent in Agents, a session in Sessions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// A session, by its key.
    Session(String),
    /// An agent, by its file.
    Agent(PathBuf),
    /// A new agent, from blank or a copy, to be saved in a place.
    New(Agent),
}

impl Pick {
    /// The view it is picked in: a session in Sessions, an agent in Agents.
    pub fn view(&self) -> View {
        match self {
            Pick::Session(_) => View::Sessions,
            Pick::Agent(_) | Pick::New(_) => View::Agents,
        }
    }
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
                self.agents.forget_rows();
                self.agents.sessions_error = None;
            }
            Err(e) => self.agents.sessions_error = Some(e),
        }
    }

    /// The project's transcripts, as listed off the window's thread.
    pub fn set_transcripts(&mut self, transcripts: Vec<Transcript>) {
        self.agents.transcripts = transcripts;
        self.agents.forget_rows();
    }

    /// Every session of the project: those Claude Code keeps a transcript of, those it lists as
    /// running, and those the log knows. Needs you first, then the running ones, then by when
    /// each was last at work.
    pub fn session_rows(&self) -> Vec<SessionRow> {
        if let Some(rows) = self.agents.rows.borrow().as_ref() {
            return rows.clone();
        }
        let rows = self.work_out_session_rows();
        *self.agents.rows.borrow_mut() = Some(rows.clone());
        rows
    }

    /// The session rows, worked out from the transcripts, the live list and the log.
    fn work_out_session_rows(&self) -> Vec<SessionRow> {
        let mut rows: Vec<SessionRow> = Vec::new();
        let find = |rows: &mut Vec<SessionRow>, id: &str| -> usize {
            match rows.iter().position(|r| r.id == id) {
                Some(i) => i,
                None => {
                    rows.push(SessionRow {
                        id: id.to_owned(),
                        name: format!("Session {}", livtk_core::format::session(id)),
                        live: None,
                        background: false,
                        started: None,
                        ended: None,
                        by: Vec::new(),
                        nodes: 0,
                        transcript: None,
                        parts: None,
                    });
                    rows.len() - 1
                }
            }
        };
        for t in &self.agents.transcripts {
            let i = find(&mut rows, &t.session);
            let r = &mut rows[i];
            r.name = t.name();
            r.started.clone_from(&t.started);
            r.ended.clone_from(&t.ended);
            r.transcript = Some(t.path.clone());
        }
        if let Some(p) = self.project() {
            for s in p.ws.sessions().unwrap_or_default() {
                let i = find(&mut rows, &s.session);
                let r = &mut rows[i];
                r.by = s.by;
                r.nodes = s.nodes;
                r.started.get_or_insert(s.first);
                if r.ended.as_ref().is_none_or(|e| *e < s.last) {
                    r.ended = Some(s.last);
                }
            }
        }
        for s in self.agents.sessions.iter().flatten() {
            let i = find(&mut rows, &s.key());
            let r = &mut rows[i];
            r.live = Some(s.standing());
            r.background = s.is_background();
            if r.transcript.is_none() {
                r.name = s.title();
            }
        }
        // Each named as livtk names sessions: its task, what it did, who; a name the human gave
        // kept. What Claude Code says of it is the fallback.
        if let Some(p) = self.project() {
            let asked: Vec<(String, Option<String>, Option<String>)> = rows
                .iter()
                .map(|r| {
                    let t = self.agents.transcripts.iter().find(|t| t.session == r.id);
                    (
                        r.id.clone(),
                        t.and_then(|t| t.title.clone()),
                        t.and_then(|t| t.first_prompt.clone())
                            .or_else(|| Some(r.name.clone())),
                    )
                })
                .collect();
            let naming = &p.ws.config().sessions;
            if let Ok(names) = p.ws.session_names(&asked) {
                for (r, n) in rows.iter_mut().zip(names) {
                    r.name = n.text(naming);
                    r.parts = Some(n);
                }
            }
        }
        rows.sort_by(|a, b| {
            let rank = |r: &SessionRow| r.live.map_or(9, |s| s as u8);
            rank(a).cmp(&rank(b)).then(b.ended.cmp(&a.ended))
        });
        rows
    }

    /// The sessions the filter finds: by a word of a session's name, its task, what it did or
    /// its agent. All of them while the filter is empty.
    pub fn found_sessions(&self) -> Vec<SessionRow> {
        let query = self.display().filter.clone();
        let naming = self
            .project()
            .map(|p| p.ws.config().sessions.clone())
            .unwrap_or_default();
        let mut rows = self.session_rows();
        rows.retain(|r| match &r.parts {
            Some(n) => n.matches(&naming, &query),
            None => r.name.to_lowercase().contains(&query.trim().to_lowercase()),
        });
        rows
    }

    /// Read the work of the subagent call `call` started, in the session being read; Esc comes
    /// back to the session, where it was.
    pub fn read_subagent(&mut self, call: &str) -> bool {
        let Some(parent) = self.agents.reader.take() else {
            return false;
        };
        let sub = self
            .agents
            .transcripts
            .iter()
            .find(|t| t.session == parent.session)
            .and_then(|t| {
                t.subagents
                    .iter()
                    .find(|s| s.tool_use_id.as_deref() == Some(call))
            })
            .cloned();
        let Some(sub) = sub else {
            self.agents.reader = Some(parent);
            self.warn("Claude Code no longer keeps this subagent's work.");
            return false;
        };
        self.agents.reader = Some(Reader {
            session: parent.session.clone(),
            path: sub.path,
            next: Some(0),
            parent: Some(Box::new(parent)),
            ..Reader::default()
        });
        true
    }

    /// The subagent a call in the session being read started, as the list of its subagents
    /// says: its kind and what it was asked to do.
    pub fn subagent_of(&self, call: &str) -> Option<livtk_core::claude::Subagent> {
        let r = self.agents.reader.as_ref()?;
        self.agents
            .transcripts
            .iter()
            .find(|t| t.session == r.session)?
            .subagents
            .iter()
            .find(|s| s.tool_use_id.as_deref() == Some(call))
            .cloned()
    }

    /// Read session `session` in the middle, from its start, or from a little before `focus` (a
    /// time in it). Its transcript is the one listed, or its file where Claude Code keeps it.
    pub fn read_session(&mut self, session: &str, focus: Option<String>) -> bool {
        let listed = self
            .agents
            .transcripts
            .iter()
            .find(|t| t.session == session)
            .map(|t| t.path.clone());
        let on_disk = || {
            let root = self.project_root()?;
            let home = agents::claude_home()?;
            let path =
                livtk_core::claude::transcripts_in(&home, &root).join(format!("{session}.jsonl"));
            path.is_file().then_some(path)
        };
        let Some(path) = listed.or_else(on_disk) else {
            self.warn("Claude Code no longer keeps this session's transcript.");
            return false;
        };
        self.agents.open = Some(Pick::Session(session.to_owned()));
        self.agents.reader = Some(Reader {
            session: session.to_owned(),
            path,
            next: Some(0),
            seek: focus.is_some(),
            focus,
            ..Reader::default()
        });
        true
    }

    /// Read the session open from its very start.
    pub fn read_from_start(&mut self) {
        if let Some(r) = self.agents.reader.as_mut() {
            r.entries.clear();
            r.next = Some(0);
            r.focus = None;
            r.seek = false;
            r.begun = 0;
            r.loading = false;
        }
    }

    /// A page read, from byte `from`: added to the reader's entries.
    pub fn add_page(&mut self, session: &str, from: u64, page: livtk_core::claude::Page) {
        if let Some(r) = self
            .agents
            .reader
            .as_mut()
            .filter(|r| r.session == session && r.loading)
        {
            if r.entries.is_empty() {
                r.begun = from;
            }
            r.entries.extend(page.entries);
            r.next = page.next;
            r.seek = false;
            r.loading = false;
            r.want = false;
        }
    }

    /// Whether the reader should read on: a page is wanted, none is being read, and the focus
    /// (if any) is not reached yet, or nothing is read yet.
    pub fn wants_page(&self) -> Option<(PathBuf, u64, Option<String>)> {
        let r = self.agents.reader.as_ref()?;
        if r.loading {
            return None;
        }
        let next = r.next?;
        // Read until past the focus, so the entry at it is the last one before it.
        let reached = r
            .focus
            .as_ref()
            .is_none_or(|f| r.entries.last().is_some_and(|e| e.at() > f.as_str()));
        let seek = r.seek.then(|| r.focus.clone()).flatten();
        (r.entries.is_empty() || r.want || !reached).then(|| (r.path.clone(), next, seek))
    }

    /// Mark a page as being read, so it is asked for once.
    pub fn page_asked(&mut self) {
        if let Some(r) = self.agents.reader.as_mut() {
            r.loading = true;
        }
    }

    /// Read the next page of the session open.
    pub fn read_on(&mut self) {
        if let Some(r) = self.agents.reader.as_mut() {
            r.want = r.next.is_some();
        }
    }

    /// Open or fold tool call `id` in the reader.
    pub fn toggle_call(&mut self, id: &str) {
        if let Some(r) = self.agents.reader.as_mut()
            && !r.open.remove(id)
        {
            r.open.insert(id.to_owned());
        }
    }

    /// Stop reading: a subagent's work gives way to its session; a session, to the list.
    pub fn close_reader(&mut self) -> bool {
        match self.agents.reader.take() {
            Some(r) => {
                self.agents.reader = r.parent.map(|p| *p);
                true
            }
            None => false,
        }
    }

    /// The project's folder, where its sessions work and its agents live.
    pub fn project_root(&self) -> Option<PathBuf> {
        self.project().map(|p| p.ws.root().to_path_buf())
    }

    /// Esc in `view`: in Sessions, the session read gives way to the list, then its pick to
    /// nothing; in Agents, the agent's pick. Whether there was anything to close there.
    pub fn agents_back(&mut self, view: View) -> bool {
        if view == View::Sessions && self.close_reader() {
            return true;
        }
        if self.agents_pick_in(view).is_some() {
            self.pick_agent(None);
            return true;
        }
        false
    }

    /// What is picked in `view`'s panel, if anything.
    pub fn agents_pick_in(&self, view: View) -> Option<&Pick> {
        self.agents.open.as_ref().filter(|p| p.view() == view)
    }

    pub fn pick_agent(&mut self, pick: Option<Pick>) {
        self.agents.open = pick;
        self.agents.confirm_remove = false;
    }

    /// The session open in the panel, as the list shows it.
    pub fn picked_row(&self) -> Option<SessionRow> {
        let Some(Pick::Session(key)) = &self.agents.open else {
            return None;
        };
        self.session_rows().into_iter().find(|r| &r.id == key)
    }

    /// The session open in the panel, while Claude Code lists it as running.
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

    /// What session `id` did in livtk, newest first: as the log records it.
    pub fn session_activity(&self, id: &str) -> Vec<Activity> {
        self.project()
            .and_then(|p| p.ws.session_activity(id, 30).ok())
            .unwrap_or_default()
    }

    /// The nodes session `id` wrote to, in the order it first did.
    pub fn session_nodes(&self, id: &str) -> Vec<livtk_core::Brief> {
        self.project()
            .and_then(|p| p.ws.session_nodes(id).ok())
            .unwrap_or_default()
    }
}
