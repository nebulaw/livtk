//! The app's state and every action it can take, without any UI. The views render this and
//! forward input to it, so all behaviour is tested as plain Rust.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use livtk_core::event::{Event, Op};
use livtk_core::{Actor, Brief, Command, Gate, Kind, Links, Node, Rel, State, Summary, Workspace};
use uuid::Uuid;

mod activity;
pub mod agents;
pub mod contour;
pub mod docs;
mod flow;
mod graph;
pub mod grid;
pub mod keys;
pub mod listing;
pub mod making;
mod palette;
pub mod prefs;
pub mod runs;
mod saved;
pub mod timeline;
pub mod toasts;
pub use activity::{FeedRow, INDENT, created_as, day_label, describe, note_markdown, reindent};
pub use flow::{
    Arrow, Look, Step, arrows, arrows_with, flow_of, has_flow, state_name, state_short, steps,
    steps_with,
};
pub use graph::{
    CARD_H, CARD_W, Camera, DOTS_BELOW, FIT_BELOW, Graph, GraphEdge, GraphNode, MAX_ZOOM, MIN_ZOOM,
    link_curve, visible_runs,
};
pub use listing::ListSpec;
pub use palette::{Action, Ask, DocItem, DocPick, Found, Item, LinkPick, Palette, Run, fuzzy};
pub use saved::{Saved, SavedView, SavedWindow};
pub use toasts::{Act, Button, Level, Mark, Toast, Toasts};

/// The views in the sidebar, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Deserialize)]
pub enum View {
    Inbox,
    /// Every change in the project, newest first.
    Activity,
    Tasks,
    /// Outcomes above tasks, with their progress.
    Projects,
    Lessons,
    Decisions,
    Findings,
    /// Every node and link, laid out.
    Graph,
    /// A shell in the project's folder.
    Terminal,
    /// The project's docs: its markdown files.
    Docs,
    /// The agents Claude sessions can run as: livtk's, the project's, yours.
    Agents,
    /// The project's Claude sessions, past and running, to read.
    Sessions,
    /// A kind the project declares (`[[kinds]]` in its config), listed as the built-in kinds are.
    Of(Kind),
}

impl View {
    pub const ALL: [View; 12] = [
        View::Inbox,
        View::Activity,
        View::Tasks,
        View::Projects,
        View::Lessons,
        View::Decisions,
        View::Findings,
        View::Docs,
        View::Agents,
        View::Sessions,
        View::Graph,
        View::Terminal,
    ];

    pub fn title(self) -> &'static str {
        match self {
            View::Of(kind) => kind_title(kind),
            View::Inbox => "Inbox",
            View::Activity => "Activity",
            View::Tasks => "Tasks",
            View::Projects => "Projects",
            View::Lessons => "Lessons",
            View::Decisions => "Decisions",
            View::Findings => "Findings",
            View::Graph => "Graph",
            View::Terminal => "Terminal",
            View::Docs => "Docs",
            View::Agents => "Agents",
            View::Sessions => "Sessions",
        }
    }

    /// The keys that go to the view, as the palette shows them (`G T` for the tasks; bound as
    /// `g t`); a declared kind's view has none. The key bindings, the View menu and the palette
    /// all read them from here.
    pub const fn go_keys(self) -> Option<&'static str> {
        Some(match self {
            View::Inbox => "G I",
            View::Activity => "G A",
            View::Tasks => "G T",
            View::Projects => "G P",
            View::Lessons => "G L",
            View::Decisions => "G D",
            View::Findings => "G F",
            View::Docs => "G O",
            View::Agents => "G E",
            View::Sessions => "G S",
            View::Graph => "G R",
            View::Terminal => "G X",
            View::Of(_) => return None,
        })
    }

    /// Whether the view lays its nodes out as a board too, as its kind's list says.
    pub fn has_board(self) -> bool {
        self.listing_spec().is_some_and(|s| s.board)
    }

    /// Whether the view can show dismissed work: the lists of one kind, and the graph.
    pub const fn shows_dismissed(self) -> bool {
        matches!(
            self,
            View::Tasks
                | View::Projects
                | View::Lessons
                | View::Decisions
                | View::Findings
                | View::Graph
        )
    }

    /// The view that lists `kind`: its own, or a declared kind's.
    pub fn listing(kind: Kind) -> View {
        View::ALL
            .into_iter()
            .find(|v| v.kind() == Some(kind))
            .unwrap_or(View::Of(kind))
    }

    /// The kind a view lists; the inbox mixes kinds, and the activity feed lists events.
    pub const fn kind(self) -> Option<Kind> {
        match self {
            View::Of(kind) => Some(kind),
            View::Inbox
            | View::Activity
            | View::Graph
            | View::Terminal
            | View::Docs
            | View::Agents
            | View::Sessions => None,
            View::Tasks => Some(Kind::Task),
            View::Projects => Some(Kind::Project),
            View::Lessons => Some(Kind::Lesson),
            View::Decisions => Some(Kind::Decision),
            View::Findings => Some(Kind::Finding),
        }
    }
}

/// A declared kind's view's name: its name, plural, as a heading (`incident` → `Incidents`).
pub fn kind_title(kind: Kind) -> &'static str {
    static TITLES: std::sync::LazyLock<std::sync::Mutex<HashMap<Kind, &'static str>>> =
        std::sync::LazyLock::new(Default::default);
    let mut titles = TITLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    titles.entry(kind).or_insert_with(|| {
        let name = kind.as_str().replace('_', " ");
        let mut c = name.chars();
        let title: String = c
            .next()
            .map(|f| f.to_uppercase().chain(c).collect())
            .unwrap_or_default();
        let plural = if title.ends_with('s') {
            format!("{title}es")
        } else if let Some(stem) = title.strip_suffix('y') {
            format!("{stem}ies")
        } else {
            format!("{title}s")
        };
        Box::leak(plural.into_boxed_str())
    })
}

/// Where looking for a newer build stands: what the sidebar's updates row, its rail button and
/// the palette all read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpdateState {
    /// Nothing staged, nothing being looked for: a check can be asked.
    #[default]
    Idle,
    /// A look for updates is under way; another is not started.
    Checking,
    /// Build N is staged and waits for a restart.
    Staged(u64),
}

impl UpdateState {
    /// The staged build's number, once there is one.
    pub fn staged(self) -> Option<u64> {
        match self {
            UpdateState::Staged(build) => Some(build),
            _ => None,
        }
    }
}

/// How a node's detail opens, as Notion opens a page from a database: in a panel beside the
/// list (side peek), in a large card over the window (center peek), or in the list's place
/// (full page).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailMode {
    #[default]
    Side,
    Center,
    Full,
}

impl DetailMode {
    pub const ALL: [DetailMode; 3] = [DetailMode::Side, DetailMode::Center, DetailMode::Full];

    pub const fn name(self) -> &'static str {
        match self {
            DetailMode::Side => "Side peek",
            DetailMode::Center => "Center peek",
            DetailMode::Full => "Full page",
        }
    }

    const fn key(self) -> &'static str {
        match self {
            DetailMode::Side => "side",
            DetailMode::Center => "center",
            DetailMode::Full => "full",
        }
    }
}

/// How docs read, as Settings sets it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub size: ReadingSize,
    pub compact: bool,
}

/// The docs' text size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadingSize {
    Small,
    #[default]
    Regular,
    Large,
}

impl ReadingSize {
    pub const ALL: [ReadingSize; 3] =
        [ReadingSize::Small, ReadingSize::Regular, ReadingSize::Large];

    /// The text's size in points.
    pub const fn points(self) -> f32 {
        match self {
            ReadingSize::Small => 15.,
            ReadingSize::Regular => 16.,
            ReadingSize::Large => 18.,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            ReadingSize::Small => "Small",
            ReadingSize::Regular => "Regular",
            ReadingSize::Large => "Large",
        }
    }
}

impl Reading {
    /// The line's height as a share of the text's size, and the gap between paragraphs.
    pub const fn line(self) -> f32 {
        if self.compact { 1.45 } else { 1.65 }
    }

    pub const fn gap(self) -> f32 {
        if self.compact { 0.7 } else { 1. }
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
    /// By the project a task is part of (tasks only).
    Project,
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
    /// Sub-tasks shown under their parent, indented, where both fall in the same group.
    pub nest: bool,
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
            // By priority where the nodes have one; else the latest first.
            ordering: if view.offers_ordering(Ordering::Priority) {
                Ordering::Priority
            } else {
                Ordering::Updated
            },
            show_empty: false,
            show_dismissed: false,
            nest: true,
        }
    }
}

/// Where `state` comes among `kind`'s states, as groups, columns and the filter order them: the
/// kind's own order, its work flowing from first to last (for a task: backlog, planned,
/// acknowledged, doing, review, done, then parked and canceled). Without a kind (the inbox mixes
/// them), the order of all states.
pub fn state_order(kind: Option<Kind>, state: State) -> u8 {
    // Projects: the active ones on top, those waiting to be committed to next.
    if kind == Some(Kind::Project) {
        const ORDER: [State; 5] = [
            State::Active,
            State::Proposed,
            State::Paused,
            State::Done,
            State::Dropped,
        ];
        if let Some(i) = ORDER.iter().position(|s| *s == state) {
            return i as u8;
        }
    }
    kind.and_then(|k| k.states().iter().position(|s| *s == state))
        .map_or(state.rank(), |i| u8::try_from(i).unwrap_or(u8::MAX))
}

/// What a group gathers: a state, a priority, or everything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupKey {
    State(State),
    Priority(u8),
    /// The project, by its id; `None` gathers the tasks part of none.
    Project(Option<Uuid>),
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
    /// The docs it is linked to, by path, each with its title (`None` once it is gone).
    pub docs: Vec<(String, Option<String>)>,
    /// The Claude Code sessions that worked on it, each by actor.
    pub sessions: Vec<livtk_core::SessionUse>,
    /// Its latest run of Claude, if it has had one.
    pub run: Option<livtk_core::run::Run>,
    /// The project it is in, directly or through its parent task.
    pub in_project: Option<Brief>,
    /// The task it is part of, if any: its project is that task's, changed there.
    pub parent_task: Option<Brief>,
    /// When it came into its state, as the log writes times.
    pub entered: Option<String>,
    /// Its sub-tasks: how many are done, of how many; none without any.
    pub sub_tasks: Option<(usize, usize)>,
    /// A project's progress, its updates newest first, and its flow.
    pub project: Option<(
        livtk_core::Progress,
        Vec<livtk_core::Update>,
        livtk_core::Flow,
    )>,
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
/// The relations a project declares (`[[relations]]`), in its order.
pub fn declared_rels(ws: &Workspace) -> Vec<Rel> {
    ws.config()
        .relations
        .iter()
        .filter_map(|r| Rel::named(&r.name))
        .collect()
}

/// A declared relation's name as a heading: `implemented_by` reads `Implemented by`; with
/// `…` after it, as a menu's choice.
pub fn heading(name: &'static str) -> &'static str {
    titled_as(name, false)
}

/// `heading`, once per name and form: the text lives for the process, made once.
fn titled_as(name: &'static str, choice: bool) -> &'static str {
    type Cache = HashMap<(&'static str, bool), &'static str>;
    static HEADINGS: std::sync::LazyLock<std::sync::Mutex<Cache>> =
        std::sync::LazyLock::new(Default::default);
    let mut cache = HEADINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.entry((name, choice)).or_insert_with(|| {
        let mut text = name.replace('_', " ");
        if choice {
            text.push('…');
        }
        let mut c = text.chars();
        let t: String = c
            .next()
            .map(|f| f.to_uppercase().chain(c).collect())
            .unwrap_or_default();
        Box::leak(t.into_boxed_str())
    })
}

pub fn link_choices(kind: Kind, declared: &[Rel]) -> Vec<LinkChoice> {
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
        _ => vec![c("Learned from…", Rel::LearnedFrom, true)],
    };
    out.push(c("Related to…", Rel::Relates, true));
    // The project's own relations, from this end and, by its inverse, from the other.
    for &rel in declared {
        let choice = |name: &'static str, outgoing| LinkChoice {
            label: titled_as(name, true),
            rel,
            outgoing,
        };
        out.push(choice(rel.as_str(), true));
        if let Some(inverse) = rel.inverse() {
            out.push(choice(inverse, false));
        }
    }
    out
}

/// The recent projects the top bar's menu shows, and how many are remembered.
pub const RECENT_SHOWN: usize = 8;
const RECENT_KEPT: usize = 32;

pub struct Model {
    pub projects: Vec<Project>,
    pub current: usize,
    /// The projects' directories in the order they were last opened, the latest first.
    pub recent: Vec<PathBuf>,
    /// The panes, each a view with its own state, and the one acted on: the focused one, but
    /// for a moment another (`with_pane`) while it is brought up to date or drawn.
    panes: Vec<Pane>,
    at: usize,
    /// How the panes are laid out, by their ids; and the id the next pane takes.
    pub split: Split,
    next_pane: u64,
    /// A view shown alone over the split, as it is in none of its panes: its pane, out of the
    /// split; and the split's pane to come back to.
    solo: Option<u64>,
    split_focus: u64,
    pub detail: Option<Detail>,
    /// Rows that changed under the app's feet (Claude, the CLI, a git pull), and when it noticed.
    pub flashed: HashMap<Uuid, Instant>,
    /// What the app tells the human in the window.
    toasts: Toasts,
    /// The activity grid above the feed.
    pub grid: grid::Grid,
    /// The Docs view, and the doc last open in each project.
    pub docs: docs::Docs,
    pub docs_open: HashMap<PathBuf, String>,
    /// The docs' folders folded in each project, by the project's folder.
    pub docs_shut: HashMap<PathBuf, HashSet<String>>,
    /// Projects with `[[on]]` hooks the human wrote to since the view last took them: their
    /// hooks are to be run, apart from the window.
    wrote: Vec<(PathBuf, Option<Uuid>)>,
    /// Whether the human was told that this app posts no system notifications where it runs.
    pub told_no_notifications: bool,
    /// How docs read: their text's size, and how close their lines sit.
    pub reading: Reading,
    /// How a node's detail opens: beside, centred over the window, or in its pane's place; and
    /// the pane it was opened from (where a full page shows it).
    pub detail_mode: DetailMode,
    pub detail_pane: Option<u64>,
    /// Your own on/off choices about what the window shows.
    pub prefs: prefs::Prefs,
    /// Where looking for a newer build stands: what offers a check or the restart (the
    /// sidebar, its rail, the palette) reads it here.
    pub updates: UpdateState,
    /// Catching up the window must not wait for, found by `tick`, for the view to run elsewhere.
    catch_ups: Vec<CatchUp>,
    /// What came into an inbox from someone else since the view last took it.
    arrivals: Vec<Arrival>,
    /// The command palette, while it is open.
    pub palette: Option<Palette>,
    /// The activity feed's events, newest first, as far as its pages are loaded.
    pub activity: Vec<livtk_core::Activity>,
    /// Agents and Sessions: the agent files, and the sessions.
    pub agents: agents::AgentsPlace,
    /// The project's flow, as the activity view shows it: what waits on the human and for how
    /// long, and the time tasks spend in each state.
    pub flow: Option<livtk_core::Flow>,
    /// Whether older events wait for "Show older".
    pub activity_more: bool,
    /// The highlighted event of the feed.
    pub activity_index: usize,
    /// The mark as it was when the feed was opened: what came after it is new on this visit.
    activity_seen_before: Option<Uuid>,
    /// The graph view's nodes, links, camera and selection.
    pub graph: Graph,
    /// Runs of Claude in the open projects: writing, and stopped by the last quit.
    pub runs: runs::Runs,
    /// Each project's graph camera, by its folder, as last left: taken back on the next visit.
    graph_cameras: HashMap<PathBuf, Camera>,
    displays: HashMap<View, Display>,
    collapsed: HashSet<(View, GroupKey)>,
}

/// Which way a split lays its panes: side by side, or one over another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    Row,
    Column,
}

/// How the panes are laid out: a pane, or panes split along an axis, each with its share of
/// the room (the shares add up to 1).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Split {
    Pane(u64),
    Split {
        axis: Axis,
        parts: Vec<(Split, f32)>,
    },
}

impl Split {
    /// The panes' ids, in reading order.
    pub fn ids(&self) -> Vec<u64> {
        match self {
            Split::Pane(id) => vec![*id],
            Split::Split { parts, .. } => parts.iter().flat_map(|(p, _)| p.ids()).collect(),
        }
    }

    /// `id`'s pane split along `axis`, `new` after it, the two sharing its room. In a split
    /// along the same axis, `new` joins it rather than nesting.
    fn split(&mut self, id: u64, axis: Axis, new: u64) -> bool {
        match self {
            Split::Pane(p) if *p == id => {
                *self = Split::Split {
                    axis,
                    parts: vec![(Split::Pane(id), 0.5), (Split::Pane(new), 0.5)],
                };
                true
            }
            Split::Pane(_) => false,
            Split::Split { axis: a, parts } => {
                if *a == axis
                    && let Some(i) = parts.iter().position(|(p, _)| *p == Split::Pane(id))
                {
                    let half = parts[i].1 / 2.;
                    parts[i].1 = half;
                    parts.insert(i + 1, (Split::Pane(new), half));
                    return true;
                }
                parts.iter_mut().any(|(p, _)| p.split(id, axis, new))
            }
        }
    }

    /// `id`'s pane taken out, its room given to the rest of its split; a split left with one
    /// part becomes that part.
    fn remove(&mut self, id: u64) -> bool {
        let Split::Split { parts, .. } = self else {
            return false;
        };
        if let Some(i) = parts.iter().position(|(p, _)| *p == Split::Pane(id)) {
            let (_, share) = parts.remove(i);
            let rest: f32 = parts.iter().map(|(_, f)| f).sum();
            for (_, f) in parts.iter_mut() {
                *f += share * *f / rest.max(f32::EPSILON);
            }
        } else if !parts.iter_mut().any(|(p, _)| p.remove(id)) {
            return false;
        }
        if parts.len() == 1 {
            *self = parts.remove(0).0;
        }
        true
    }

    /// The split at `path` (each step a part's index), if there is one.
    pub fn at_mut(&mut self, path: &[usize]) -> Option<&mut Split> {
        match path.split_first() {
            None => Some(self),
            Some((&i, rest)) => match self {
                Split::Split { parts, .. } => parts.get_mut(i)?.0.at_mut(rest),
                Split::Pane(_) => None,
            },
        }
    }
}

/// A pane of the window: the view it shows, and that view's own state: what it lists and how,
/// and where its selection is. Each view shows in one pane at most, so what a view keeps by
/// itself (the docs, the graph, the feed, its display) stays the model's.
#[derive(Clone, Debug)]
pub struct Pane {
    pub id: u64,
    /// Whether it waits for a view to be chosen: a split's new pane, until one is.
    pub placeholder: bool,
    pub view: View,
    pub layout: Layout,
    /// The rows shown, in display order: filtered, grouped, ordered, collapsed groups left out.
    pub rows: Vec<Summary>,
    /// The groups of `rows`, in order.
    pub groups: Vec<Group>,
    /// Every row after filtering, collapsed or not: what the board shows.
    pub filtered: Vec<Summary>,
    /// Rows the human just moved out of the view, where they were, and when: the view fades
    /// them out in place.
    pub leaving: Vec<(Summary, usize, Instant)>,
    selected: Option<Uuid>,
    /// The group band the selection rests on in a list, instead of a row: Enter folds it.
    band: Option<GroupKey>,
    /// Everything the view holds, before display options.
    all: Vec<Summary>,
    /// How deep each nested sub-task sits under its parent (absent: not nested).
    pub depth: HashMap<Uuid, usize>,
    /// Each parent's sub-tasks: how many are done, of how many.
    pub progress: HashMap<Uuid, (usize, usize)>,
    /// In the projects view, each project's progress.
    pub projects: HashMap<Uuid, livtk_core::Progress>,
    /// Grouped by project, the project each row is part of, by its id, and the projects in
    /// key order with their titles.
    pub project_of: HashMap<Uuid, Uuid>,
    pub project_names: Vec<(Uuid, Brief)>,
}

impl Pane {
    pub fn new(id: u64, view: View) -> Pane {
        Pane {
            id,
            placeholder: false,
            view,
            layout: Layout::List,
            rows: Vec::new(),
            groups: Vec::new(),
            filtered: Vec::new(),
            leaving: Vec::new(),
            selected: None,
            band: None,
            all: Vec::new(),
            depth: HashMap::new(),
            progress: HashMap::new(),
            projects: HashMap::new(),
            project_of: HashMap::new(),
            project_names: Vec::new(),
        }
    }
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
            recent: Vec::new(),
            panes: vec![Pane::new(0, View::Inbox)],
            at: 0,
            split: Split::Pane(0),
            next_pane: 1,
            solo: None,
            split_focus: 0,
            detail: None,
            flashed: HashMap::new(),
            toasts: Toasts::default(),
            grid: grid::Grid::default(),
            docs: docs::Docs::default(),
            docs_open: HashMap::new(),
            docs_shut: HashMap::new(),
            wrote: Vec::new(),
            told_no_notifications: false,
            reading: Reading::default(),
            detail_mode: DetailMode::default(),
            detail_pane: None,
            prefs: prefs::Prefs::default(),
            updates: UpdateState::Idle,
            catch_ups: Vec::new(),
            arrivals: Vec::new(),
            palette: None,
            activity: Vec::new(),
            agents: agents::AgentsPlace::default(),
            flow: None,
            activity_more: false,
            activity_index: 0,
            activity_seen_before: None,
            graph: Graph::default(),
            runs: runs::Runs::default(),
            graph_cameras: HashMap::new(),
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
            if let Some(t) = more.toasts.latest() {
                self.warn(t.text.clone());
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

    // ---- panes --------------------------------------------------------------------------------

    /// The pane acted on: the focused one, or the one `with_pane` is on.
    fn pane(&self) -> &Pane {
        &self.panes[self.at]
    }

    fn pane_mut(&mut self) -> &mut Pane {
        &mut self.panes[self.at]
    }

    /// Act on pane `i` for a while (bring it up to date, draw it), then on the one before.
    pub fn with_pane<R>(&mut self, i: usize, f: impl FnOnce(&mut Model) -> R) -> R {
        let was = self.at;
        self.at = i.min(self.panes.len() - 1);
        let out = f(self);
        self.at = was;
        out
    }

    /// The panes, in the order they were made.
    pub fn panes(&self) -> &[Pane] {
        &self.panes
    }

    /// The focused pane, by its place in `panes`.
    pub fn focused(&self) -> usize {
        self.at
    }

    /// The place in `panes` of the pane `id`.
    pub fn pane_index(&self, id: u64) -> Option<usize> {
        self.panes.iter().position(|p| p.id == id)
    }

    /// Act on pane `i` (to draw it) until told otherwise; the pane acted on before.
    pub fn act_on(&mut self, i: usize) -> usize {
        let was = self.at;
        self.at = i.min(self.panes.len() - 1);
        was
    }

    /// Give pane `i` the keys, the sidebar and the dock; one of the split's, the split back.
    pub fn focus_pane(&mut self, i: usize) {
        let Some(id) = self.panes.get(i).map(|p| p.id) else {
            return;
        };
        if Some(id) != self.solo {
            self.leave_solo();
        }
        self.at = self.pane_index(id).unwrap_or(0);
    }

    /// The view shown alone put away: the split again, on the pane it was left on.
    fn leave_solo(&mut self) {
        if let Some(id) = self.solo.take() {
            self.panes.retain(|p| p.id != id);
            self.at = self.pane_index(self.split_focus).unwrap_or(0);
            self.pane_gone(id);
        }
    }

    /// What the middle shows: the split, or the view shown alone over it.
    pub fn shown_split(&self) -> Split {
        match self.solo {
            Some(id) => Split::Pane(id),
            None => self.split.clone(),
        }
    }

    /// The split's panes in reading order, each with its view (none for a placeholder), while
    /// there is a split: what the sidebar groups at its top.
    pub fn split_views(&self) -> Vec<(u64, Option<View>)> {
        let ids = self.split.ids();
        if ids.len() < 2 {
            return Vec::new();
        }
        ids.into_iter()
            .filter_map(|id| {
                let p = self.panes.iter().find(|p| p.id == id)?;
                Some((id, (!p.placeholder).then_some(p.view)))
            })
            .collect()
    }

    /// Take pane `id` out of the split (its view goes back to standing alone in the sidebar),
    /// the focus staying where it is, or, on that pane, going to the one before it.
    pub fn unsplit(&mut self, id: u64) {
        let order = self.split.ids();
        let Some(here) = order.iter().position(|p| *p == id) else {
            return;
        };
        if order.len() < 2 {
            return;
        }
        let neighbour = if here > 0 { order[here - 1] } else { order[1] };
        let focused = self.pane().id;
        if !self.split.remove(id) {
            return;
        }
        self.panes.retain(|p| p.id != id);
        self.pane_gone(id);
        if self.split_focus == id {
            self.split_focus = neighbour;
        }
        let keep = if focused == id { neighbour } else { focused };
        self.at = self.pane_index(keep).unwrap_or(0);
        // No split left under a view shown alone: that view is the one view.
        if let (Some(solo), Split::Pane(left)) = (self.solo, self.split.clone()) {
            self.panes.retain(|p| p.id != left);
            self.pane_gone(left);
            self.split = Split::Pane(solo);
            self.solo = None;
            self.at = self.pane_index(solo).unwrap_or(0);
        }
    }

    /// The pane the keys are in, by id.
    pub fn focused_id(&self) -> u64 {
        self.pane().id
    }

    /// Whether a view shows alone over the split.
    pub fn solo(&self) -> bool {
        self.solo.is_some()
    }

    /// The pane `delta` along from the focused one, in reading order, round the ends.
    pub fn focus_next_pane(&mut self, delta: isize) {
        // From a view shown alone, back to the split.
        if self.solo.is_some() {
            self.leave_solo();
            return;
        }
        let order = self.split.ids();
        let id = self.pane().id;
        let Some(here) = order.iter().position(|p| *p == id) else {
            return;
        };
        let n = order.len() as isize;
        let to = order[(here as isize + delta).rem_euclid(n) as usize];
        if let Some(i) = self.pane_index(to) {
            self.at = i;
        }
    }

    /// Split the focused pane along `axis`: a placeholder beside it (after it), focused.
    pub fn split_pane(&mut self, axis: Axis) {
        // A view shown alone joins the split, beside the pane it was left on.
        if let Some(solo) = self.solo {
            if self.split.split(self.split_focus, axis, solo) {
                self.solo = None;
            }
            return;
        }
        let id = self.next_pane;
        if self.split.split(self.pane().id, axis, id) {
            self.next_pane += 1;
            let mut pane = Pane::new(id, View::Inbox);
            pane.placeholder = true;
            self.panes.push(pane);
            self.at = self.panes.len() - 1;
        }
    }

    /// Close the focused pane, unless it is the last; the one before it (else after) is
    /// focused.
    pub fn close_pane(&mut self) {
        // A view shown alone closes back to the split.
        if self.solo.is_some() {
            self.leave_solo();
            return;
        }
        if self.panes.len() < 2 {
            return;
        }
        let id = self.pane().id;
        let order = self.split.ids();
        let here = order.iter().position(|p| *p == id).unwrap_or(0);
        let next = if here > 0 { order[here - 1] } else { order[1] };
        if self.split.remove(id) {
            self.panes.retain(|p| p.id != id);
            self.at = self.pane_index(next).unwrap_or(0);
            self.pane_gone(id);
        }
    }

    /// Whether the pane acted on waits for a view to be chosen.
    pub fn placeholder(&self) -> bool {
        self.pane().placeholder
    }

    /// The view of the pane acted on.
    pub fn view(&self) -> View {
        self.pane().view
    }

    pub fn layout(&self) -> Layout {
        self.pane().layout
    }

    /// The rows the pane acted on shows, in display order.
    pub fn rows(&self) -> &[Summary] {
        &self.pane().rows
    }

    /// Grouped by project, the projects the rows are part of, in key order.
    pub fn project_names(&self) -> &[(Uuid, Brief)] {
        &self.pane().project_names
    }

    /// In the projects view, a project's progress.
    pub fn project_progress(&self, id: Uuid) -> Option<&livtk_core::Progress> {
        self.pane().projects.get(&id)
    }

    pub fn groups(&self) -> &[Group] {
        &self.pane().groups
    }

    /// Every row after filtering, collapsed or not: what the board shows.
    pub fn filtered(&self) -> &[Summary] {
        &self.pane().filtered
    }

    pub fn leaving(&self) -> &[(Summary, usize, Instant)] {
        &self.pane().leaving
    }

    /// How deep each nested sub-task sits under its parent.
    pub fn depth(&self) -> &HashMap<Uuid, usize> {
        &self.pane().depth
    }

    /// Each parent's sub-tasks: how many are done, of how many.
    pub fn progress(&self) -> &HashMap<Uuid, (usize, usize)> {
        &self.pane().progress
    }

    /// The sidebar's views for the open project: the built-in ones, with the kinds it declares
    /// after the findings.
    pub fn views(&self) -> Vec<View> {
        let declared: Vec<View> = self
            .project()
            .map(|p| {
                p.ws.config()
                    .kinds
                    .iter()
                    .filter_map(|k| Kind::named(&k.name))
                    .map(View::Of)
                    .collect()
            })
            .unwrap_or_default();
        let mut out = Vec::with_capacity(View::ALL.len() + declared.len());
        for v in View::ALL {
            out.push(v);
            if v == View::Findings {
                out.extend(declared.iter().copied());
            }
        }
        out
    }

    /// Change the open project's settings through `edit` (which writes its config, checked);
    /// everything shown read again after. What went wrong, as the human should read it.
    pub fn edit_project(
        &mut self,
        edit: impl FnOnce(&mut Workspace) -> livtk_core::Result<()>,
    ) -> Result<(), String> {
        let Some(p) = self.projects.get_mut(self.current) else {
            return Err("no project is open".into());
        };
        edit(&mut p.ws).map_err(|e| e.to_string())?;
        self.reload();
        Ok(())
    }

    /// The open project's config, as it was last read.
    pub fn config(&self) -> Option<&livtk_core::Config> {
        self.project().map(|p| p.ws.config())
    }

    /// Write the open project's markdown export to its default folder.
    pub fn export(&self) -> Option<livtk_core::Result<livtk_core::export::Report>> {
        let p = self.project()?;
        Some(livtk_core::export::export(&p.ws, None, &p.dir, false))
    }

    /// The relations the open project declares.
    pub fn declared_rels(&self) -> Vec<Rel> {
        self.project()
            .map(|p| declared_rels(&p.ws))
            .unwrap_or_default()
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
            // Every pane starts afresh in the other project.
            for p in &mut self.panes {
                p.selected = None;
                p.band = None;
                p.leaving.clear();
            }
            self.detail = None;
            self.grid.pick.clear();
            self.flashed.clear();
            // The feed, wherever it shows, marks what is new since its own last visit.
            if let Some(i) = self
                .panes
                .iter()
                .position(|p| !p.placeholder && p.view == View::Activity)
            {
                self.with_pane(i, Model::enter_activity);
            }
            self.enter_graph();
            self.reload();
            self.note_recent();
        }
    }

    /// The open project is the latest opened: first among the recent.
    fn note_recent(&mut self) {
        let Some(dir) = self.project().map(|p| p.dir.clone()) else {
            return;
        };
        self.recent.retain(|d| d != &dir);
        self.recent.insert(0, dir);
        self.recent.truncate(RECENT_KEPT);
    }

    /// The projects the top bar's menu offers, as indexes into `projects`: the open one, then
    /// the rest most recently opened first (those never opened after them, as registered), at
    /// most `RECENT_SHOWN`.
    pub fn recent_projects(&self) -> Vec<usize> {
        let mut order: Vec<usize> = Vec::new();
        let take = |i: usize, order: &mut Vec<usize>| {
            if i < self.projects.len() && !order.contains(&i) {
                order.push(i);
            }
        };
        take(self.current, &mut order);
        for dir in &self.recent {
            if let Some(i) = self.projects.iter().position(|p| &p.dir == dir) {
                take(i, &mut order);
            }
        }
        for i in 0..self.projects.len() {
            take(i, &mut order);
        }
        order.truncate(RECENT_SHOWN);
        order
    }

    /// Show `view` in the focused pane; shown in another pane already, that pane is focused
    /// instead, as each view shows in one pane at most.
    pub fn set_view(&mut self, view: View) {
        // In a pane of the split: that pane, the split back if a view was shown alone over it.
        let ids = self.split.ids();
        if let Some(id) = self
            .panes
            .iter()
            .find(|p| !p.placeholder && p.view == view && ids.contains(&p.id))
            .map(|p| p.id)
        {
            self.leave_solo();
            self.at = self.pane_index(id).unwrap_or(0);
            return;
        }
        // In none of a split's panes, and asked for from one that shows a view: it shows alone
        // over the split, which is kept to come back to. (A placeholder takes it in.)
        if ids.len() > 1 && self.solo.is_none() && !self.pane().placeholder {
            self.split_focus = self.pane().id;
            let id = self.next_pane;
            self.next_pane += 1;
            self.panes.push(Pane::new(id, view));
            self.solo = Some(id);
            self.at = self.panes.len() - 1;
            self.detail = None;
            if view == View::Activity {
                self.enter_activity();
            }
            if view == View::Graph {
                self.enter_graph();
            }
            self.reload();
            return;
        }
        if view != self.pane().view || self.pane().placeholder {
            let p = self.pane_mut();
            p.placeholder = false;
            p.view = view;
            p.selected = None;
            p.band = None;
            p.leaving.clear();
            self.detail = None;
            if view == View::Activity {
                self.enter_activity();
            }
            if view == View::Graph {
                self.enter_graph();
            }
            self.reload();
        }
    }

    /// List or board, as chosen from the View options menu.
    pub fn set_layout(&mut self, layout: Layout) {
        if self.pane().layout != layout {
            self.toggle_layout();
        }
    }

    pub fn toggle_layout(&mut self) {
        // A band is a list's; the board has none.
        self.pane_mut().band = None;
        self.pane_mut().layout = match self.pane().layout {
            Layout::List => Layout::Board,
            Layout::Board => Layout::List,
        };
        // A board needs columns: as in Linear, it is never ungrouped.
        if self.pane().layout == Layout::Board && self.display().grouping == Grouping::None {
            self.set_grouping(Grouping::State);
        }
    }

    // ---- selection ----------------------------------------------------------------------------

    /// The selected node: a row of the list, or on the board any card, a folded group's too.
    pub fn selected(&self) -> Option<&Summary> {
        let id = self.pane().selected?;
        let shown = if self.on_board() {
            &self.pane().filtered
        } else {
            &self.pane().rows
        };
        shown.iter().find(|r| r.id == id)
    }

    pub fn selected_index(&self) -> Option<usize> {
        let id = self.pane().selected?;
        self.pane().rows.iter().position(|r| r.id == id)
    }

    pub fn select(&mut self, id: Uuid) {
        if self.pane().rows.iter().any(|r| r.id == id) {
            self.pane_mut().band = None;
            self.pane_mut().selected = Some(id);
            self.follow_detail();
        }
    }

    /// The group band the selection rests on, if it rests on one.
    pub fn band(&self) -> Option<GroupKey> {
        self.pane().band
    }

    /// Whether the view is the tasks' board.
    pub fn on_board(&self) -> bool {
        self.pane().view.has_board() && self.pane().layout == Layout::Board
    }

    /// Move the selection by `delta`, stopping at the ends: in a list through its group bands
    /// and their rows; on the board down a column and on into the next (up into the previous).
    pub fn select_by(&mut self, delta: isize) {
        if self.pane().view == View::Docs {
            self.docs_select_by(delta);
            return;
        }
        if self.pane().view == View::Activity {
            self.activity_select_by(delta);
            return;
        }
        if self.pane().view == View::Graph {
            self.graph_select_by(delta);
            return;
        }
        if self.on_board() {
            self.board_select_by(delta);
            return;
        }
        // A list's stops: each group's band (when grouped), then its rows shown.
        let mut stops: Vec<(Option<GroupKey>, Option<Uuid>)> = Vec::new();
        for g in &self.pane().groups {
            if g.key != GroupKey::All {
                stops.push((Some(g.key), None));
            }
            for r in &self.pane().rows[g.start..g.start + g.shown] {
                stops.push((None, Some(r.id)));
            }
        }
        if stops.is_empty() {
            return;
        }
        let at = stops.iter().position(|(band, row)| match self.pane().band {
            Some(k) => *band == Some(k),
            None => row.is_some() && *row == self.pane().selected,
        });
        let next = match at {
            None => 0,
            Some(i) => i.saturating_add_signed(delta).min(stops.len() - 1),
        };
        match stops[next] {
            (Some(k), _) => {
                self.pane_mut().band = Some(k);
                self.pane_mut().selected = None;
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
        let at = self.pane().selected.and_then(|id| {
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
        // In the docs' list, across is in and out of the folders; reading, it is nothing.
        if self.pane().view == View::Docs {
            if self.docs.reading {
                return;
            }
            if delta < 0 {
                self.docs_left();
            } else {
                self.docs_right();
            }
            return;
        }
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
        self.pane_mut().band = None;
        self.pane_mut().selected = Some(id);
        self.follow_detail();
    }

    /// Enter: fold or unfold the band the selection rests on; else open the selected node.
    pub fn enter(&mut self) {
        if self.pane().view == View::Docs {
            self.docs_enter();
            return;
        }
        match self.pane().band {
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
            parent: self
                .pane()
                .all
                .iter()
                .find(|r| r.id == n.id)
                .and_then(|r| r.parent),
        })
    }

    // ---- the detail panel ---------------------------------------------------------------------

    pub fn open_detail(&mut self) {
        if self.pane().view == View::Activity {
            // The feed's highlighted event opens its node, and the feed stays.
            self.pane_mut().selected = self.activity_node();
        }
        if self.pane().view == View::Graph {
            self.pane_mut().selected = self.graph_node();
        }
        let Some(id) = self.pane().selected else {
            return;
        };
        let Some(project) = self.projects.get(self.current) else {
            return;
        };
        match load_detail(&project.ws, id) {
            Ok(d) => {
                // A detail opens in the pane it is opened from: a full page shows there.
                self.detail_pane = Some(self.pane().id);
                self.detail = Some(d);
            }
            Err(e) => self.warn(e.to_string()),
        }
    }

    /// The selection moved with a detail open: the detail follows it, unless it is a full page
    /// in another pane (whose list is not this one).
    fn follow_detail(&mut self) {
        let here = self.pane().id;
        if self.detail.is_some()
            && (self.detail_mode != DetailMode::Full || self.detail_pane == Some(here))
        {
            self.open_detail();
        }
    }

    /// Pane `id` is gone: a detail shown as a full page there goes with it.
    fn pane_gone(&mut self, id: u64) {
        if self.detail_pane == Some(id) {
            self.detail_pane = None;
            if self.detail_mode == DetailMode::Full {
                self.detail = None;
            }
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

    /// Put task `id` in project `project` (by key), or out of any (`None`). A refusal (a sub-task,
    /// whose project is its parent's) becomes the notice.
    pub fn set_project_of(&mut self, id: Uuid, project: Option<String>) {
        let Some(p) = self.projects.get_mut(self.current) else {
            return;
        };
        match p
            .ws
            .set_project_of(&id.to_string(), project.as_deref(), &Actor::Human)
        {
            Ok(_) => {
                self.toasts.clear_errors();
                self.reload();
            }
            Err(e) => self.warn(e.to_string()),
        }
    }

    /// Give task `id` to agent `name` (as a run takes it), or take it back (`None`).
    pub fn set_agent(&mut self, id: Uuid, name: Option<String>) {
        self.run(
            Command::SetAgent {
                node: id.to_string(),
                agent: name.unwrap_or_default(),
            },
            None,
        );
    }

    /// Whether task `id`'s builds work in their own worktree (`Some`), or as the project says.
    pub fn set_worktree(&mut self, id: Uuid, worktree: Option<bool>) {
        self.run(
            Command::SetWorktree {
                node: id.to_string(),
                worktree,
            },
            None,
        );
    }

    /// Whether the project's builds work in their own worktree, unless a task says otherwise.
    pub fn project_worktree(&self) -> bool {
        self.projects
            .get(self.current)
            .is_none_or(|p| p.ws.config().runs.worktree)
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
            // A run waiting on the task reads it next: at once, not a beat later.
            self.survey_runs_soon();
        }
    }

    /// Run `cmd` as the human; whether it was made.
    fn run(&mut self, cmd: Command, row: Option<Summary>) -> bool {
        // Only a row that was on screen can fade out of it, from where it was.
        let at = row
            .as_ref()
            .and_then(|r| self.pane().rows.iter().position(|x| x.id == r.id));
        let row = row.filter(|_| at.is_some());
        let was_selected = row
            .as_ref()
            .is_some_and(|r| self.pane().selected == Some(r.id));
        let Some(project) = self.projects.get_mut(self.current) else {
            return false;
        };
        let before = project.ws.newest_event().ok().flatten();
        match project.ws.execute(cmd, &Actor::Human) {
            Ok(_) => {
                if !project.ws.config().on.is_empty() {
                    self.wrote.push((project.dir.clone(), before));
                }
                self.toasts.clear_errors();
                self.reload();
                // A row that left the view (acknowledged out of the inbox, say) fades out, and
                // the selection moves to the row that took its place.
                if let Some(row) = row
                    && !self.pane().rows.iter().any(|r| r.id == row.id)
                {
                    self.pane_mut()
                        .leaving
                        .push((row, at.unwrap_or(0), Instant::now()));
                    // The selection moves on only if it was on the row that left.
                    if was_selected
                        && let Some(i) = at
                        && !self.pane().rows.is_empty()
                    {
                        let id = self.pane().rows[i.min(self.pane().rows.len() - 1)].id;
                        self.pane_mut().selected = Some(id);
                    }
                    self.follow_detail();
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
        let mut failed = Vec::new();
        let mut recovered = Vec::new();
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
            let source = failure_source(&p.dir);
            let was_failing = p.failure.is_some();
            match p.went(failure) {
                Some(text) => failed.push((sync_failed(&p.name, &text), source)),
                None if was_failing && p.failure.is_none() => recovered.push(source),
                None => {}
            }
        }
        // A project that works again takes back what its failure told.
        for source in recovered {
            self.toasts.clear_source(&source);
        }
        for (toast, source) in failed {
            self.notify(toast.source(source));
        }
        if !changed {
            return began;
        }
        let before: HashMap<Uuid, String> = self
            .panes
            .iter()
            .flat_map(|p| &p.rows)
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
        for r in self.panes.iter().flat_map(|p| &p.rows) {
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

    /// The projects written to that have hooks to run, each once, with the newest event before
    /// the first of those writes (so a hook new to them hears them all).
    pub fn take_written(&mut self) -> Vec<(PathBuf, Option<Uuid>)> {
        let mut out: Vec<(PathBuf, Option<Uuid>)> = Vec::new();
        for (dir, before) in std::mem::take(&mut self.wrote) {
            if !out.iter().any(|(d, _)| *d == dir) {
                out.push((dir, before));
            }
        }
        out
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
        let source = failure_source(&p.dir);
        let was_failing = p.failure.is_some();
        match p.went(failure) {
            Some(text) => {
                let toast = sync_failed(&p.name, &text).source(source);
                self.notify(toast);
            }
            None if was_failing && p.failure.is_none() => self.toasts.clear_source(&source),
            None => {}
        }
    }

    /// Forget visual state older than `age`: finished flashes and fades.
    pub fn settle(&mut self, now: Instant, age: std::time::Duration) {
        self.flashed.retain(|_, at| now.duration_since(*at) < age);
        for p in &mut self.panes {
            p.leaving.retain(|(_, _, at)| now.duration_since(*at) < age);
        }
    }

    // ---- notifications -------------------------------------------------------------------------

    pub fn toasts(&self) -> &Toasts {
        &self.toasts
    }

    /// The newest notification.
    pub fn notice(&self) -> Option<&Toast> {
        self.toasts.latest()
    }

    /// Tell the human something: the one way every notification is raised. Its id.
    pub fn notify(&mut self, t: Toast) -> u64 {
        self.toasts.raise(t)
    }

    /// Tell the human something went wrong: it stays until closed, or until what it told of no
    /// longer holds.
    pub fn warn(&mut self, text: impl Into<String>) {
        self.notify(Toast::error(text));
    }

    /// Tell the human something worth knowing that is no error.
    pub fn inform(&mut self, text: impl Into<String>) {
        self.notify(Toast::info(text));
    }

    /// Tell the human why this app posts no system notifications where it runs, and where it
    /// would. Running outside the bundle is told once ever; a bundle that would not open, each
    /// time. While something else is being told, it waits for another time.
    pub fn tell_quiet(&mut self, why: Quiet) {
        if !self.toasts.is_empty() || (why == Quiet::NoBundle && self.told_no_notifications) {
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
        self.notify(Toast::warning(text).about("livtk", "Notifications"));
    }

    /// Close every notification.
    pub fn dismiss_all(&mut self) {
        self.toasts.dismiss_all();
    }

    /// Close notification `id`: the human read it.
    pub fn dismiss(&mut self, id: u64) {
        self.toasts.dismiss(id);
    }

    /// The pointer came over notification `id` (`held`), or left it at `now`.
    pub fn hold(&mut self, id: u64, held: bool, now: Instant) {
        self.toasts.hold(id, held, now);
    }

    /// Let the notifications' time pass to `now`; whether any changed.
    pub fn age_notices(&mut self, now: Instant, fade: Duration) -> bool {
        self.toasts.age(now, fade)
    }

    // ---- display options -----------------------------------------------------------------------

    pub fn display(&self) -> Display {
        self.displays
            .get(&self.pane().view)
            .cloned()
            .unwrap_or_else(|| Display::for_view(self.pane().view))
    }

    fn edit_display(&mut self, edit: impl FnOnce(&mut Display)) {
        let mut d = self.display();
        edit(&mut d);
        self.displays.insert(self.pane().view, d);
        // Grouped by project now: which project each row is part of.
        self.load_projects();
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
        if let Some(k) = self.pane().band {
            if self.pane().groups.iter().any(|g| g.key == k) {
                return;
            }
            self.pane_mut().band = None;
        }
        if self.selected().is_none() {
            let panel = self
                .detail
                .as_ref()
                .map(|d| d.node.id)
                .filter(|id| self.pane().rows.iter().any(|r| r.id == *id));
            match panel.or_else(|| self.pane().rows.first().map(|r| r.id)) {
                Some(id) => self.select(id),
                None => self.pane_mut().selected = None,
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
        let kind = self.pane().view.kind();
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
        // What the view does not offer is not taken, whoever asks: the palette, a saved view.
        if (self.pane().layout == Layout::Board && grouping == Grouping::None)
            || !self.pane().view.offers_grouping(grouping)
        {
            return;
        }
        self.edit_display(|d| d.grouping = grouping);
    }

    pub fn set_ordering(&mut self, ordering: Ordering) {
        if !self.pane().view.offers_ordering(ordering) {
            return;
        }
        self.edit_display(|d| d.ordering = ordering);
    }

    /// Show or hide the current view's empty groups, where it lists groups of one kind.
    pub fn toggle_empty(&mut self) {
        if self.pane().view.kind().is_some() {
            self.edit_display(|d| d.show_empty = !d.show_empty);
        }
    }

    /// Show or hide the current view's dismissed work, where it has any. What a view holds is
    /// read without it, so it is read again.
    pub fn toggle_dismissed(&mut self) {
        if self.pane().view.shows_dismissed() {
            let mut d = self.display();
            d.show_dismissed = !d.show_dismissed;
            // A dismissed state chosen in the filter would show nothing while hidden.
            if !d.show_dismissed {
                let kind = self.pane().view.kind();
                d.states
                    .retain(|&s| !kind.map_or(s.is_dismissed(), |k| k.is_dismissed(s)));
            }
            self.displays.insert(self.pane().view, d);
            self.reload();
        }
    }

    pub fn toggle_collapsed(&mut self, key: GroupKey) {
        if !self.collapsed.remove(&(self.pane().view, key)) {
            self.collapsed.insert((self.pane().view, key));
        }
        self.arrange();
        self.reselect();
    }

    /// Apply the view's display options to everything it holds.
    fn arrange(&mut self) {
        let d = self.display();
        let needle = d.filter.trim().to_lowercase();
        let mut rows: Vec<Summary> = self
            .pane()
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
        let project_of = self.pane().project_of.clone();
        let group_of = |r: &Summary| match d.grouping {
            Grouping::State => GroupKey::State(r.state),
            Grouping::Priority => GroupKey::Priority(r.priority),
            Grouping::Project => GroupKey::Project(project_of.get(&r.id).copied()),
            Grouping::None => GroupKey::All,
        };
        let kind = self.pane().view.kind();
        let names = self.pane().project_names.clone();
        let group_rank = |k: GroupKey| match k {
            GroupKey::State(s) => state_order(kind, s),
            GroupKey::Priority(p) => p,
            // Projects in key order, then the tasks part of none.
            GroupKey::Project(Some(id)) => names
                .iter()
                .position(|(p, _)| *p == id)
                .map_or(u8::MAX - 1, |i| u8::try_from(i).unwrap_or(u8::MAX - 1)),
            GroupKey::Project(None) => u8::MAX,
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
        self.pane_mut().progress = progress(&self.pane().all);
        let (rows, depth) = if d.nest {
            nest(rows, |a, b| group_of(a) == group_of(b))
        } else {
            (rows, HashMap::new())
        };
        self.pane_mut().depth = depth;
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
            let collapsed = self.collapsed.contains(&(self.pane().view, key));
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
                kind: self.pane().view.kind(),
                count,
                collapsed,
                start,
                shown: shown.len() - start,
            });
        }
        self.pane_mut().filtered = rows;
        self.pane_mut().rows = shown;
        self.pane_mut().groups = groups;
    }

    /// The key of `r`'s parent, when `r` is a sub-task shown on its own (not under it): its
    /// parent is in another group, filtered out, or nesting is off.
    pub fn parent_key(&self, r: &Summary) -> Option<&str> {
        if self.pane().depth.contains_key(&r.id) {
            return None;
        }
        self.parent_of(r)
    }

    /// The key of `r`'s parent, if `r` is a sub-task (as a board card names it).
    pub fn parent_of(&self, r: &Summary) -> Option<&str> {
        let parent = r.parent?;
        self.pane()
            .all
            .iter()
            .find(|p| p.id == parent)
            .map(|p| p.key.as_str())
    }

    /// Nest sub-tasks under their parents, or not.
    pub fn toggle_nest(&mut self) {
        self.edit_display(|d| d.nest = !d.nest);
    }

    /// Every group the view's grouping has, in no order: each state of the view's kind, the
    /// dismissed ones only while shown, or each priority. None where the view is ungrouped or
    /// mixes kinds, as the inbox does.
    fn every_group(&self, d: &Display) -> Vec<GroupKey> {
        match (d.grouping, self.pane().view.kind()) {
            (Grouping::State, Some(kind)) => kind
                .states()
                .iter()
                .filter(|&&s| d.show_dismissed || !kind.is_dismissed(s))
                .filter(|s| d.states.is_empty() || d.states.contains(s))
                .map(|&s| GroupKey::State(s))
                .collect(),
            (Grouping::Priority, Some(_)) => (1..=4).map(GroupKey::Priority).collect(),
            (Grouping::Project, Some(_)) => self
                .pane()
                .project_names
                .iter()
                .map(|(id, _)| GroupKey::Project(Some(*id)))
                .chain([GroupKey::Project(None)])
                .collect(),
            _ => Vec::new(),
        }
    }

    // ---- drag and drop ------------------------------------------------------------------------

    /// Whether dropping node `id` on `target` is a legal change.
    pub fn can_drop(&self, id: Uuid, target: GroupKey) -> bool {
        self.pane()
            .all
            .iter()
            .find(|r| r.id == id)
            .is_some_and(|r| drop_allowed(r.kind, r.state, r.priority, target))
    }

    /// Drop node `id` on a group or column: move it to that state, or give it that priority.
    /// A move the rules refuse becomes the notice, with the core's reason.
    pub fn drop_on(&mut self, id: Uuid, target: GroupKey) {
        let Some(r) = self.pane().all.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        // The card dragged is what the human acts on next: the panel follows it too, even
        // when its group is folded in the list (the board shows it all the same).
        self.pane_mut().selected = Some(id);
        self.follow_detail();
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
        } else if d.grouping == Grouping::Project {
            self.every_group(&d)
        } else {
            // The kind's states in their own order, as the list's groups; dismissed ones only
            // while shown. The filter says what the board is about: no column for a state it
            // leaves out.
            let kind = self.pane().view.kind().unwrap_or(Kind::Task);
            let mut states: Vec<State> = kind
                .states()
                .iter()
                .copied()
                .filter(|&s| d.show_dismissed || !kind.is_dismissed(s))
                .filter(|s| d.states.is_empty() || d.states.contains(s))
                .collect();
            states.sort_by_key(|s| state_order(Some(kind), *s));
            states.into_iter().map(GroupKey::State).collect()
        };
        keys.into_iter()
            .map(|key| {
                let rows = self
                    .pane()
                    .filtered
                    .iter()
                    .filter(|r| match key {
                        GroupKey::State(s) => r.state == s,
                        GroupKey::Priority(p) => r.priority == p,
                        GroupKey::Project(p) => self.pane().project_of.get(&r.id).copied() == p,
                        GroupKey::All => true,
                    })
                    .collect::<Vec<_>>();
                (key, rows)
            })
            .filter(|(_, rows)| d.show_empty || !rows.is_empty())
            .collect()
    }

    /// Read everything shown again: what every pane shows, the sidebar's counts, the panel.
    fn reload(&mut self) {
        self.load_grid();
        // The panes first: the feed, looked at, marks what it shows seen before it is counted.
        for i in 0..self.panes.len() {
            self.with_pane(i, Model::reload_pane);
        }
        for p in &mut self.projects {
            p.inbox_items = p.ws.inbox().unwrap_or_default();
            p.inbox = p.inbox_items.len();
            p.unseen = p.ws.unseen_activity().unwrap_or(0);
        }
        if let Some(id) = self.detail.as_ref().map(|d| d.node.id)
            && let Some(project) = self.projects.get(self.current)
        {
            self.detail = load_detail(&project.ws, id).ok();
        }
    }

    /// What the pane needs of projects: each project's progress in the projects view, and the
    /// project each row is part of while grouped by project. Nothing, at no cost, otherwise.
    fn load_projects(&mut self) {
        let view = self.pane().view;
        let by_project = self.display().grouping == Grouping::Project;
        let Some(project) = self.projects.get(self.current) else {
            return;
        };
        let mut progress = HashMap::new();
        if view == View::Projects {
            let now = jiff::Timestamp::now();
            for r in &self.pane().all {
                if let Ok(g) = project.ws.progress(&r.key, now) {
                    progress.insert(r.id, g);
                }
            }
        }
        let (mut of, mut names) = (HashMap::new(), Vec::new());
        if by_project && let Ok(map) = project.ws.projects_of() {
            for (node, (id, brief)) in map {
                of.insert(node, id);
                if !names.iter().any(|(p, _): &(Uuid, Brief)| *p == id) {
                    names.push((id, brief));
                }
            }
            // In key order: PRJ-2 before PRJ-10.
            names.sort_by_key(|(_, b)| {
                b.key
                    .rsplit('-')
                    .next()
                    .and_then(|n| n.parse::<u32>().ok())
                    .unwrap_or(u32::MAX)
            });
        }
        let pane = self.pane_mut();
        pane.projects = progress;
        pane.project_of = of;
        pane.project_names = names;
    }

    /// How many projects are active, when more are than is wise at once.
    pub fn too_many_active(&self) -> Option<usize> {
        if self.pane().view != View::Projects {
            return None;
        }
        let active = self
            .pane()
            .all
            .iter()
            .filter(|r| r.state == State::Active)
            .count();
        (active > livtk_core::ACTIVE_AT_ONCE).then_some(active)
    }

    /// Read again what the pane acted on shows.
    fn reload_pane(&mut self) {
        if self.pane().placeholder {
            return;
        }
        if self.pane().view == View::Docs {
            self.load_docs();
        }
        if self.pane().view == View::Agents {
            self.load_agents();
        }
        if self.pane().view == View::Activity {
            self.load_activity();
        }
        if self.pane().view == View::Graph {
            self.load_graph();
        }
        let Some(project) = self.projects.get(self.current) else {
            self.pane_mut().rows.clear();
            return;
        };
        let rows = match self.pane().view.kind() {
            // The feed lists events, the graph lays nodes out, the docs and the terminal are
            // their own: none has rows.
            None if self.pane().view != View::Inbox => Ok(Vec::new()),
            None => project.ws.inbox(),
            Some(kind) => {
                let states: Vec<State> = if self.display().show_dismissed {
                    Vec::new()
                } else {
                    kind.states()
                        .iter()
                        .copied()
                        .filter(|&s| !kind.is_dismissed(s))
                        .collect()
                };
                project.ws.list(kind, &states)
            }
        };
        match rows {
            Ok(rows) => self.pane_mut().all = rows,
            Err(e) => self.warn(e.to_string()),
        }
        self.load_projects();
        self.arrange();
        if self.selected().is_none() {
            self.pane_mut().selected = self.pane().rows.first().map(|r| r.id);
        }
    }
}

/// Whether a node of `kind` in `state` with `priority` may be dropped on `target`. One rule for
/// the model and for the view's drop highlighting.
pub fn drop_allowed(kind: Kind, state: State, priority: u8, target: GroupKey) -> bool {
    match target {
        GroupKey::State(s) => state != s && kind.can_move(state, s),
        GroupKey::Priority(p) => kind == Kind::Task && priority != p,
        // A task joins a project by a link, from its panel, not by a drop.
        GroupKey::Project(_) | GroupKey::All => false,
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
            (Kind::Project, State::Active) if state == State::Proposed => "Commit to it",
            (Kind::Project, State::Active) if state == State::Paused => "Resume",
            (Kind::Project, State::Active) => "Reopen",
            (Kind::Project, State::Done) => "Done",
            (Kind::Project, State::Paused) => "Pause",
            (Kind::Project, State::Dropped) => "Drop",
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
    // A project's parts: the projects in it, then its tasks by state, in working order.
    let parts_groups: Vec<LinkGroup> = if node.kind == Kind::Project {
        let mut out = vec![group(
            "Projects",
            Rel::Parent,
            false,
            &parts
                .iter()
                .filter(|b| b.kind == Kind::Project)
                .cloned()
                .collect(),
        )];
        for &s in Kind::Task.states() {
            let at: Vec<Brief> = parts
                .iter()
                .filter(|b| b.kind == Kind::Task && b.state == s)
                .cloned()
                .collect();
            out.push(group(state_name(s), Rel::Parent, false, &at));
        }
        out
    } else {
        vec![group("Sub-tasks", Rel::Parent, false, &parts)]
    };
    let project = if node.kind == Kind::Project {
        let now = jiff::Timestamp::now();
        Some((
            ws.progress(&key, now)?,
            ws.updates(&key)?,
            ws.flow(Some(&key), now)?,
        ))
    } else {
        None
    };
    let link_groups: Vec<LinkGroup> = [group("Part of", Rel::Parent, true, &part_of)]
        .into_iter()
        .chain(parts_groups)
        .chain([
            group("Waits for", Rel::DependsOn, true, &links.depends_on),
            group("Blocks", Rel::DependsOn, false, &links.blocks),
            group("Related to", Rel::Relates, true, &related),
            group("Replaces", Rel::Supersedes, true, &replaces),
            group("Replaced by", Rel::Supersedes, false, &replaced_by),
            group("Learned from", Rel::LearnedFrom, true, &learned_from),
            group("Taught", Rel::LearnedFrom, false, &taught),
        ])
        // The project's own relations, each from either end: by its name, and by its inverse.
        .chain(declared_rels(ws).into_iter().flat_map(|rel| {
            let (to, from) = ws.related(&key, rel).unwrap_or_default();
            [
                group(heading(rel.as_str()), rel, true, &to),
                group(
                    heading(rel.inverse().unwrap_or(rel.as_str())),
                    rel,
                    false,
                    &from,
                ),
            ]
        }))
        .filter(|g| !g.briefs.is_empty())
        .collect();
    // Where it sits: its project, directly or through its parent task.
    let in_project = ws.project_of(&key)?.map(|p| Brief {
        key: p.key,
        title: p.title,
        state: p.state,
        kind: p.kind,
    });
    let parent_task = part_of.first().filter(|b| b.kind != Kind::Project).cloned();
    // Its sub-tasks still to count: a canceled one is no longer part of the work.
    let tasks: Vec<&Brief> = parts
        .iter()
        .filter(|b| b.kind == Kind::Task && b.state != State::Canceled)
        .collect();
    let sub_tasks = (node.kind == Kind::Task && !tasks.is_empty()).then(|| {
        let done = tasks.iter().filter(|b| b.state == State::Done).count();
        (done, tasks.len())
    });
    Ok(Detail {
        in_project,
        parent_task,
        entered: ws.entered(&key)?,
        sub_tasks,
        choices: choices_with(node.kind, node.state, ws.gates()),
        links,
        history: ws.history(Some(&key))?,
        docs: ws.docs_of(&key)?,
        replaces,
        replaced_by,
        learned_from,
        link_groups,
        node,
        project,
        sessions: ws.sessions_of(&key).unwrap_or_default(),
        run: ws
            .runs()
            .unwrap_or_default()
            .into_iter()
            .rev()
            .find(|r| r.node == id),
    })
}

/// Each parent's sub-tasks among `all`: how many are done, of how many.
fn progress(all: &[Summary]) -> HashMap<Uuid, (usize, usize)> {
    let mut out: HashMap<Uuid, (usize, usize)> = HashMap::new();
    for r in all {
        if let Some(p) = r.parent {
            let e = out.entry(p).or_default();
            e.1 += 1;
            if r.state == State::Done {
                e.0 += 1;
            }
        }
    }
    out
}

/// How deep a sub-task nests at most: deeper ones sit at this depth under their parent.
const NEST_MAX: usize = 3;

/// `rows` (sorted) with each sub-task moved right after its parent, when both are shown and
/// `same_group`, keeping the order among siblings: the rows, and each nested one's depth.
fn nest(
    rows: Vec<Summary>,
    same_group: impl Fn(&Summary, &Summary) -> bool,
) -> (Vec<Summary>, HashMap<Uuid, usize>) {
    let at: HashMap<Uuid, usize> = rows.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    let mut kids: HashMap<Uuid, Vec<usize>> = HashMap::new();
    let mut nested = vec![false; rows.len()];
    for (i, r) in rows.iter().enumerate() {
        if let Some(p) = r.parent
            && let Some(&pi) = at.get(&p)
            && same_group(&rows[pi], r)
        {
            kids.entry(p).or_default().push(i);
            nested[i] = true;
        }
    }
    let mut order: Vec<usize> = Vec::with_capacity(rows.len());
    let mut depth = HashMap::new();
    let mut seen = vec![false; rows.len()];
    // Each top row in order, its sub-tasks depth first under it: (row, depth). Then any row a loop of parents left unreached (the core refuses such a loop; a log
    // edited by hand may not), at its own place rather than lost.
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let starts = (0..rows.len()).filter(|i| !nested[*i]).chain(0..rows.len());
    for start in starts.collect::<Vec<_>>() {
        if seen[start] {
            continue;
        }
        stack.push((start, 0));
        while let Some((i, d)) = stack.pop() {
            if std::mem::replace(&mut seen[i], true) {
                continue;
            }
            let id = rows[i].id;
            order.push(i);
            if d > 0 {
                depth.insert(id, d);
            }
            for &k in kids.get(&id).into_iter().flatten().rev() {
                stack.push((k, (d + 1).min(NEST_MAX)));
            }
        }
    }
    let mut slots: Vec<Option<Summary>> = rows.into_iter().map(Some).collect();
    let rows = order.into_iter().filter_map(|i| slots[i].take()).collect();
    (rows, depth)
}

/// What a project's failure is raised as, so it is taken back once the project works again.
fn failure_source(dir: &Path) -> String {
    format!("project:{}", dir.display())
}

/// A project's catching up failed, told as `failure` ("tasks: the disk is full"): its name first,
/// as the notification's breadcrumb, and the rest as its text.
fn sync_failed(name: &str, failure: &str) -> Toast {
    let text = failure
        .strip_prefix(name)
        .and_then(|t| t.strip_prefix(": "))
        .unwrap_or(failure);
    Toast::error(text).about(name, "Sync failed")
}
