//! What the app remembers between runs: where the human was (project, view), how each view is
//! shown (filter, priorities, grouping, ordering, empty groups, dismissed work, collapsed
//! groups), list or board, and the window's own layout (the sidebar's width and whether it is collapsed, the
//! panel's width). Kept in `app.toml` in livtk's config directory; GPU-free, so tested as plain
//! Rust. The model restores its part; the view restores the window's.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use livtk_core::State;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Display, GroupKey, Grouping, Layout, Model, Ordering, View};

/// Everything remembered, as written to `app.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    /// The open project's directory.
    pub project: Option<PathBuf>,
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
    pub window: SavedWindow,
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
    /// Parents whose sub-tasks are folded away, by id.
    pub folded: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedWindow {
    pub sidebar_width: Option<f32>,
    pub sidebar_collapsed: bool,
    pub panel_width: Option<f32>,
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

fn view_name(view: View) -> String {
    view.title().to_lowercase()
}

fn group_name(key: GroupKey) -> String {
    match key {
        GroupKey::State(s) => format!("state:{}", s.as_str()),
        GroupKey::Priority(p) => format!("priority:{p}"),
        GroupKey::All => "all".to_owned(),
    }
}

fn group_key(name: &str) -> Option<GroupKey> {
    match name.split_once(':') {
        Some(("state", s)) => s.parse::<State>().ok().map(GroupKey::State),
        Some(("priority", p)) => p.parse().ok().map(GroupKey::Priority),
        None if name == "all" => Some(GroupKey::All),
        _ => None,
    }
}

const GROUPINGS: [(Grouping, &str); 3] = [
    (Grouping::State, "state"),
    (Grouping::Priority, "priority"),
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
        for view in View::ALL {
            let d = self.displays.get(&view).cloned();
            let collapsed: Vec<String> = self
                .collapsed
                .iter()
                .filter(|(v, _)| *v == view)
                .map(|(_, k)| group_name(*k))
                .collect();
            if d.is_none() && collapsed.is_empty() && !self.folded.iter().any(|(v, _)| *v == view) {
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
                    folded: self
                        .folded
                        .iter()
                        .filter(|(v, _)| *v == view)
                        .map(|(_, id)| id.to_string())
                        .collect(),
                },
            );
        }
        Saved {
            project: self.project().map(|p| p.dir.clone()),
            view: Some(view_name(self.view)),
            layout: Some(
                match self.layout {
                    Layout::List => "list",
                    Layout::Board => "board",
                }
                .to_owned(),
            ),
            show_all: false,
            views,
            told_no_notifications: self.told_no_notifications,
            window: SavedWindow::default(),
        }
    }

    /// Bring back what `saved` remembers. What no longer applies (a project gone, a view or
    /// group renamed) is passed over.
    pub fn restore(&mut self, saved: &Saved) {
        for view in View::ALL {
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
            if let Some(g) = GROUPINGS.iter().find(|g| g.1 == s.grouping) {
                d.grouping = g.0;
            }
            if let Some(o) = ORDERINGS.iter().find(|o| o.1 == s.ordering) {
                d.ordering = o.0;
            }
            d.show_empty = s.show_empty && view.kind().is_some();
            d.show_dismissed = s.show_dismissed && view.shows_dismissed();
            d.nest = !s.flat;
            self.folded.retain(|(v, _)| *v != view);
            for id in s.folded.iter().filter_map(|id| id.parse::<Uuid>().ok()) {
                self.folded.insert((view, id));
            }
            if !d.show_dismissed {
                d.states.retain(|st| !st.is_dismissed());
            }
            self.displays.insert(view, d);
            self.collapsed.retain(|(v, _)| *v != view);
            for key in s.collapsed.iter().filter_map(|k| group_key(k)) {
                self.collapsed.insert((view, key));
            }
        }
        self.told_no_notifications |= saved.told_no_notifications;
        self.layout = match saved.layout.as_deref() {
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
        if let Some(dir) = &saved.project
            && let Some(i) = self.projects.iter().position(|p| &p.dir == dir)
        {
            self.select_project(i);
        }
        let view = View::ALL
            .into_iter()
            .find(|v| Some(view_name(*v)) == saved.view);
        match view {
            Some(v) if v != self.view => self.set_view(v),
            _ => self.reload(),
        }
    }
}
