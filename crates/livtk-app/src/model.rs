//! The app's state and every action it can take, without any UI. The views render this and
//! forward input to it, so all behaviour is tested as plain Rust.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use livtk_core::event::Event;
use livtk_core::{Actor, Brief, Command, Kind, Links, Node, Rel, State, Summary, Workspace};
use uuid::Uuid;

/// The views in the sidebar, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Inbox,
    Tasks,
    Lessons,
    Decisions,
    Findings,
}

impl View {
    pub const ALL: [View; 5] = [
        View::Inbox,
        View::Tasks,
        View::Lessons,
        View::Decisions,
        View::Findings,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            View::Inbox => "Inbox",
            View::Tasks => "Tasks",
            View::Lessons => "Lessons",
            View::Decisions => "Decisions",
            View::Findings => "Findings",
        }
    }

    /// The kind a view lists; the inbox mixes kinds.
    pub const fn kind(self) -> Option<Kind> {
        match self {
            View::Inbox => None,
            View::Tasks => Some(Kind::Task),
            View::Lessons => Some(Kind::Lesson),
            View::Decisions => Some(Kind::Decision),
            View::Findings => Some(Kind::Finding),
        }
    }
}

/// How the task view lays out its rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    List,
    Board,
}

/// Something the human can do to a node from where it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub label: &'static str,
    pub to: State,
    /// Ratify as a standing rule, shown at every session start.
    pub always: bool,
    /// A step only the human takes: acknowledging, ratifying, accepting, rejecting, retiring.
    pub decides: bool,
}

/// One open project.
pub struct Project {
    pub name: String,
    pub dir: PathBuf,
    pub inbox: usize,
    ws: Workspace,
}

/// The node open in the detail panel, with what surrounds it.
#[derive(Clone, Debug)]
pub struct Detail {
    pub node: Node,
    pub links: Links,
    pub replaces: Vec<Brief>,
    pub replaced_by: Vec<Brief>,
    pub learned_from: Vec<Brief>,
    pub history: Vec<Event>,
    pub choices: Vec<Choice>,
}

pub struct Model {
    pub projects: Vec<Project>,
    pub current: usize,
    pub view: View,
    pub layout: Layout,
    /// Include what is over: done, canceled, rejected, retired, superseded.
    pub show_all: bool,
    pub rows: Vec<Summary>,
    pub detail: Option<Detail>,
    /// Rows that changed under the app's feet (Claude, the CLI, a git pull), and when it noticed.
    pub flashed: HashMap<Uuid, Instant>,
    /// Rows the human just moved out of the current view, where they were, and when: the view
    /// fades them out in place.
    pub leaving: Vec<(Summary, usize, Instant)>,
    /// The last thing worth telling the human: a refusal, an error.
    pub notice: Option<String>,
    selected: Option<Uuid>,
}

impl Model {
    /// Open the given project directories. One that fails to open is reported, not fatal.
    pub fn open(dirs: &[PathBuf]) -> Model {
        let mut projects = Vec::new();
        let mut notice = None;
        for dir in dirs {
            match Workspace::open(dir) {
                Ok(ws) => {
                    let root = ws
                        .dir()
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| dir.clone());
                    if projects.iter().any(|p: &Project| p.dir == root) {
                        continue;
                    }
                    let name = root
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| root.display().to_string());
                    projects.push(Project {
                        name,
                        dir: root,
                        inbox: 0,
                        ws,
                    });
                }
                Err(e) => notice = Some(format!("{}: {e}", dir.display())),
            }
        }
        let mut model = Model {
            projects,
            current: 0,
            view: View::Inbox,
            layout: Layout::List,
            show_all: false,
            rows: Vec::new(),
            detail: None,
            flashed: HashMap::new(),
            leaving: Vec::new(),
            notice,
            selected: None,
        };
        model.reload();
        model
    }

    /// Open another project and switch to it. Returns whether it opened; an error becomes the
    /// notice.
    pub fn add_project(&mut self, dir: &Path) -> bool {
        let before = self.projects.len();
        let mut more = Model::open(&[dir.to_path_buf()]);
        if let Some(p) = more.projects.pop() {
            if let Some(i) = self.projects.iter().position(|q| q.dir == p.dir) {
                self.select_project(i);
                return true;
            }
            self.projects.push(p);
            self.select_project(before);
            true
        } else {
            self.notice = more.notice;
            false
        }
    }

    pub fn project(&self) -> Option<&Project> {
        self.projects.get(self.current)
    }

    pub fn select_project(&mut self, index: usize) {
        if index < self.projects.len() && index != self.current {
            self.current = index;
            self.selected = None;
            self.detail = None;
            self.leaving.clear();
            self.flashed.clear();
            self.reload();
        }
    }

    pub fn set_view(&mut self, view: View) {
        if view != self.view {
            self.view = view;
            self.selected = None;
            self.detail = None;
            self.leaving.clear();
            self.reload();
        }
    }

    pub fn toggle_layout(&mut self) {
        self.layout = match self.layout {
            Layout::List => Layout::Board,
            Layout::Board => Layout::List,
        };
    }

    pub fn toggle_show_all(&mut self) {
        self.show_all = !self.show_all;
        self.reload();
    }

    // ---- selection ----------------------------------------------------------------------------

    pub fn selected(&self) -> Option<&Summary> {
        let id = self.selected?;
        self.rows.iter().find(|r| r.id == id)
    }

    pub fn selected_index(&self) -> Option<usize> {
        let id = self.selected?;
        self.rows.iter().position(|r| r.id == id)
    }

    pub fn select(&mut self, id: Uuid) {
        if self.rows.iter().any(|r| r.id == id) {
            self.selected = Some(id);
            if self.detail.is_some() {
                self.open_detail();
            }
        }
    }

    /// Move the selection by `delta` rows, stopping at the ends.
    pub fn select_by(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let next = match self.selected_index() {
            None => 0,
            Some(i) => i.saturating_add_signed(delta).min(self.rows.len() - 1),
        };
        let id = self.rows[next].id;
        self.select(id);
    }

    // ---- the detail panel ---------------------------------------------------------------------

    pub fn open_detail(&mut self) {
        let Some(id) = self.selected else { return };
        let Some(project) = self.projects.get(self.current) else {
            return;
        };
        match load_detail(&project.ws, id) {
            Ok(d) => self.detail = Some(d),
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    pub fn close_detail(&mut self) {
        self.detail = None;
    }

    // ---- acting as the human ------------------------------------------------------------------

    /// Take a choice on the selected node.
    pub fn choose(&mut self, choice: &Choice) {
        let Some(row) = self.selected().cloned() else {
            return;
        };
        let cmd = if choice.to == State::Ratified {
            Command::Ratify {
                node: row.id.to_string(),
                always: choice.always,
            }
        } else {
            Command::Move {
                node: row.id.to_string(),
                to: choice.to,
            }
        };
        self.run(cmd, Some(row));
    }

    /// The first choice that decides (acknowledge, ratify, accept), for the one-key shortcut.
    pub fn decide(&mut self) {
        let choice = self
            .selected()
            .map(|r| choices(r.kind, r.state))
            .and_then(|c| {
                c.into_iter()
                    .find(|c| c.decides && !matches!(c.to, State::Rejected | State::Retired))
            });
        if let Some(c) = choice {
            self.choose(&c);
        }
    }

    pub fn set_priority(&mut self, priority: u8) {
        let Some(row) = self.selected().cloned() else {
            return;
        };
        if row.kind == Kind::Task {
            self.run(
                Command::SetPriority {
                    node: row.id.to_string(),
                    priority,
                },
                None,
            );
        }
    }

    pub fn add_note(&mut self, text: &str) {
        let Some(row) = self.selected().cloned() else {
            return;
        };
        if !text.trim().is_empty() {
            self.run(
                Command::Note {
                    node: row.id.to_string(),
                    text: text.to_owned(),
                },
                None,
            );
        }
    }

    pub fn create_task(&mut self, title: &str, body: &str, priority: Option<u8>) -> bool {
        let cmd = Command::Create {
            kind: Kind::Task,
            title: title.to_owned(),
            body: body.to_owned(),
            priority,
            links: Vec::<(Rel, String)>::new(),
        };
        let Some(project) = self.projects.get_mut(self.current) else {
            return false;
        };
        match project.ws.execute(cmd, &Actor::Human) {
            Ok(events) => {
                self.notice = None;
                let created = events.first().map(|e| e.op.subjects().0);
                if self.view != View::Tasks {
                    self.view = View::Tasks;
                    self.detail = None;
                }
                self.reload();
                if let Some(id) = created {
                    self.select(id);
                }
                true
            }
            Err(e) => {
                self.notice = Some(e.to_string());
                false
            }
        }
    }

    fn run(&mut self, cmd: Command, row: Option<Summary>) {
        let index = self.selected_index();
        let Some(project) = self.projects.get_mut(self.current) else {
            return;
        };
        match project.ws.execute(cmd, &Actor::Human) {
            Ok(_) => {
                self.notice = None;
                self.reload();
                // A row that left the view (acknowledged out of the inbox, say) fades out, and
                // the selection moves to the row that took its place.
                if let Some(row) = row
                    && !self.rows.iter().any(|r| r.id == row.id)
                {
                    self.leaving.push((row, index.unwrap_or(0), Instant::now()));
                    if let Some(i) = index
                        && !self.rows.is_empty()
                    {
                        let id = self.rows[i.min(self.rows.len() - 1)].id;
                        self.selected = Some(id);
                    }
                    if self.detail.is_some() {
                        self.open_detail();
                    }
                }
            }
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    // ---- staying current ----------------------------------------------------------------------

    /// Pick up changes made elsewhere. Cheap when nothing changed. Returns whether anything did.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        for p in &mut self.projects {
            // Any project counts: the sidebar shows every project's inbox.
            match p.ws.refresh() {
                Ok(changed_here) => changed |= changed_here,
                Err(e) => self.notice = Some(format!("{}: {e}", p.name)),
            }
        }
        if !changed {
            return false;
        }
        let before: HashMap<Uuid, String> = self
            .rows
            .iter()
            .map(|r| (r.id, r.updated.clone()))
            .collect();
        self.reload();
        let now = Instant::now();
        for r in &self.rows {
            if before.get(&r.id) != Some(&r.updated) {
                self.flashed.insert(r.id, now);
            }
        }
        true
    }

    /// Forget visual state older than `age`: finished flashes and fades.
    pub fn settle(&mut self, now: Instant, age: std::time::Duration) {
        self.flashed.retain(|_, at| now.duration_since(*at) < age);
        self.leaving
            .retain(|(_, _, at)| now.duration_since(*at) < age);
    }

    /// The task board: one column per state, in working order.
    pub fn columns(&self) -> Vec<(State, Vec<&Summary>)> {
        let mut states = vec![
            State::Backlog,
            State::Planned,
            State::Acked,
            State::Doing,
            State::Review,
            State::Done,
        ];
        if self.show_all {
            states.extend([State::Parked, State::Canceled]);
        } else {
            states.insert(5, State::Parked);
        }
        states
            .into_iter()
            .map(|s| (s, self.rows.iter().filter(|r| r.state == s).collect()))
            .collect()
    }

    fn reload(&mut self) {
        for p in &mut self.projects {
            p.inbox = p.ws.inbox().map(|i| i.len()).unwrap_or(0);
        }
        let Some(project) = self.projects.get(self.current) else {
            self.rows.clear();
            return;
        };
        let rows = match self.view.kind() {
            None => project.ws.inbox(),
            Some(kind) => {
                let states: Vec<State> = if self.show_all {
                    Vec::new()
                } else {
                    kind.states()
                        .iter()
                        .copied()
                        .filter(|s| s.is_active())
                        .collect()
                };
                project.ws.list(kind, &states)
            }
        };
        match rows {
            Ok(rows) => self.rows = rows,
            Err(e) => self.notice = Some(e.to_string()),
        }
        if self.selected().is_none() {
            self.selected = self.rows.first().map(|r| r.id);
        }
        if let Some(d) = &self.detail {
            let id = d.node.id;
            match load_detail(&project.ws, id) {
                Ok(d) => self.detail = Some(d),
                Err(_) => self.detail = None,
            }
        }
    }
}

/// What the human can do with a node of `kind` in `state`, most important first.
pub fn choices(kind: Kind, state: State) -> Vec<Choice> {
    let mut out = Vec::new();
    for &to in kind.next_states(state) {
        let decides = kind.human_only(state, to);
        let label = match (kind, to) {
            (Kind::Task, State::Acked) if state == State::Planned => "Acknowledge",
            (Kind::Task, State::Acked) => "Pause",
            (Kind::Task, State::Doing) if state == State::Acked => "Start",
            (Kind::Task, State::Doing) => "Back to doing",
            (Kind::Task, State::Review) => "Send to review",
            (Kind::Task, State::Done) => "Done",
            (Kind::Task, State::Planned) if state == State::Backlog => "Plan",
            (Kind::Task, State::Planned) => "Back to planned",
            (Kind::Task, State::Backlog) => "Back to backlog",
            (Kind::Task, State::Parked) => "Park",
            (Kind::Task, State::Canceled) => "Cancel",
            (_, State::Ratified) => "Ratify",
            (_, State::Accepted) => "Accept",
            (_, State::Rejected) => "Reject",
            (_, State::Retired) => "Retire",
            (_, State::Proposed) => "Propose again",
            (_, State::Superseded) => "Mark superseded",
            _ => to.as_str(),
        };
        out.push(Choice {
            label,
            to,
            always: false,
            decides,
        });
        if kind == Kind::Lesson && to == State::Ratified {
            out.push(Choice {
                label: "Ratify as standing rule",
                to,
                always: true,
                decides,
            });
        }
    }
    // Decisions first: they are what the human is here for.
    out.sort_by_key(|c| !c.decides || matches!(c.to, State::Rejected | State::Retired));
    out
}

fn load_detail(ws: &Workspace, id: Uuid) -> livtk_core::Result<Detail> {
    let key = id.to_string();
    let node = ws.node(&key)?;
    let (replaces, replaced_by) = ws.related(&key, Rel::Supersedes)?;
    let (learned_from, _) = ws.related(&key, Rel::LearnedFrom)?;
    Ok(Detail {
        choices: choices(node.kind, node.state),
        links: ws.links(&key)?,
        history: ws.history(Some(&key))?,
        replaces,
        replaced_by,
        learned_from,
        node,
    })
}
