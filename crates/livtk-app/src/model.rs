//! The app's state and every action it can take, without any UI. The views render this and
//! forward input to it, so all behaviour is tested as plain Rust.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use livtk_core::event::{Event, Op};
use livtk_core::{Actor, Brief, Command, Gate, Kind, Links, Node, Rel, State, Summary, Workspace};
use uuid::Uuid;

mod activity;
mod flow;
mod graph;
pub mod keys;
mod palette;
mod saved;
pub use activity::{FeedRow, created_as, day_label, describe, note_markdown};
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

    /// Whether the view can show dismissed work: the lists of one kind, and the graph.
    pub const fn shows_dismissed(self) -> bool {
        matches!(
            self,
            View::Tasks | View::Lessons | View::Decisions | View::Findings | View::Graph
        )
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

/// A view's display options, like Linear's Display menu. Kept per view, and between runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Display {
    /// Shown only when the key or title contains this, ignoring case.
    pub filter: String,
    /// Shown only with one of these priorities; empty shows every priority.
    pub priorities: Vec<u8>,
    /// Shown only in one of these states, in display order; empty shows every state.
    pub states: Vec<State>,
    pub grouping: Grouping,
    pub ordering: Ordering,
    /// Every group of the grouping is listed, an empty one with a count of 0, as in Linear: the
    /// shape of the work shows, what has nothing in review, nothing planned.
    pub show_empty: bool,
    /// Include what was dismissed: canceled, rejected, retired, superseded. Done work is always
    /// shown.
    pub show_dismissed: bool,
}

impl Display {
    fn for_view(view: View) -> Display {
        Display {
            filter: String::new(),
            priorities: Vec::new(),
            states: Vec::new(),
            grouping: if matches!(view, View::Inbox | View::Activity | View::Graph) {
                Grouping::None
            } else {
                Grouping::State
            },
            ordering: Ordering::Priority,
            show_empty: false,
            show_dismissed: false,
        }
    }
}

/// Where `state` comes among `kind`'s states, as groups, columns and the filter order them: the
/// kind's own order, its work flowing from first to last (for a task: backlog, planned,
/// acknowledged, doing, review, done, then parked and canceled). Without a kind (the inbox mixes
/// them), the order of all states.
pub fn state_order(kind: Option<Kind>, state: State) -> u8 {
    kind.and_then(|k| k.states().iter().position(|s| *s == state))
        .map_or(state.rank(), |i| u8::try_from(i).unwrap_or(u8::MAX))
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

/// The pause before trying again to catch up a store whose catch-up failed: the first, and the
/// longest it grows to, doubling each time it fails again.
pub const RETRY_FIRST: Duration = Duration::from_secs(1);
pub const RETRY_MAX: Duration = Duration::from_secs(30);

/// How long a notice stays before it starts to leave, unless the pointer holds it.
pub const NOTICE_FOR: Duration = Duration::from_secs(6);

/// The last thing worth telling the human: a refusal, an error. It leaves on its own
/// `NOTICE_FOR` after it was shown, fading out, but not while the pointer is over it; it can be
/// closed; a new one takes its place and its time starts again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    /// Something went wrong (a refusal, an error), or only something to know (a tool linked).
    pub error: bool,
    /// Counts the notices shown, so each is told apart from the one it replaced, even with the
    /// same text: the view plays its animations afresh.
    pub id: u64,
    /// Fading out since then, if it is leaving.
    pub leaving: Option<Instant>,
    /// When its time started: when it was shown, or when the pointer last let go of it.
    since: Instant,
    /// The pointer is over it, so it stays.
    held: bool,
}

/// Why this app posts no system notifications where it runs (macOS posts them only for its
/// bundle), for the human to be told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quiet {
    /// Not run from the bundle: told once ever.
    NoBundle,
    /// The bundle would not open: told when it happens.
    OpenFailed,
}

/// Something that came into a project's inbox from someone else (Claude, an agent): what a
/// system notification tells while the window is away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrival {
    /// The project, as an index into `Model::projects`.
    pub project: usize,
    pub id: Uuid,
    pub key: String,
    pub title: String,
}

/// One open project.
pub struct Project {
    pub name: String,
    pub dir: PathBuf,
    pub inbox: usize,
    /// What waits in its inbox, as last read.
    inbox_items: Vec<Summary>,
    /// Events by others since the human last looked at the activity feed, up to a cap.
    pub unseen: usize,
    /// Whether its store is being brought up to date with its log on another thread.
    pub catching_up: bool,
    /// Why bringing it up to date (looking for changes, catching up) failed last time, if it did.
    failure: Option<String>,
    /// After a failed catch-up, when the next may start, and the pause that led there.
    next_try: Option<Instant>,
    pause: Duration,
    ws: Workspace,
}

impl Project {
    /// Record how bringing it up to date went: `Some` reason it failed, `None` it worked. Returns
    /// what to tell the human. A failure that lasts is told once, when it begins or changes, not
    /// on every tick or catch-up: each telling would start the notice's time again, and bring it
    /// back once closed.
    fn went(&mut self, failure: Option<String>) -> Option<String> {
        let tell = failure.clone().filter(|f| self.failure.as_ref() != Some(f));
        self.failure = failure;
        tell
    }

    /// A catch-up failed at `now`: the next waits, a little longer each time, so a failure that
    /// lasts (a read-only store, a lock held elsewhere) is not retried as fast as it fails.
    fn try_later(&mut self, now: Instant) {
        self.pause = (self.pause * 2).clamp(RETRY_FIRST, RETRY_MAX);
        self.next_try = Some(now + self.pause);
    }

    /// Catching up worked, or the store is current: the next catch-up, if one is needed, starts
    /// at once.
    fn try_now(&mut self) {
        self.pause = Duration::ZERO;
        self.next_try = None;
    }
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
    notice: Option<Notice>,
    /// How many notices were shown: the next one's id.
    notices: u64,
    /// Whether the human was told that this app posts no system notifications where it runs.
    pub told_no_notifications: bool,
    /// Catching up the window must not wait for, found by `tick`, for the view to run elsewhere.
    catch_ups: Vec<CatchUp>,
    /// What came into an inbox from someone else since the view last took it.
    arrivals: Vec<Arrival>,
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
    /// The group band the selection rests on in a list, instead of a row: Enter folds it.
    band: Option<GroupKey>,
    /// Everything the view holds, before display options.
    all: Vec<Summary>,
    displays: HashMap<View, Display>,
    collapsed: HashSet<(View, GroupKey)>,
}

impl Model {
    /// Open the given project directories. One that fails to open is reported, not fatal.
    pub fn open(dirs: &[PathBuf]) -> Model {
        let mut projects = Vec::new();
        let mut failed = None;
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
                        inbox_items: Vec::new(),
                        unseen: 0,
                        catching_up: false,
                        failure: None,
                        next_try: None,
                        pause: Duration::ZERO,
                        ws,
                    });
                }
                Err(e) => failed = Some(format!("{}: {e}", dir.display())),
            }
        }
        let mut model = Model {
            projects,
            current: 0,
            view: View::Inbox,
            layout: Layout::List,
            rows: Vec::new(),
            groups: Vec::new(),
            filtered: Vec::new(),
            detail: None,
            flashed: HashMap::new(),
            leaving: Vec::new(),
            notice: None,
            notices: 0,
            told_no_notifications: false,
            catch_ups: Vec::new(),
            arrivals: Vec::new(),
            palette: None,
            activity: Vec::new(),
            activity_more: false,
            activity_index: 0,
            activity_seen_before: None,
            graph: Graph::default(),
            selected: None,
            band: None,
            all: Vec::new(),
            displays: HashMap::new(),
            collapsed: HashSet::new(),
        };
        if let Some(text) = failed {
            model.warn(text);
        }
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
            if let Some(n) = more.notice {
                self.warn(n.text);
            }
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
            self.band = None;
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
            self.band = None;
            self.detail = None;
            self.leaving.clear();
            self.enter_activity();
            self.enter_graph();
            self.reload();
        }
    }

    /// List or board, as chosen from the View options menu.
    pub fn set_layout(&mut self, layout: Layout) {
        if self.layout != layout {
            self.toggle_layout();
        }
    }

    pub fn toggle_layout(&mut self) {
        // A band is a list's; the board has none.
        self.band = None;
        self.layout = match self.layout {
            Layout::List => Layout::Board,
            Layout::Board => Layout::List,
        };
        // A board needs columns: as in Linear, it is never ungrouped.
        if self.layout == Layout::Board && self.display().grouping == Grouping::None {
            self.set_grouping(Grouping::State);
        }
    }

    // ---- selection ----------------------------------------------------------------------------

    /// The selected node: a row of the list, or on the board any card, a folded group's too.
    pub fn selected(&self) -> Option<&Summary> {
        let id = self.selected?;
        let shown = if self.on_board() {
            &self.filtered
        } else {
            &self.rows
        };
        shown.iter().find(|r| r.id == id)
    }

    pub fn selected_index(&self) -> Option<usize> {
        let id = self.selected?;
        self.rows.iter().position(|r| r.id == id)
    }

    pub fn select(&mut self, id: Uuid) {
        if self.rows.iter().any(|r| r.id == id) {
            self.band = None;
            self.selected = Some(id);
            if self.detail.is_some() {
                self.open_detail();
            }
        }
    }

    /// The group band the selection rests on, if it rests on one.
    pub fn band(&self) -> Option<GroupKey> {
        self.band
    }

    /// Whether the view is the tasks' board.
    pub fn on_board(&self) -> bool {
        self.view == View::Tasks && self.layout == Layout::Board
    }

    /// Move the selection by `delta`, stopping at the ends: in a list through its group bands
    /// and their rows; on the board down a column and on into the next (up into the previous).
    pub fn select_by(&mut self, delta: isize) {
        if self.view == View::Activity {
            self.activity_select_by(delta);
            return;
        }
        if self.view == View::Graph {
            self.graph_select_by(delta);
            return;
        }
        if self.on_board() {
            self.board_select_by(delta);
            return;
        }
        // A list's stops: each group's band (when grouped), then its rows shown.
        let mut stops: Vec<(Option<GroupKey>, Option<Uuid>)> = Vec::new();
        for g in &self.groups {
            if g.key != GroupKey::All {
                stops.push((Some(g.key), None));
            }
            for r in &self.rows[g.start..g.start + g.shown] {
                stops.push((None, Some(r.id)));
            }
        }
        if stops.is_empty() {
            return;
        }
        let at = stops.iter().position(|(band, row)| match self.band {
            Some(k) => *band == Some(k),
            None => row.is_some() && *row == self.selected,
        });
        let next = match at {
            None => 0,
            Some(i) => i.saturating_add_signed(delta).min(stops.len() - 1),
        };
        match stops[next] {
            (Some(k), _) => {
                self.band = Some(k);
                self.selected = None;
            }
            (_, Some(id)) => self.select(id),
            _ => {}
        }
    }

    /// The board's columns that hold cards, as their cards' ids, and where the selection is.
    fn board_at(&self) -> (Vec<Vec<Uuid>>, Option<(usize, usize)>) {
        let cols: Vec<Vec<Uuid>> = self
            .columns()
            .into_iter()
            .map(|(_, rows)| rows.iter().map(|r| r.id).collect::<Vec<_>>())
            .filter(|c| !c.is_empty())
            .collect();
        let at = self.selected.and_then(|id| {
            cols.iter()
                .enumerate()
                .find_map(|(c, col)| col.iter().position(|x| *x == id).map(|i| (c, i)))
        });
        (cols, at)
    }

    fn board_select_by(&mut self, delta: isize) {
        let (cols, at) = self.board_at();
        let Some((c, i)) = at else {
            if let Some(&id) = cols.first().and_then(|c| c.first()) {
                self.select_card(id);
            }
            return;
        };
        let (c, i) = if delta > 0 {
            if i + 1 < cols[c].len() {
                (c, i + 1)
            } else if c + 1 < cols.len() {
                (c + 1, 0)
            } else {
                (c, i)
            }
        } else if i > 0 {
            (c, i - 1)
        } else if c > 0 {
            (c - 1, cols[c - 1].len() - 1)
        } else {
            (c, i)
        };
        self.select_card(cols[c][i]);
    }

    /// `h` and `l` on the board: to the column `delta` along, as far down as it goes.
    pub fn select_column_by(&mut self, delta: isize) {
        if !self.on_board() {
            return;
        }
        let (cols, at) = self.board_at();
        let Some((c, i)) = at else {
            return self.board_select_by(1);
        };
        let to = c.saturating_add_signed(delta).min(cols.len() - 1);
        self.select_card(cols[to][i.min(cols[to].len() - 1)]);
    }

    /// Select a card on the board, which shows the cards of groups folded in the list too.
    fn select_card(&mut self, id: Uuid) {
        self.band = None;
        self.selected = Some(id);
        if self.detail.is_some() {
            self.open_detail();
        }
    }

    /// Enter: fold or unfold the band the selection rests on; else open the selected node.
    pub fn enter(&mut self) {
        match self.band {
            Some(k) => self.toggle_collapsed(k),
            None => self.open_detail(),
        }
    }

    /// The node actions act on: the one open in the panel, which is what the human is looking
    /// at (a change from elsewhere can move the selection while the panel stays); else the
    /// selected row.
    pub fn acted_on(&self) -> Option<Summary> {
        let Some(n) = self.detail.as_ref().map(|d| &d.node) else {
            return self.selected().cloned();
        };
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
            Err(e) => self.warn(e.to_string()),
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

    /// Add a note to the node acted on: the one open in the panel, whose field it is written in.
    pub fn add_note(&mut self, text: &str) {
        let Some(node) = self.acted_on().map(|r| r.id) else {
            return;
        };
        if !text.trim().is_empty() {
            self.run(
                Command::Note {
                    node: node.to_string(),
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
                self.warn(e.to_string());
                false
            }
        }
    }

    /// Run `cmd` as the human; whether it was made.
    fn run(&mut self, cmd: Command, row: Option<Summary>) -> bool {
        // Only a row that was on screen can fade out of it, from where it was.
        let at = row
            .as_ref()
            .and_then(|r| self.rows.iter().position(|x| x.id == r.id));
        let row = row.filter(|_| at.is_some());
        let was_selected = row.as_ref().is_some_and(|r| self.selected == Some(r.id));
        let Some(project) = self.projects.get_mut(self.current) else {
            return false;
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
            Err(e) => {
                self.warn(e.to_string());
                return false;
            }
        }
        true
    }

    // ---- staying current ----------------------------------------------------------------------

    /// Pick up changes made elsewhere. Cheap when nothing changed. Returns whether anything did.
    /// It never catches a store up itself: a project whose log is ahead of its store (git
    /// rewrote it) is left as it was shown, and a `CatchUp` for it waits in `take_catch_ups`.
    pub fn tick(&mut self) -> bool {
        self.tick_at(Instant::now())
    }

    /// `tick`, at `now`: a store whose last catch-up failed is tried again only once its pause
    /// is over.
    pub fn tick_at(&mut self, now: Instant) -> bool {
        let mut changed = false;
        // A catch-up just begun changes only what the sidebar says: a repaint, no reload.
        let mut began = false;
        let mut failed = None;
        for p in &mut self.projects {
            if p.catching_up {
                continue;
            }
            // Any project counts: the sidebar shows every project's inbox. `None`: it is stale.
            let looked = p.ws.stale().and_then(|stale| {
                if stale {
                    Ok(None)
                } else {
                    p.ws.refresh().map(Some)
                }
            });
            let failure = match looked {
                Ok(None) if p.next_try.is_some_and(|t| now < t) => continue,
                Ok(None) => {
                    p.catching_up = true;
                    self.catch_ups.push(CatchUp {
                        project: p.dir.clone(),
                        livtk: p.ws.dir().to_path_buf(),
                    });
                    began = true;
                    // Whether catching up works is for the catch-up to tell: a failure of the
                    // last one stands until one succeeds.
                    continue;
                }
                Ok(Some(changed_here)) => {
                    // Current again, whoever caught it up.
                    p.try_now();
                    changed |= changed_here;
                    None
                }
                Err(e) => Some(format!("{}: {e}", p.name)),
            };
            failed = p.went(failure).or(failed);
        }
        if let Some(text) = failed {
            self.warn(text);
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
        let waited: Vec<HashSet<Uuid>> = self
            .projects
            .iter()
            .map(|p| p.inbox_items.iter().map(|r| r.id).collect())
            .collect();
        self.reload();
        self.note_arrivals(&waited);
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

    /// What entered each project's inbox since `waited`, put there by anyone but the human: a
    /// change made elsewhere, as `tick` finds them. Who put it there is who last set its state
    /// (or created it), whatever came after, a note or a link: the human's own move, from the
    /// app or a terminal, is not news to them, and Claude's is, even once the human noted it.
    fn note_arrivals(&mut self, waited: &[HashSet<Uuid>]) {
        for (i, p) in self.projects.iter().enumerate() {
            let before = waited.get(i);
            for r in &p.inbox_items {
                if before.is_some_and(|b| b.contains(&r.id)) {
                    continue;
                }
                let by_human =
                    p.ws.history(Some(&r.id.to_string()))
                        .ok()
                        .and_then(|h| {
                            h.iter()
                                .rev()
                                .find(|e| match &e.op {
                                    Op::NodeCreate { .. } => true,
                                    Op::NodeSet { data, .. } => data.state.is_some(),
                                    _ => false,
                                })
                                .map(|e| e.by == "human")
                        })
                        .unwrap_or(true);
                if !by_human {
                    self.arrivals.push(Arrival {
                        project: i,
                        id: r.id,
                        key: r.key.clone(),
                        title: r.title.clone(),
                    });
                }
            }
        }
    }

    /// What came into an inbox from someone else since this was last called, each once.
    pub fn take_arrivals(&mut self) -> Vec<Arrival> {
        std::mem::take(&mut self.arrivals)
    }

    /// Open the inbox where `arrival` waits, on it, with its detail.
    pub fn open_arrival(&mut self, arrival: &Arrival) {
        self.select_project(arrival.project);
        self.set_view(View::Inbox);
        self.select(arrival.id);
        if self.selected().is_some_and(|r| r.id == arrival.id) {
            self.open_detail();
        }
    }

    /// The catching up `tick` found and left to be run elsewhere, each once.
    pub fn take_catch_ups(&mut self) -> Vec<CatchUp> {
        std::mem::take(&mut self.catch_ups)
    }

    /// A `CatchUp` finished with `result`: the project is shown from its store again, which the
    /// next `tick` finds current and changed. One that failed is tried again after a pause.
    pub fn caught_up(&mut self, job: &CatchUp, result: Result<(), String>) {
        self.caught_up_at(job, result, Instant::now());
    }

    /// `caught_up`, at `now`: the pause after a failure counts from it.
    pub fn caught_up_at(&mut self, job: &CatchUp, result: Result<(), String>, now: Instant) {
        let Some(p) = self.projects.iter_mut().find(|p| p.dir == job.project) else {
            return;
        };
        p.catching_up = false;
        match result {
            Ok(()) => p.try_now(),
            Err(_) => p.try_later(now),
        }
        let failure = result.err().map(|e| format!("{}: {e}", p.name));
        if let Some(text) = p.went(failure) {
            self.warn(text);
        }
    }

    /// Forget visual state older than `age`: finished flashes and fades.
    pub fn settle(&mut self, now: Instant, age: std::time::Duration) {
        self.flashed.retain(|_, at| now.duration_since(*at) < age);
        self.leaving
            .retain(|(_, _, at)| now.duration_since(*at) < age);
    }

    // ---- the notice ---------------------------------------------------------------------------

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    /// Tell the human something went wrong. It replaces any notice shown, and its time starts now.
    /// It shows where the one it replaces did, so a pointer over that one is over it, and holds
    /// it.
    pub fn warn(&mut self, text: impl Into<String>) {
        self.notice_of(text.into(), true);
    }

    /// Tell the human something worth knowing that is no error, as a notice does.
    pub fn inform(&mut self, text: impl Into<String>) {
        self.notice_of(text.into(), false);
    }

    fn notice_of(&mut self, text: String, error: bool) {
        self.notices += 1;
        self.notice = Some(Notice {
            text,
            error,
            id: self.notices,
            leaving: None,
            since: Instant::now(),
            held: self.notice.as_ref().is_some_and(|n| n.held),
        });
    }

    /// Tell the human why this app posts no system notifications where it runs, and where it
    /// would. Running outside the bundle is told once ever; a bundle that would not open, each
    /// time. A notice already showing (a project that would not open, say) is left in place, and
    /// what was not told then is told another time.
    pub fn tell_quiet(&mut self, why: Quiet) {
        if self.notice.is_some() || (why == Quiet::NoBundle && self.told_no_notifications) {
            return;
        }
        let text = match why {
            Quiet::NoBundle => {
                self.told_no_notifications = true;
                "No notifications from here: on macOS they come from livtk.app only. \
                 scripts/check --install puts it in Applications and livtk-app on PATH, which \
                 then opens it for you."
            }
            Quiet::OpenFailed => {
                "livtk.app would not open, so this runs here, without notifications."
            }
        };
        self.warn(text);
    }

    /// Close the notice: the human read it, or what it told of no longer holds.
    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    /// The pointer came over the notice (`held`), which keeps it, a leaving one included, or
    /// went away at `now`, which gives it its whole time again.
    pub fn hold_notice(&mut self, held: bool, now: Instant) {
        if let Some(n) = &mut self.notice {
            n.held = held;
            if held {
                n.leaving = None;
            } else {
                n.since = now;
            }
        }
    }

    /// Let the notice's time pass to `now`: it starts to leave once its time is up, unless held,
    /// and is gone `fade` after that. Returns whether it changed, so the view repaints only then.
    pub fn age_notice(&mut self, now: Instant, fade: Duration) -> bool {
        let Some(n) = &mut self.notice else {
            return false;
        };
        if n.held {
            return false;
        }
        match n.leaving {
            None if now.duration_since(n.since) >= NOTICE_FOR => {
                n.leaving = Some(now);
                true
            }
            Some(at) if now.duration_since(at) >= fade => {
                self.notice = None;
                true
            }
            _ => false,
        }
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
        self.reselect();
    }

    /// When what is shown changed under the human's hand (a filter, a group folded) and the
    /// selection went with it: the first row, the panel following it, so every action still
    /// acts on the node in view.
    /// The node open in the panel comes first, when it is shown again (a filter cleared).
    fn reselect(&mut self) {
        // Resting on a band is a selection too: folding it keeps it there. A band gone from
        // view (filtered out) is let go, and the selection found again as below.
        if let Some(k) = self.band {
            if self.groups.iter().any(|g| g.key == k) {
                return;
            }
            self.band = None;
        }
        if self.selected().is_none() {
            let panel = self
                .detail
                .as_ref()
                .map(|d| d.node.id)
                .filter(|id| self.rows.iter().any(|r| r.id == *id));
            match panel.or_else(|| self.rows.first().map(|r| r.id)) {
                Some(id) => self.select(id),
                None => self.selected = None,
            }
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

    /// Show only rows in `state`, besides those in the states already chosen, or no longer.
    pub fn toggle_state(&mut self, state: State) {
        let kind = self.view.kind();
        self.edit_display(|d| {
            if let Some(i) = d.states.iter().position(|s| *s == state) {
                d.states.remove(i);
            } else {
                d.states.push(state);
                d.states.sort_by_key(|s| state_order(kind, *s));
            }
        });
    }

    /// Clear the view's filter: its text, priorities and states.
    pub fn clear_filters(&mut self) {
        self.edit_display(|d| {
            d.filter.clear();
            d.priorities.clear();
            d.states.clear();
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

    /// Show or hide the current view's empty groups, where it lists groups of one kind.
    pub fn toggle_empty(&mut self) {
        if self.view.kind().is_some() {
            self.edit_display(|d| d.show_empty = !d.show_empty);
        }
    }

    /// Show or hide the current view's dismissed work, where it has any. What a view holds is
    /// read without it, so it is read again.
    pub fn toggle_dismissed(&mut self) {
        if self.view.shows_dismissed() {
            let mut d = self.display();
            d.show_dismissed = !d.show_dismissed;
            // A dismissed state chosen in the filter would show nothing while hidden.
            if !d.show_dismissed {
                d.states.retain(|s| !s.is_dismissed());
            }
            self.displays.insert(self.view, d);
            self.reload();
        }
    }

    pub fn toggle_collapsed(&mut self, key: GroupKey) {
        if !self.collapsed.remove(&(self.view, key)) {
            self.collapsed.insert((self.view, key));
        }
        self.arrange();
        self.reselect();
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
            .filter(|r| d.states.is_empty() || d.states.contains(&r.state))
            .cloned()
            .collect();
        let group_of = |r: &Summary| match d.grouping {
            Grouping::State => GroupKey::State(r.state),
            Grouping::Priority => GroupKey::Priority(r.priority),
            Grouping::None => GroupKey::All,
        };
        let kind = self.view.kind();
        let group_rank = |k: GroupKey| match k {
            GroupKey::State(s) => state_order(kind, s),
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
        // The groups the rows fall in, in order; with `show_empty`, every group there can be.
        let mut keys: Vec<GroupKey> = Vec::new();
        for r in &rows {
            let key = group_of(r);
            if keys.last() != Some(&key) {
                keys.push(key);
            }
        }
        if d.show_empty {
            keys.extend(self.every_group(&d));
            keys.sort_by_key(|k| group_rank(*k));
            keys.dedup();
        }
        let mut groups: Vec<Group> = Vec::with_capacity(keys.len());
        let mut shown: Vec<Summary> = Vec::with_capacity(rows.len());
        let mut next = rows.iter().peekable();
        for key in keys {
            let collapsed = self.collapsed.contains(&(self.view, key));
            let start = shown.len();
            let mut count = 0;
            while let Some(r) = next.next_if(|r| group_of(r) == key) {
                count += 1;
                if !collapsed {
                    shown.push(r.clone());
                }
            }
            groups.push(Group {
                key,
                kind: self.view.kind(),
                count,
                collapsed,
                start,
                shown: shown.len() - start,
            });
        }
        self.filtered = rows;
        self.rows = shown;
        self.groups = groups;
    }

    /// Every group the view's grouping has, in no order: each state of the view's kind, the
    /// dismissed ones only while shown, or each priority. None where the view is ungrouped or
    /// mixes kinds, as the inbox does.
    fn every_group(&self, d: &Display) -> Vec<GroupKey> {
        match (d.grouping, self.view.kind()) {
            (Grouping::State, Some(kind)) => kind
                .states()
                .iter()
                .filter(|s| d.show_dismissed || !s.is_dismissed())
                .filter(|s| d.states.is_empty() || d.states.contains(s))
                .map(|&s| GroupKey::State(s))
                .collect(),
            (Grouping::Priority, Some(_)) => (1..=4).map(GroupKey::Priority).collect(),
            _ => Vec::new(),
        }
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
        // The card dragged is what the human acts on next: the panel follows it too, even
        // when its group is folded in the list (the board shows it all the same).
        self.selected = Some(id);
        if self.detail.is_some() {
            self.open_detail();
        }
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
    /// or priorities from urgent to low), each in the chosen order. As the list's groups, an
    /// empty column shows only with `show_empty`; a card goes to a state with no column from its
    /// state's menu.
    pub fn columns(&self) -> Vec<(GroupKey, Vec<&Summary>)> {
        let d = self.display();
        let keys: Vec<GroupKey> = if d.grouping == Grouping::Priority {
            (1..=4u8).map(GroupKey::Priority).collect()
        } else {
            // The task's states in their own order, as the list's groups; dismissed ones only
            // while shown. The filter says what the board is about: no column for a state it
            // leaves out.
            Kind::Task
                .states()
                .iter()
                .copied()
                .filter(|s| d.show_dismissed || !s.is_dismissed())
                .filter(|s| d.states.is_empty() || d.states.contains(s))
                .map(GroupKey::State)
                .collect()
        };
        keys.into_iter()
            .map(|key| {
                let rows = self
                    .filtered
                    .iter()
                    .filter(|r| match key {
                        GroupKey::State(s) => r.state == s,
                        GroupKey::Priority(p) => r.priority == p,
                        GroupKey::All => true,
                    })
                    .collect::<Vec<_>>();
                (key, rows)
            })
            .filter(|(_, rows)| d.show_empty || !rows.is_empty())
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
            p.inbox_items = p.ws.inbox().unwrap_or_default();
            p.inbox = p.inbox_items.len();
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
                let states: Vec<State> = if self.display().show_dismissed {
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
            Err(e) => self.warn(e.to_string()),
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
