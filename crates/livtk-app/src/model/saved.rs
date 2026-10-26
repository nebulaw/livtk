//! What the app remembers between runs: where the human was (project, view), how each view is
//! shown (filter, priorities, grouping, ordering, empty groups, dismissed work, collapsed
//! groups), list or board, each project's graph camera, and the window's own layout (the
//! sidebar's width and whether it is collapsed, the panel's width). Kept in `app.toml` in
//! livtk's config directory; GPU-free, so tested as plain Rust. The model restores its part; the
//! view restores the window's.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use livtk_core::State;
use serde::{Deserialize, Serialize};

use super::{Camera, Display, GroupKey, Grouping, Layout, Model, Ordering, Pane, Split, View};

/// Everything remembered, as written to `app.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    /// The open project's directory, and the projects' in the order they were last opened.
    pub project: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
    /// The doc last open in each project, by the project's folder.
    pub docs_open: BTreeMap<String, String>,
    /// The docs' folders folded in each project, by the project's folder.
    pub docs_shut: BTreeMap<String, Vec<String>>,
    /// Each project's graph camera (pan and zoom), by the project's folder.
    pub graph_cameras: BTreeMap<String, Camera>,
    pub view: Option<String>,
    /// "list" or "board".
    pub layout: Option<String>,
    /// Dismissed work shown in every view, as the app kept it before each view had its own: read
    /// from an older file, so its setting carries over, and never written.
    #[serde(skip_serializing)]
    pub show_all: bool,
    /// Each view's display, by the view's name.
    pub views: BTreeMap<String, SavedView>,
    /// The human was told this app posts no notifications where it ran: not again.
    pub told_no_notifications: bool,
    /// How docs read: "small", "regular" or "large", and compact or not.
    pub reading_size: Option<String>,
    pub reading_compact: bool,
    /// How a node's detail opens: "side", "center" or "full".
    pub detail_mode: Option<String>,
    /// Your on/off choices, each under its own name at the top, as before they were grouped.
    #[serde(flatten)]
    pub prefs: super::prefs::Prefs,
    /// The panes: how they are laid out, what each shows, and the one focused.
    pub panes: Option<SavedPanes>,
    pub window: SavedWindow,
}

/// The panes as remembered: the split by the panes' ids, each pane, the focused one's id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPanes {
    pub split: Split,
    pub panes: Vec<SavedPane>,
    pub focused: u64,
}

/// A pane as remembered: its view by name (none while it is a placeholder), list or board.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPane {
    pub id: u64,
    pub view: Option<String>,
    pub board: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedView {
    pub filter: String,
    pub priorities: Vec<u8>,
    /// The states the view is filtered to, by name.
    pub states: Vec<String>,
    pub grouping: String,
    pub ordering: String,
    pub show_empty: bool,
    pub show_dismissed: bool,
    /// Collapsed groups: "state:doing", "priority:2", "all".
    pub collapsed: Vec<String>,
    /// Sub-tasks shown flat rather than under their parents (nested unless set).
    pub flat: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedWindow {
    pub sidebar_width: Option<f32>,
    pub sidebar_collapsed: bool,
    pub panel_width: Option<f32>,
    pub bottom_open: bool,
    pub bottom_height: Option<f32>,
}

impl Saved {
    /// Where it lives: `app.toml` beside `projects.toml`.
    pub fn path() -> Option<PathBuf> {
        livtk_core::registry::config_dir().map(|d| d.join("app.toml"))
    }

    /// What was saved at `path`; nothing, if it is missing or unreadable (a newer format, say):
    /// the app starts as new rather than not at all.
    pub fn load(path: &Path) -> Saved {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Write it whole, through a temporary file, so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(tmp, path)
    }
}

/// Every view there is: the built-in ones, and one for each kind a project declares.
fn known_views() -> Vec<View> {
    View::ALL
        .into_iter()
        .chain(
            livtk_core::Kind::all()
                .into_iter()
                .filter(|k| k.is_declared())
                .map(View::Of),
        )
        .collect()
}

fn view_name(view: View) -> String {
    view.title().to_lowercase()
}

fn group_name(key: GroupKey) -> String {
    match key {
        GroupKey::State(s) => format!("state:{}", s.as_str()),
        GroupKey::Priority(p) => format!("priority:{p}"),
        GroupKey::Project(Some(id)) => format!("project:{id}"),
        GroupKey::Project(None) => "project:none".to_owned(),
        GroupKey::All => "all".to_owned(),
    }
}

fn group_key(name: &str) -> Option<GroupKey> {
    match name.split_once(':') {
        Some(("state", s)) => s.parse::<State>().ok().map(GroupKey::State),
        Some(("priority", p)) => p.parse().ok().map(GroupKey::Priority),
        Some(("project", "none")) => Some(GroupKey::Project(None)),
        Some(("project", id)) => id.parse().ok().map(|id| GroupKey::Project(Some(id))),
        None if name == "all" => Some(GroupKey::All),
        _ => None,
    }
}

const GROUPINGS: [(Grouping, &str); 4] = [
    (Grouping::State, "state"),
    (Grouping::Priority, "priority"),
    (Grouping::Project, "project"),
    (Grouping::None, "none"),
];

const ORDERINGS: [(Ordering, &str); 3] = [
    (Ordering::Priority, "priority"),
    (Ordering::Updated, "updated"),
    (Ordering::Created, "created"),
];

impl Model {
    /// The model's part of what is remembered; the window's part is left for the view.
    pub fn saved(&self) -> Saved {
        let mut views = BTreeMap::new();
        for view in known_views() {
            let d = self.displays.get(&view).cloned();
            let collapsed: Vec<String> = self
                .collapsed
                .iter()
                .filter(|(v, _)| *v == view)
                .map(|(_, k)| group_name(*k))
                .collect();
            if d.is_none() && collapsed.is_empty() {
                continue;
            }
            let d = d.unwrap_or_else(|| Display::for_view(view));
            let mut collapsed = collapsed;
            collapsed.sort();
            views.insert(
                view_name(view),
                SavedView {
                    filter: d.filter,
                    priorities: d.priorities,
                    states: d.states.iter().map(|s| s.as_str().to_owned()).collect(),
                    grouping: GROUPINGS
                        .iter()
                        .find(|g| g.0 == d.grouping)
                        .map_or("state", |g| g.1)
                        .to_owned(),
                    ordering: ORDERINGS
                        .iter()
                        .find(|o| o.0 == d.ordering)
                        .map_or("priority", |o| o.1)
                        .to_owned(),
                    show_empty: d.show_empty,
                    show_dismissed: d.show_dismissed,
                    collapsed,
                    flat: !d.nest,
                },
            );
        }
        Saved {
            project: self.project().map(|p| p.dir.clone()),
            recent: self.recent.clone(),
            docs_open: self
                .docs_open
                .iter()
                .map(|(dir, path)| (dir.to_string_lossy().into_owned(), path.clone()))
                .collect(),
            docs_shut: self
                .docs_shut
                .iter()
                .filter(|(_, shut)| !shut.is_empty())
                .map(|(dir, shut)| {
                    let mut shut: Vec<String> = shut.iter().cloned().collect();
                    shut.sort();
                    (dir.to_string_lossy().into_owned(), shut)
                })
                .collect(),
            graph_cameras: self
                .graph_cameras()
                .into_iter()
                .map(|(dir, camera)| (dir.to_string_lossy().into_owned(), camera))
                .collect(),
            view: Some(view_name(self.pane().view)),
            layout: Some(
                match self.pane().layout {
                    Layout::List => "list",
                    Layout::Board => "board",
                }
                .to_owned(),
            ),
            show_all: false,
            panes: Some(SavedPanes {
                split: self.split.clone(),
                panes: self
                    .panes
                    .iter()
                    // A view shown alone over the split is not part of it.
                    .filter(|p| self.split.ids().contains(&p.id))
                    .map(|p| SavedPane {
                        id: p.id,
                        view: (!p.placeholder).then(|| view_name(p.view)),
                        board: p.layout == Layout::Board,
                    })
                    .collect(),
                focused: if self.solo() {
                    self.split_focus
                } else {
                    self.pane().id
                },
            }),
            views,
            told_no_notifications: self.told_no_notifications,
            reading_size: Some(self.reading.size.name().to_lowercase()),
            reading_compact: self.reading.compact,
            detail_mode: Some(self.detail_mode.key().to_owned()),
            prefs: self.prefs,
            window: SavedWindow::default(),
        }
    }

    /// Bring back what `saved` remembers. What no longer applies (a project gone, a view or
    /// group renamed) is passed over.
    pub fn restore(&mut self, saved: &Saved) {
        // First: the view restored below may be the graph, entered with its project's camera.
        self.graph_cameras = saved
            .graph_cameras
            .iter()
            .filter(|(_, c)| c.pan_x.is_finite() && c.pan_y.is_finite() && c.zoom > 0.)
            .map(|(dir, camera)| (PathBuf::from(dir), *camera))
            .collect();
        for view in known_views() {
            let Some(s) = saved.views.get(&view_name(view)) else {
                continue;
            };
            let mut d = Display::for_view(view);
            d.filter = s.filter.clone();
            d.priorities = s
                .priorities
                .iter()
                .copied()
                .filter(|p| (1..=4).contains(p))
                .collect();
            // Only states the view's kind has, in display order; any other name is passed over.
            let kind_states = view.kind().map_or(&[][..], |k| k.states());
            d.states = s
                .states
                .iter()
                .filter_map(|name| name.parse::<State>().ok())
                .filter(|st| kind_states.contains(st))
                .collect();
            d.states
                .sort_by_key(|st| super::state_order(view.kind(), *st));
            d.states.dedup();
            // Only what the view offers: one saved before it stopped offering it falls back.
            if let Some(g) = GROUPINGS
                .iter()
                .find(|g| g.1 == s.grouping && view.offers_grouping(g.0))
            {
                d.grouping = g.0;
            }
            if let Some(o) = ORDERINGS
                .iter()
                .find(|o| o.1 == s.ordering && view.offers_ordering(o.0))
            {
                d.ordering = o.0;
            }
            d.show_empty = s.show_empty && view.kind().is_some();
            d.show_dismissed = s.show_dismissed && view.shows_dismissed();
            d.nest = !s.flat;
            if !d.show_dismissed {
                let kind = view.kind();
                d.states
                    .retain(|&st| !kind.map_or(st.is_dismissed(), |k| k.is_dismissed(st)));
            }
            self.displays.insert(view, d);
            self.collapsed.retain(|(v, _)| *v != view);
            for key in s.collapsed.iter().filter_map(|k| group_key(k)) {
                self.collapsed.insert((view, key));
            }
        }
        self.told_no_notifications |= saved.told_no_notifications;
        self.reading = super::Reading {
            size: super::ReadingSize::ALL
                .into_iter()
                .find(|s| Some(s.name().to_lowercase()) == saved.reading_size)
                .unwrap_or_default(),
            compact: saved.reading_compact,
        };
        self.detail_mode = super::DetailMode::ALL
            .into_iter()
            .find(|m| Some(m.key()) == saved.detail_mode.as_deref())
            .unwrap_or_default();
        self.prefs = saved.prefs;
        self.pane_mut().layout = match saved.layout.as_deref() {
            Some("board") => Layout::Board,
            _ => Layout::List,
        };
        if saved.show_all {
            for view in View::ALL.into_iter().filter(|v| v.shows_dismissed()) {
                self.displays
                    .entry(view)
                    .or_insert_with(|| Display::for_view(view))
                    .show_dismissed = true;
            }
        }
        self.recent = saved.recent.clone();
        self.docs_open = saved
            .docs_open
            .iter()
            .map(|(dir, path)| (PathBuf::from(dir), path.clone()))
            .collect();
        self.docs_shut = saved
            .docs_shut
            .iter()
            .map(|(dir, shut)| (PathBuf::from(dir), shut.iter().cloned().collect()))
            .collect();
        if let Some(dir) = &saved.project
            && let Some(i) = self.projects.iter().position(|p| &p.dir == dir)
        {
            self.select_project(i);
        }
        if let Some(panes) = &saved.panes
            && self.restore_panes(panes)
        {
            self.reload();
            return;
        }
        let view = known_views()
            .into_iter()
            .find(|v| Some(view_name(*v)) == saved.view);
        match view {
            Some(v) if v != self.pane().view => self.set_view(v),
            _ => self.reload(),
        }
    }
}

impl Model {
    /// The panes as remembered, when what was remembered holds together: each pane in the split
    /// once. A view remembered in two panes shows in the first; the other waits for a view.
    fn restore_panes(&mut self, saved: &SavedPanes) -> bool {
        let mut ids = saved.split.ids();
        let mut theirs: Vec<u64> = saved.panes.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        theirs.sort_unstable();
        if ids.is_empty()
            || ids != theirs
            || ids.windows(2).any(|w| w[0] == w[1])
            || !well_split(&saved.split)
        {
            return false;
        }
        let mut shown: Vec<View> = Vec::new();
        let panes: Vec<Pane> = saved
            .panes
            .iter()
            .map(|sp| {
                let view = known_views()
                    .into_iter()
                    .find(|v| sp.view.as_deref() == Some(view_name(*v).as_str()))
                    .filter(|v| !shown.contains(v));
                let mut p = Pane::new(sp.id, view.unwrap_or(View::Inbox));
                p.placeholder = view.is_none();
                p.layout = if sp.board {
                    Layout::Board
                } else {
                    Layout::List
                };
                shown.extend(view);
                p
            })
            .collect();
        self.next_pane = ids.last().map_or(0, |m| m + 1);
        self.at = panes
            .iter()
            .position(|p| p.id == saved.focused)
            .unwrap_or(0);
        self.panes = panes;
        self.split = saved.split.clone();
        self.solo = None;
        if let Some(i) = self
            .panes
            .iter()
            .position(|p| !p.placeholder && p.view == View::Activity)
        {
            self.with_pane(i, Model::enter_activity);
        }
        true
    }
}

/// Whether every split has two parts or more, each with a share above nothing.
fn well_split(split: &Split) -> bool {
    match split {
        Split::Pane(_) => true,
        Split::Split { parts, .. } => {
            parts.len() >= 2
                && parts
                    .iter()
                    .all(|(p, f)| f.is_finite() && *f > 0. && well_split(p))
        }
    }
}
