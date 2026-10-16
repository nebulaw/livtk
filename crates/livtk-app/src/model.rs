//! The app's state and every action it can take, without any UI. The views render this and
//! forward input to it, so all behaviour is tested as plain Rust.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use livtk_core::event::Event;
use livtk_core::{Actor, Brief, Command, Gate, Kind, Links, Node, Rel, State, Summary, Workspace};
use uuid::Uuid;

mod activity;
mod flow;
mod graph;
mod palette;
mod saved;
pub use activity::{FeedRow, created_as, day_label, describe};
pub use flow::{
    Arrow, Look, Step, arrows, arrows_with, flow_of, has_flow, state_name, state_short, steps,
    steps_with,
};
pub use graph::{
    CARD_H, CARD_W, Camera, DOTS_BELOW, FIT_BELOW, Graph, GraphEdge, GraphNode, MAX_ZOOM, MIN_ZOOM,
    link_curve, visible_runs,
};
pub use palette::{Action, Ask, Found, Item, LinkPick, Palette, Run, fuzzy};
pub use saved::{Saved, SavedView, SavedWindow};

/// The views in the sidebar, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Inbox,
    /// Every change in the project, newest first.
    Activity,
    Tasks,
    Lessons,
    Decisions,
    Findings,
    /// Every node and link, laid out.
    Graph,
}

impl View {
    pub const ALL: [View; 7] = [
        View::Inbox,
        View::Activity,
        View::Tasks,
        View::Lessons,
        View::Decisions,
        View::Findings,
        View::Graph,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            View::Inbox => "Inbox",
            View::Activity => "Activity",
            View::Tasks => "Tasks",
            View::Lessons => "Lessons",
            View::Decisions => "Decisions",
            View::Findings => "Findings",
            View::Graph => "Graph",
        }
    }

    /// The kind a view lists; the inbox mixes kinds, and the activity feed lists events.
    pub const fn kind(self) -> Option<Kind> {
        match self {
            View::Inbox | View::Activity | View::Graph => None,
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

/// How a list groups its rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grouping {
    State,
    Priority,
    None,
}

/// How rows are ordered inside a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ordering {
    Priority,
    Updated,
    Created,
}

/// A view's display options, like Linear's Display menu. Kept per view while the app runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Display {
    /// Shown only when the key or title contains this, ignoring case.
    pub filter: String,
    /// Shown only with one of these priorities; empty shows every priority.
    pub priorities: Vec<u8>,
    pub grouping: Grouping,
    pub ordering: Ordering,
}

impl Display {
    fn for_view(view: View) -> Display {
        Display {
            filter: String::new(),
            priorities: Vec::new(),
            grouping: if matches!(view, View::Inbox | View::Activity | View::Graph) {
                Grouping::None
            } else {
                Grouping::State
            },
            ordering: Ordering::Priority,
        }
    }
}

/// What a group gathers: a state, a priority, or everything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupKey {
    State(State),
    Priority(u8),
    All,
}

/// One group of the list as shown: its rows are `rows[start..start + shown]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub key: GroupKey,
    /// The kind its rows are, for the state mark; `None` in the mixed inbox.
    pub kind: Option<Kind>,
    /// Rows in the group after filtering, shown or collapsed.
    pub count: usize,
    pub collapsed: bool,
    pub start: usize,
    pub shown: usize,
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
    /// Events by others since the human last looked at the activity feed, up to a cap.
    pub unseen: usize,
    /// Whether its store is being brought up to date with its log on another thread.
    pub catching_up: bool,
    ws: Workspace,
}

/// Bringing a project's store up to date with its log: after a git pull, checkout or rebase,
/// a full replay of the log. Run off the window's thread, it opens a workspace of its own (its
/// own connection to the store), so the window keeps showing what it had until it is done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatchUp {
    /// The project, as `Project::dir` names it.
    pub project: PathBuf,
    /// Its `.livtk` directory.
    livtk: PathBuf,
}

impl CatchUp {
    /// Do it: open the workspace, which catches up under the write lock, and close it.
    pub fn run(&self) -> Result<(), String> {
        Workspace::open_dir(&self.livtk)
            .map(drop)
            .map_err(|e| e.to_string())
    }
}

/// The node open in the detail panel, with what surrounds it.
#[derive(Clone, Debug)]
pub struct Detail {
    pub node: Node,
    pub links: Links,
    pub replaces: Vec<Brief>,
    pub replaced_by: Vec<Brief>,
    pub learned_from: Vec<Brief>,
    /// Every link, grouped as the panel shows them, each group knowing its relation and its
    /// direction so a link can be removed from it.
    pub link_groups: Vec<LinkGroup>,
    pub history: Vec<Event>,
    pub choices: Vec<Choice>,
}

/// Links of one relation and direction, as the detail panel lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkGroup {
    pub label: &'static str,
    pub rel: Rel,
    /// The open node is the link's source (it waits for, learned from, supersedes, relates to).
    pub outgoing: bool,
    pub briefs: Vec<Brief>,
}

/// A link the human can add from a node: how it reads, and which relation and direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkChoice {
    pub label: &'static str,
    pub rel: Rel,
    pub outgoing: bool,
}

/// The links a node of `kind` can gain, as the panel and the palette offer them.
pub fn link_choices(kind: Kind) -> Vec<LinkChoice> {
    let c = |label, rel, outgoing| LinkChoice {
        label,
        rel,
        outgoing,
    };
    let mut out = match kind {
        Kind::Task => vec![
            c("Waits for…", Rel::DependsOn, true),
            c("Blocks…", Rel::DependsOn, false),
            c("Part of…", Rel::Parent, true),
            c("Add sub-task…", Rel::Parent, false),
        ],
        Kind::Lesson | Kind::Decision => vec![
            c("Learned from…", Rel::LearnedFrom, true),
            c("Supersedes…", Rel::Supersedes, true),
        ],
        Kind::Finding => vec![c("Learned from…", Rel::LearnedFrom, true)],
    };
    out.push(c("Related to…", Rel::Relates, true));
    out
}

pub struct Model {
    pub projects: Vec<Project>,
    pub current: usize,
    pub view: View,
    pub layout: Layout,
    /// Include what was dismissed: canceled, rejected, retired, superseded. Done work is always
    /// shown.
    pub show_all: bool,
    /// The rows shown, in display order: filtered, grouped, ordered, collapsed groups left out.
    pub rows: Vec<Summary>,
    /// The groups of `rows`, in order.
    pub groups: Vec<Group>,
    /// Every row after filtering, collapsed or not: what the board shows.
    pub filtered: Vec<Summary>,
    pub detail: Option<Detail>,
    /// Rows that changed under the app's feet (Claude, the CLI, a git pull), and when it noticed.
    pub flashed: HashMap<Uuid, Instant>,
    /// Rows the human just moved out of the current view, where they were, and when: the view
    /// fades them out in place.
    pub leaving: Vec<(Summary, usize, Instant)>,
    /// The last thing worth telling the human: a refusal, an error.
    pub notice: Option<String>,
    /// Catching up the window must not wait for, found by `tick`, for the view to run elsewhere.
    catch_ups: Vec<CatchUp>,
    /// The command palette, while it is open.
    pub palette: Option<Palette>,
    /// The activity feed's events, newest first, as far as its pages are loaded.
    pub activity: Vec<livtk_core::Activity>,
    /// Whether older events wait for "Show older".
    pub activity_more: bool,
    /// The highlighted event of the feed.
    pub activity_index: usize,
    /// The mark as it was when the feed was opened: what came after it is new on this visit.
    activity_seen_before: Option<Uuid>,
    /// The graph view's nodes, links, camera and selection.
    pub graph: Graph,
    selected: Option<Uuid>,
    /// Everything the view holds, before display options.
    all: Vec<Summary>,
    displays: HashMap<View, Display>,
    collapsed: HashSet<(View, GroupKey)>,
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
                        unseen: 0,
                        catching_up: false,
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
            groups: Vec::new(),
            filtered: Vec::new(),
            detail: None,
            flashed: HashMap::new(),
            leaving: Vec::new(),
            notice,
            catch_ups: Vec::new(),
            palette: None,
            activity: Vec::new(),
            activity_more: false,
            activity_index: 0,
            activity_seen_before: None,
            graph: Graph::default(),
            selected: None,
            all: Vec::new(),
            displays: HashMap::new(),
            collapsed: HashSet::new(),
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

    /// Rank search by meaning with models from `dir`, in every open project; `None` ranks by
    /// keywords only.
    pub fn set_models_dir(&mut self, dir: Option<PathBuf>) {
        for p in &mut self.projects {
            p.ws.set_models_dir(dir.clone());
        }
    }

    pub fn project(&self) -> Option<&Project> {
        self.projects.get(self.current)
    }

    /// The gates the open project's config adds to the built-in ones.
    pub fn gates(&self) -> &[Gate] {
        self.project().map_or(&[], |p| p.ws.gates())
    }

    /// The moves a `kind` in `state` can make in the open project, its gates counted.
    pub fn choices(&self, kind: Kind, state: State) -> Vec<Choice> {
        choices_with(kind, state, self.gates())
    }

    pub fn select_project(&mut self, index: usize) {
        if index < self.projects.len() && index != self.current {
            self.current = index;
            self.selected = None;
            self.detail = None;
            self.leaving.clear();
            self.flashed.clear();
            self.enter_activity();
            self.enter_graph();
            self.reload();
        }
    }

    pub fn set_view(&mut self, view: View) {
        if view != self.view {
            self.view = view;
            self.selected = None;
            self.detail = None;
            self.leaving.clear();
            self.enter_activity();
            self.enter_graph();
            self.reload();
        }
    }

    pub fn toggle_layout(&mut self) {
        self.layout = match self.layout {
            Layout::List => Layout::Board,
            Layout::Board => Layout::List,
        };
        // A board needs columns: as in Linear, it is never ungrouped.
        if self.layout == Layout::Board && self.display().grouping == Grouping::None {
            self.set_grouping(Grouping::State);
        }
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
        if self.view == View::Activity {
            self.activity_select_by(delta);
            return;
        }
        if self.view == View::Graph {
            self.graph_select_by(delta);
            return;
        }
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

    /// The node actions act on: the selected row, or, where no row is selected (the activity
    /// feed), the node open in the detail panel.
    pub fn acted_on(&self) -> Option<Summary> {
        if let Some(row) = self.selected() {
            return Some(row.clone());
        }
        let n = &self.detail.as_ref()?.node;
        Some(Summary {
            id: n.id,
            key: n.key.clone(),
            kind: n.kind,
            title: n.title.clone(),
            state: n.state,
            priority: n.priority,
            updated: n.updated.clone(),
            blocked: n.blocked,
            standing: n.props.get("scope").and_then(|v| v.as_str()) == Some("always"),
        })
    }

    // ---- the detail panel ---------------------------------------------------------------------

    pub fn open_detail(&mut self) {
        if self.view == View::Activity {
            // The feed's highlighted event opens its node, and the feed stays.
            self.selected = self.activity_node();
        }
        if self.view == View::Graph {
            self.selected = self.graph_node();
        }
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
        let Some(row) = self.acted_on() else {
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
            .acted_on()
            .map(|r| self.choices(r.kind, r.state))
            .and_then(|c| {
                c.into_iter()
                    .find(|c| c.decides && !matches!(c.to, State::Rejected | State::Retired))
            });
        if let Some(c) = choice {
            self.choose(&c);
        }
    }

    pub fn set_priority(&mut self, priority: u8) {
        let Some(row) = self.acted_on() else {
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

    /// Change a node's title or body. A changed plan that was acknowledged, or a changed rule,
    /// goes back for approval: the core's rule, as from the CLI.
    pub fn edit(&mut self, id: Uuid, title: Option<&str>, body: Option<&str>) {
        self.run(
            Command::Edit {
                node: id.to_string(),
                title: title.map(str::to_owned),
                body: body.map(str::to_owned),
            },
            None,
        );
    }

    /// Set a task's priority wherever it shows: a list row, a board card, the panel.
    pub fn set_priority_of(&mut self, id: Uuid, priority: u8) {
        self.run(
            Command::SetPriority {
                node: id.to_string(),
                priority,
            },
            None,
        );
    }

    /// Link node `id` and `other` by `choice`: from `id` when outgoing, else to it. A refusal
    /// (a cycle, say) becomes the notice.
    pub fn link(&mut self, id: Uuid, choice: LinkChoice, other: Uuid) {
        let (src, dst) = if choice.outgoing {
            (id, other)
        } else {
            (other, id)
        };
        self.run(
            Command::Link {
                src: src.to_string(),
                rel: choice.rel,
                dst: dst.to_string(),
            },
            None,
        );
    }

    /// Remove the link between the open node and `other`, as listed in a group of the panel.
    pub fn unlink(&mut self, rel: Rel, outgoing: bool, other: &str) {
        let Some(id) = self.detail.as_ref().map(|d| d.node.id.to_string()) else {
            return;
        };
        let (src, dst) = if outgoing {
            (id, other.to_owned())
        } else {
            (other.to_owned(), id)
        };
        self.run(Command::Unlink { src, rel, dst }, None);
    }

    pub fn add_note(&mut self, text: &str) {
        let Some(row) = self.acted_on() else {
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
        // Only a row that was on screen can fade out of it, from where it was.
        let at = row
            .as_ref()
            .and_then(|r| self.rows.iter().position(|x| x.id == r.id));
        let row = row.filter(|_| at.is_some());
        let was_selected = row.as_ref().is_some_and(|r| self.selected == Some(r.id));
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
                    self.leaving.push((row, at.unwrap_or(0), Instant::now()));
                    // The selection moves on only if it was on the row that left.
                    if was_selected
                        && let Some(i) = at
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
    /// It never catches a store up itself: a project whose log is ahead of its store (git
    /// rewrote it) is left as it was shown, and a `CatchUp` for it waits in `take_catch_ups`.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        // A catch-up just begun changes only what the sidebar says: a repaint, no reload.
        let mut began = false;
        for p in &mut self.projects {
            if p.catching_up {
                continue;
            }
            // Any project counts: the sidebar shows every project's inbox.
            match p.ws.stale() {
                Ok(true) => {
                    p.catching_up = true;
                    self.catch_ups.push(CatchUp {
                        project: p.dir.clone(),
                        livtk: p.ws.dir().to_path_buf(),
                    });
                    began = true;
                    continue;
                }
                Ok(false) => {}
                Err(e) => {
                    self.notice = Some(format!("{}: {e}", p.name));
                    continue;
                }
            }
            match p.ws.refresh() {
                Ok(changed_here) => changed |= changed_here,
                Err(e) => self.notice = Some(format!("{}: {e}", p.name)),
            }
        }
        if !changed {
            return began;
        }
        let before: HashMap<Uuid, String> = self
            .rows
            .iter()
            .map(|r| (r.id, r.updated.clone()))
            .collect();
        let newest = self.activity.first().map(|a| a.event.id);
        self.reload();
        let now = Instant::now();
        // Events that arrived while the feed is open flash once, as changed rows do.
        for a in &self.activity {
            if newest.is_some_and(|n| a.event.id > n) {
                self.flashed.insert(a.event.id, now);
            }
        }
        for r in &self.rows {
            if before.get(&r.id) != Some(&r.updated) {
                self.flashed.insert(r.id, now);
            }
        }
        true
    }

    /// The catching up `tick` found and left to be run elsewhere, each once.
    pub fn take_catch_ups(&mut self) -> Vec<CatchUp> {
        std::mem::take(&mut self.catch_ups)
    }

    /// A `CatchUp` finished with `result`: the project is shown from its store again, which the
    /// next `tick` finds current and changed.
    pub fn caught_up(&mut self, job: &CatchUp, result: Result<(), String>) {
        if let Some(p) = self.projects.iter_mut().find(|p| p.dir == job.project) {
            p.catching_up = false;
            if let Err(e) = result {
                self.notice = Some(format!("{}: {e}", p.name));
            }
        }
    }

    /// Forget visual state older than `age`: finished flashes and fades.
    pub fn settle(&mut self, now: Instant, age: std::time::Duration) {
        self.flashed.retain(|_, at| now.duration_since(*at) < age);
        self.leaving
            .retain(|(_, _, at)| now.duration_since(*at) < age);
    }

    // ---- display options -----------------------------------------------------------------------

    pub fn display(&self) -> Display {
        self.displays
            .get(&self.view)
            .cloned()
            .unwrap_or_else(|| Display::for_view(self.view))
    }

    fn edit_display(&mut self, edit: impl FnOnce(&mut Display)) {
        let mut d = self.display();
        edit(&mut d);
        self.displays.insert(self.view, d);
        self.arrange();
        if self.selected().is_none() {
            self.selected = self.rows.first().map(|r| r.id);
        }
    }

    pub fn set_filter(&mut self, text: &str) {
        self.edit_display(|d| d.filter = text.to_owned());
    }

    pub fn toggle_priority(&mut self, priority: u8) {
        self.edit_display(|d| {
            if let Some(i) = d.priorities.iter().position(|p| *p == priority) {
                d.priorities.remove(i);
            } else {
                d.priorities.push(priority);
                d.priorities.sort_unstable();
            }
        });
    }

    pub fn set_grouping(&mut self, grouping: Grouping) {
        if self.layout == Layout::Board && grouping == Grouping::None {
            return;
        }
        self.edit_display(|d| d.grouping = grouping);
    }

    pub fn set_ordering(&mut self, ordering: Ordering) {
        self.edit_display(|d| d.ordering = ordering);
    }

    pub fn toggle_collapsed(&mut self, key: GroupKey) {
        if !self.collapsed.remove(&(self.view, key)) {
            self.collapsed.insert((self.view, key));
        }
        self.arrange();
        if self.selected().is_none() {
            self.selected = self.rows.first().map(|r| r.id);
        }
    }

    /// Apply the view's display options to everything it holds.
    fn arrange(&mut self) {
        let d = self.display();
        let needle = d.filter.trim().to_lowercase();
        let mut rows: Vec<Summary> = self
            .all
            .iter()
            .filter(|r| {
                needle.is_empty()
                    || r.title.to_lowercase().contains(&needle)
                    || r.key.to_lowercase().contains(&needle)
            })
            .filter(|r| {
                d.priorities.is_empty()
                    || r.kind != Kind::Task
                    || d.priorities.contains(&r.priority)
            })
            .cloned()
            .collect();
        let group_of = |r: &Summary| match d.grouping {
            Grouping::State => GroupKey::State(r.state),
            Grouping::Priority => GroupKey::Priority(r.priority),
            Grouping::None => GroupKey::All,
        };
        let group_rank = |k: GroupKey| match k {
            GroupKey::State(s) => s.rank(),
            GroupKey::Priority(p) => p,
            GroupKey::All => 0,
        };
        rows.sort_by(|a, b| {
            group_rank(group_of(a))
                .cmp(&group_rank(group_of(b)))
                .then_with(|| match d.ordering {
                    Ordering::Priority => a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)),
                    Ordering::Updated => b.updated.cmp(&a.updated).then(b.id.cmp(&a.id)),
                    Ordering::Created => b.id.cmp(&a.id),
                })
        });
        let mut groups: Vec<Group> = Vec::new();
        let mut shown: Vec<Summary> = Vec::with_capacity(rows.len());
        for r in &rows {
            let key = group_of(r);
            if groups.last().map(|g| g.key) != Some(key) {
                let collapsed = self.collapsed.contains(&(self.view, key));
                groups.push(Group {
                    key,
                    kind: self.view.kind(),
                    count: 0,
                    collapsed,
                    start: shown.len(),
                    shown: 0,
                });
            }
            if let Some(g) = groups.last_mut() {
                g.count += 1;
                if !g.collapsed {
                    g.shown += 1;
                    shown.push(r.clone());
                }
            }
        }
        self.filtered = rows;
        self.rows = shown;
        self.groups = groups;
    }

    // ---- drag and drop ------------------------------------------------------------------------

    /// Whether dropping node `id` on `target` is a legal change.
    pub fn can_drop(&self, id: Uuid, target: GroupKey) -> bool {
        self.all
            .iter()
            .find(|r| r.id == id)
            .is_some_and(|r| drop_allowed(r.kind, r.state, r.priority, target))
    }

    /// Drop node `id` on a group or column: move it to that state, or give it that priority.
    /// A move the rules refuse becomes the notice, with the core's reason.
    pub fn drop_on(&mut self, id: Uuid, target: GroupKey) {
        let Some(r) = self.all.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        self.selected = Some(id);
        let node = id.to_string();
        let cmd = match target {
            GroupKey::State(s) if s == r.state => return,
            GroupKey::State(State::Ratified) => Command::Ratify {
                node,
                always: false,
            },
            GroupKey::State(to) => Command::Move { node, to },
            GroupKey::Priority(p) if r.kind == Kind::Task && p != r.priority => {
                Command::SetPriority { node, priority: p }
            }
            _ => return,
        };
        self.run(cmd, Some(r));
    }

    /// The task board: one column per group, as the grouping says (states in working order,
    /// or priorities from urgent to low), each in the chosen order.
    pub fn columns(&self) -> Vec<(GroupKey, Vec<&Summary>)> {
        if self.display().grouping == Grouping::Priority {
            return (1..=4u8)
                .map(|p| {
                    let rows = self.filtered.iter().filter(|r| r.priority == p).collect();
                    (GroupKey::Priority(p), rows)
                })
                .collect();
        }
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
            .map(|s| {
                let rows = self.filtered.iter().filter(|r| r.state == s).collect();
                (GroupKey::State(s), rows)
            })
            .collect()
    }

    fn reload(&mut self) {
        if self.view == View::Activity {
            self.load_activity();
        }
        if self.view == View::Graph {
            self.load_graph();
        }
        for p in &mut self.projects {
            p.inbox = p.ws.inbox().map(|i| i.len()).unwrap_or(0);
            p.unseen = p.ws.unseen_activity().unwrap_or(0);
        }
        let Some(project) = self.projects.get(self.current) else {
            self.rows.clear();
            return;
        };
        let rows = match self.view.kind() {
            // The feed lists events and the graph lays nodes out; neither has rows.
            None if matches!(self.view, View::Activity | View::Graph) => Ok(Vec::new()),
            None => project.ws.inbox(),
            Some(kind) => {
                let states: Vec<State> = if self.show_all {
                    Vec::new()
                } else {
                    kind.states()
                        .iter()
                        .copied()
                        .filter(|s| !s.is_dismissed())
                        .collect()
                };
                project.ws.list(kind, &states)
            }
        };
        match rows {
            Ok(rows) => self.all = rows,
            Err(e) => self.notice = Some(e.to_string()),
        }
        self.arrange();
        if self.selected().is_none() {
            self.selected = self.rows.first().map(|r| r.id);
        }
        if let Some(id) = self.detail.as_ref().map(|d| d.node.id)
            && let Some(project) = self.projects.get(self.current)
        {
            self.detail = load_detail(&project.ws, id).ok();
        }
    }
}

/// Whether a node of `kind` in `state` with `priority` may be dropped on `target`. One rule for
/// the model and for the view's drop highlighting.
pub fn drop_allowed(kind: Kind, state: State, priority: u8, target: GroupKey) -> bool {
    match target {
        GroupKey::State(s) => state != s && kind.can_move(state, s),
        GroupKey::Priority(p) => kind == Kind::Task && priority != p,
        GroupKey::All => false,
    }
}

/// What the human can do with a node of `kind` in `state`, most important first.
pub fn choices(kind: Kind, state: State) -> Vec<Choice> {
    choices_with(kind, state, &[])
}

/// `choices`, with the gates a project's config adds counted as the human's decisions.
pub fn choices_with(kind: Kind, state: State, gates: &[Gate]) -> Vec<Choice> {
    let mut out = Vec::new();
    for &to in kind.next_states(state) {
        let decides = kind.human_only(state, to) || gates.iter().any(|g| g.holds(kind, state, to));
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
    let (learned_from, taught) = ws.related(&key, Rel::LearnedFrom)?;
    let (part_of, parts) = ws.related(&key, Rel::Parent)?;
    let (relates_to, related_by) = ws.related(&key, Rel::Relates)?;
    // Relating has no direction, and the core removes it from either end.
    let related: Vec<Brief> = relates_to.into_iter().chain(related_by).collect();
    let links = ws.links(&key)?;
    let group = |label, rel, outgoing, briefs: &Vec<Brief>| LinkGroup {
        label,
        rel,
        outgoing,
        briefs: briefs.clone(),
    };
    let link_groups: Vec<LinkGroup> = [
        group("Part of", Rel::Parent, true, &part_of),
        group("Sub-tasks", Rel::Parent, false, &parts),
        group("Waits for", Rel::DependsOn, true, &links.depends_on),
        group("Blocks", Rel::DependsOn, false, &links.blocks),
        group("Related to", Rel::Relates, true, &related),
        group("Replaces", Rel::Supersedes, true, &replaces),
        group("Replaced by", Rel::Supersedes, false, &replaced_by),
        group("Learned from", Rel::LearnedFrom, true, &learned_from),
        group("Taught", Rel::LearnedFrom, false, &taught),
    ]
    .into_iter()
    .filter(|g| !g.briefs.is_empty())
    .collect();
    Ok(Detail {
        choices: choices_with(node.kind, node.state, ws.gates()),
        links,
        history: ws.history(Some(&key))?,
        replaces,
        replaced_by,
        learned_from,
        link_groups,
        node,
    })
}
