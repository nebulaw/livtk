//! The command palette, as Linear's Cmd+K: find any node and run any legal action from one
//! field. What it offers and what choosing does live here; the view only draws it.

use livtk_core::{Hit, Kind, State, Summary};
use uuid::Uuid;

use super::{Choice, Grouping, Layout, Model, Ordering, View, choices};

/// Nodes shown for a typed query.
const FOUND: usize = 20;
/// Nodes shown for an empty query: the ones updated last.
const RECENT: usize = 8;
/// Actions shown for a typed query.
const ACTIONS: usize = 8;

/// The palette while it is open.
#[derive(Clone, Debug, Default)]
pub struct Palette {
    pub query: String,
    pub items: Vec<Item>,
    /// The highlighted item, which `Enter` runs.
    pub selected: usize,
}

/// One line of the palette.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Action(Action),
    Node(Found),
}

/// Something to do, with the shortcut that does it outside the palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub label: String,
    /// What it acts on, shown dimmed: the selected node's key.
    pub context: Option<String>,
    pub shortcut: Option<&'static str>,
    pub run: Run,
}

/// What an action does.
#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Choose(Choice),
    Priority(u8),
    Go(View),
    Project(usize),
    ToggleLayout,
    Group(Grouping),
    Order(Ordering),
    ToggleDismissed,
    /// Done by the view: it owns the forms, inputs and dialogs.
    Ask(Ask),
}

/// Actions the view carries out after the palette closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    NewTask,
    AddNote,
    EditTitle,
    EditBody,
    OpenFolder,
    Help,
}

/// A node the palette found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub id: Uuid,
    pub key: String,
    pub kind: Kind,
    pub state: State,
    pub title: String,
}

impl From<Hit> for Found {
    fn from(h: Hit) -> Found {
        Found {
            id: h.id,
            key: h.key,
            kind: h.kind,
            state: h.state,
            title: h.title,
        }
    }
}

impl From<&Summary> for Found {
    fn from(s: &Summary) -> Found {
        Found {
            id: s.id,
            key: s.key.clone(),
            kind: s.kind,
            state: s.state,
            title: s.title.clone(),
        }
    }
}

const PRIORITY_NAMES: [&str; 4] = ["Urgent", "High", "Medium", "Low"];
const PRIORITY_KEYS: [&str; 4] = ["1", "2", "3", "4"];

impl Model {
    /// Open the palette, empty, and get search ready so typing never waits for the model.
    pub fn open_palette(&mut self) {
        if let Some(p) = self.projects.get_mut(self.current)
            && let Err(e) = p.ws.prepare_search()
        {
            self.notice = Some(e.to_string());
        }
        self.palette = Some(Palette::default());
        self.palette_query("");
    }

    pub fn close_palette(&mut self) {
        self.palette = None;
    }

    /// Show what `query` finds: matching actions, then matching nodes. An empty query shows the
    /// selected node's actions, then the nodes updated last.
    pub fn palette_query(&mut self, query: &str) {
        let items = if query.trim().is_empty() {
            let mut items: Vec<Item> = self.node_actions().into_iter().map(Item::Action).collect();
            items.extend(self.recent().into_iter().map(Item::Node));
            items
        } else {
            let mut scored: Vec<(i32, Action)> = self
                .actions()
                .into_iter()
                .filter_map(|a| fuzzy(query, &a.label).map(|s| (s, a)))
                .collect();
            // Best first; ties keep the catalogue's order.
            scored.sort_by_key(|(s, _)| -s);
            let mut items: Vec<Item> = scored
                .into_iter()
                .take(ACTIONS)
                .map(|(_, a)| Item::Action(a))
                .collect();
            items.extend(self.find(query).into_iter().map(Item::Node));
            items
        };
        if let Some(p) = &mut self.palette {
            p.query = query.to_owned();
            p.items = items;
            p.selected = 0;
        }
    }

    /// Move the highlight by `delta`, wrapping round the ends as Linear's does.
    pub fn palette_move(&mut self, delta: isize) {
        if let Some(p) = &mut self.palette
            && !p.items.is_empty()
        {
            let n = p.items.len() as isize;
            p.selected = (p.selected as isize + delta).rem_euclid(n) as usize;
        }
    }

    /// Highlight item `index`, as the pointer does.
    pub fn palette_highlight(&mut self, index: usize) {
        if let Some(p) = &mut self.palette
            && index < p.items.len()
        {
            p.selected = index;
        }
    }

    /// Run the highlighted item and close the palette. What only the view can do comes back.
    pub fn palette_run(&mut self) -> Option<Ask> {
        let p = self.palette.take()?;
        let item = p.items.into_iter().nth(p.selected)?;
        self.run_item(item)
    }

    /// Run item `index` (a click) and close the palette.
    pub fn palette_run_at(&mut self, index: usize) -> Option<Ask> {
        let item = self.palette.take()?.items.into_iter().nth(index)?;
        self.run_item(item)
    }

    fn run_item(&mut self, item: Item) -> Option<Ask> {
        match item {
            Item::Node(found) => {
                self.reveal(&found);
                None
            }
            Item::Action(a) => match a.run {
                Run::Choose(c) => {
                    self.choose(&c);
                    None
                }
                Run::Priority(p) => {
                    self.set_priority(p);
                    None
                }
                Run::Go(v) => {
                    self.set_view(v);
                    None
                }
                Run::Project(i) => {
                    self.select_project(i);
                    None
                }
                Run::ToggleLayout => {
                    self.toggle_layout();
                    None
                }
                Run::Group(g) => {
                    self.set_grouping(g);
                    None
                }
                Run::Order(o) => {
                    self.set_ordering(o);
                    None
                }
                Run::ToggleDismissed => {
                    self.toggle_show_all();
                    None
                }
                Run::Ask(ask) => Some(ask),
            },
        }
    }

    /// Go to a node wherever it is: its view, past any filter, collapsed group or hidden
    /// dismissed state, selected and open in the detail panel.
    fn reveal(&mut self, found: &Found) {
        let view = match found.kind {
            Kind::Task => View::Tasks,
            Kind::Lesson => View::Lessons,
            Kind::Decision => View::Decisions,
            Kind::Finding => View::Findings,
        };
        // The inbox shows it already, if it waits for the human.
        let in_view = self.all.iter().any(|r| r.id == found.id);
        if !(self.view == View::Inbox && in_view) {
            self.set_view(view);
        }
        if found.state.is_dismissed() && !self.show_all {
            self.toggle_show_all();
        }
        if !self.rows.iter().any(|r| r.id == found.id) {
            let view = self.view;
            let d = self
                .displays
                .entry(view)
                .or_insert_with(|| super::Display::for_view(view));
            d.filter.clear();
            d.priorities.clear();
            self.collapsed.retain(|(v, _)| *v != view);
            self.arrange();
        }
        self.selected = Some(found.id);
        self.open_detail();
    }

    /// Nodes for a typed query: a key first, then by keywords and meaning.
    fn find(&mut self, query: &str) -> Vec<Found> {
        let Some(p) = self.projects.get_mut(self.current) else {
            return Vec::new();
        };
        match p.ws.find(query, FOUND) {
            Ok(hits) => hits.into_iter().map(Found::from).collect(),
            Err(e) => {
                self.notice = Some(e.to_string());
                Vec::new()
            }
        }
    }

    /// The nodes updated last, dismissed ones left out.
    fn recent(&self) -> Vec<Found> {
        let Some(p) = self.projects.get(self.current) else {
            return Vec::new();
        };
        let mut all: Vec<Summary> = Vec::new();
        for kind in [Kind::Task, Kind::Lesson, Kind::Decision, Kind::Finding] {
            let states: Vec<State> = kind
                .states()
                .iter()
                .copied()
                .filter(|s| !s.is_dismissed())
                .collect();
            all.extend(p.ws.list(kind, &states).unwrap_or_default());
        }
        all.sort_by(|a, b| b.updated.cmp(&a.updated));
        all.iter().take(RECENT).map(Found::from).collect()
    }

    /// What can be done to the selected node: its legal moves, then priority and editing.
    fn node_actions(&self) -> Vec<Action> {
        let Some(row) = self.selected() else {
            return Vec::new();
        };
        let key = Some(row.key.clone());
        let mut out = Vec::new();
        let mut decided = false;
        for c in choices(row.kind, row.state) {
            let primary = c.decides && !matches!(c.to, State::Rejected | State::Retired);
            let shortcut = if c.always {
                Some("S")
            } else if c.to == State::Rejected {
                Some("X")
            } else if primary && !decided {
                decided = true;
                Some("A")
            } else {
                None
            };
            out.push(Action {
                label: c.label.to_owned(),
                context: key.clone(),
                shortcut,
                run: Run::Choose(c),
            });
        }
        if row.kind == Kind::Task {
            for p in 1..=4u8 {
                if p != row.priority {
                    out.push(Action {
                        label: format!("Set priority: {}", PRIORITY_NAMES[usize::from(p - 1)]),
                        context: key.clone(),
                        shortcut: Some(PRIORITY_KEYS[usize::from(p - 1)]),
                        run: Run::Priority(p),
                    });
                }
            }
        }
        for (label, shortcut, ask) in [
            ("Add a note", Some("C"), Ask::AddNote),
            ("Edit title", None, Ask::EditTitle),
            ("Edit description", None, Ask::EditBody),
        ] {
            out.push(Action {
                label: label.to_owned(),
                context: key.clone(),
                shortcut,
                run: Run::Ask(ask),
            });
        }
        out
    }

    /// Every action from where the human stands: on the selected node, then creating,
    /// navigating and changing the view.
    fn actions(&self) -> Vec<Action> {
        let mut out = self.node_actions();
        let plain = |label: &str, shortcut: Option<&'static str>, run: Run| Action {
            label: label.to_owned(),
            context: None,
            shortcut,
            run,
        };
        out.push(plain("New task", Some("N"), Run::Ask(Ask::NewTask)));
        for (v, keys) in [
            (View::Inbox, "G I"),
            (View::Tasks, "G T"),
            (View::Lessons, "G L"),
            (View::Decisions, "G D"),
            (View::Findings, "G F"),
        ] {
            if v != self.view {
                out.push(plain(
                    &format!("Go to {}", v.title()),
                    Some(keys),
                    Run::Go(v),
                ));
            }
        }
        for (i, p) in self.projects.iter().enumerate() {
            if i != self.current {
                out.push(plain(
                    &format!("Switch to project {}", p.name),
                    None,
                    Run::Project(i),
                ));
            }
        }
        out.push(plain(
            "Open a project folder…",
            Some("O"),
            Run::Ask(Ask::OpenFolder),
        ));
        if self.view == View::Tasks {
            let label = match self.layout {
                Layout::List => "Switch to board",
                Layout::Board => "Switch to list",
            };
            out.push(plain(label, Some("B"), Run::ToggleLayout));
        }
        let d = self.display();
        let board = self.view == View::Tasks && self.layout == Layout::Board;
        for (label, g) in [
            ("Group by state", Grouping::State),
            ("Group by priority", Grouping::Priority),
            ("No grouping", Grouping::None),
        ] {
            if d.grouping != g && !(board && g == Grouping::None) {
                out.push(plain(label, None, Run::Group(g)));
            }
        }
        for (label, o) in [
            ("Order by priority", Ordering::Priority),
            ("Order by last updated", Ordering::Updated),
            ("Order by creation", Ordering::Created),
        ] {
            if d.ordering != o {
                out.push(plain(label, None, Run::Order(o)));
            }
        }
        if self.view != View::Inbox {
            let label = if self.show_all {
                "Hide dismissed work"
            } else {
                "Show dismissed work"
            };
            out.push(plain(label, Some("H"), Run::ToggleDismissed));
        }
        out.push(plain(
            "Keyboard shortcuts",
            Some("Ctrl ?"),
            Run::Ask(Ask::Help),
        ));
        out
    }
}

/// How well `query` matches `label` as a subsequence of its letters, ignoring case and spaces:
/// `None` when it does not, higher when it does better. Letters that start a word or follow the
/// previous match score more; gaps cost a little. Greedy, which is enough for short labels.
pub fn fuzzy(query: &str, label: &str) -> Option<i32> {
    let label: Vec<char> = label.to_lowercase().chars().collect();
    let mut score = 0;
    let mut at = 0;
    let mut last: Option<usize> = None;
    for q in query.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let found = (at..label.len()).find(|&i| label[i] == q)?;
        let word_start = found == 0 || !label[found - 1].is_alphanumeric();
        score += 1;
        if word_start {
            score += 8;
        }
        match last {
            Some(l) if l + 1 == found => score += 5,
            Some(l) => score -= ((found - l - 1) as i32).min(3),
            None => score -= (found as i32).min(3),
        }
        last = Some(found);
        at = found + 1;
    }
    Some(score)
}
