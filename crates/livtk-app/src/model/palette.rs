//! The command palette, as Linear's Cmd+K: find any node and run any legal action from one
//! field. What it offers and what choosing does live here; the view only draws it.

use livtk_core::{Hit, Kind, State, Summary, link_allowed};
use uuid::Uuid;

use super::{Choice, Grouping, Layout, LinkChoice, Model, Ordering, View, link_choices};

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
    /// Picking the other end of a link instead of running commands.
    pub link: Option<LinkPick>,
    /// Picking a doc to link a node to, or a node to link a doc to.
    pub doc_pick: Option<DocPick>,
}

/// A link between a node and a doc waiting for its other end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocPick {
    /// A doc for the node keyed so.
    ForNode(String),
    /// A node for the doc at this path.
    ForDoc(String),
}

/// A doc the palette found: its path and title, and the line its text matched in, when it
/// was found by its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocItem {
    pub path: String,
    pub title: String,
    pub line: Option<String>,
}

/// A link waiting for its other end: from (or to) `node`, by `choice`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkPick {
    pub node: Uuid,
    pub key: String,
    pub kind: Kind,
    pub choice: LinkChoice,
}

impl LinkPick {
    /// Whether `other` can be this link's other end, by the core's rule.
    fn accepts(&self, other: &Found) -> bool {
        let rel = self.choice.rel;
        other.id != self.node
            && if self.choice.outgoing {
                link_allowed(self.kind, rel, other.kind)
            } else {
                link_allowed(other.kind, rel, self.kind)
            }
    }
}

/// One line of the palette.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Action(Action),
    Node(Found),
    Doc(DocItem),
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
    ToggleEmpty,
    FitGraph,
    /// Pick the other end of a link from the selected node.
    PickLink(LinkChoice),
    /// Done by the view: it owns the forms, inputs and dialogs.
    Ask(Ask),
}

/// Actions the view carries out after the palette closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// The palette reopened to pick a link's other end: its field starts empty.
    PickLink,
    NewTask,
    AddNote,
    EditTitle,
    EditBody,
    OpenFolder,
    Help,
    SplitRight,
    SplitDown,
    ClosePane,
    NextPane,
    NewTerminal,
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
            self.warn(e.to_string());
        }
        self.palette = Some(Palette::default());
        self.palette_query("");
    }

    /// Open the palette to pick the other end of a link from the node actions act on.
    pub fn open_link_picker(&mut self, choice: LinkChoice) {
        let Some(row) = self.acted_on() else { return };
        if let Some(p) = self.projects.get_mut(self.current)
            && let Err(e) = p.ws.prepare_search()
        {
            self.warn(e.to_string());
        }
        self.palette = Some(Palette {
            link: Some(LinkPick {
                node: row.id,
                key: row.key,
                kind: row.kind,
                choice,
            }),
            ..Palette::default()
        });
        self.palette_query("");
    }

    /// Pick a doc for the node acted on to link to.
    pub fn open_doc_picker(&mut self) {
        let Some(row) = self.acted_on() else { return };
        self.palette = Some(Palette {
            doc_pick: Some(DocPick::ForNode(row.key)),
            ..Palette::default()
        });
        self.palette_query("");
    }

    /// Pick a node to link the open doc to.
    pub fn open_doc_node_picker(&mut self) {
        let Some(path) = self.docs.open.clone() else {
            return;
        };
        if let Some(p) = self.projects.get_mut(self.current)
            && let Err(e) = p.ws.prepare_search()
        {
            self.warn(e.to_string());
        }
        self.palette = Some(Palette {
            doc_pick: Some(DocPick::ForDoc(path)),
            ..Palette::default()
        });
        self.palette_query("");
    }

    /// The project's docs `query` finds, by title or path: every doc for an empty query.
    fn find_docs(&self, query: &str) -> Vec<DocItem> {
        let Some(p) = self.projects.get(self.current) else {
            return Vec::new();
        };
        let tree = p.ws.docs();
        let mut scored: Vec<(i32, DocItem)> = tree
            .all()
            .into_iter()
            .filter_map(|d| {
                let score = if query.trim().is_empty() {
                    Some(0)
                } else {
                    fuzzy(query, &d.title).or_else(|| fuzzy(query, &d.path))
                };
                score.map(|s| {
                    (
                        s,
                        DocItem {
                            path: d.path.clone(),
                            title: d.title.clone(),
                            line: None,
                        },
                    )
                })
            })
            .collect();
        scored.sort_by_key(|(s, _)| -s);
        let mut out: Vec<DocItem> = scored.into_iter().map(|(_, d)| d).collect();
        // Then those whose text holds it, with the line it is in.
        if !query.trim().is_empty() {
            for f in livtk_core::docs::search(p.ws.root(), &tree.all(), query) {
                if !out.iter().any(|d| d.path == f.path) {
                    out.push(DocItem {
                        path: f.path,
                        title: f.title,
                        line: f.lines.into_iter().next(),
                    });
                }
            }
        }
        out
    }

    pub fn close_palette(&mut self) {
        self.palette = None;
    }

    /// Show what `query` finds: matching actions, then matching nodes. An empty query shows the
    /// selected node's actions, then the nodes updated last.
    pub fn palette_query(&mut self, query: &str) {
        let pick = self.palette.as_ref().and_then(|p| p.link.clone());
        let doc_pick = self.palette.as_ref().and_then(|p| p.doc_pick.clone());
        let items = if let Some(doc_pick) = doc_pick {
            let ws = self.projects.get(self.current).map(|p| &p.ws);
            match doc_pick {
                // A doc for a node: those it is not linked to yet.
                DocPick::ForNode(key) => {
                    let linked: Vec<String> = ws
                        .and_then(|w| w.docs_of(&key).ok())
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(p, _)| p)
                        .collect();
                    self.find_docs(query)
                        .into_iter()
                        .filter(|d| !linked.contains(&d.path))
                        .map(Item::Doc)
                        .collect()
                }
                // A node for a doc: those not linked to it yet.
                DocPick::ForDoc(path) => {
                    let linked: Vec<Uuid> = ws
                        .and_then(|w| w.doc_nodes(&path).ok())
                        .unwrap_or_default()
                        .into_iter()
                        .map(|n| n.id)
                        .collect();
                    let found = if query.trim().is_empty() {
                        self.recent()
                    } else {
                        self.find(query)
                    };
                    found
                        .into_iter()
                        .filter(|f| !linked.contains(&f.id))
                        .map(Item::Node)
                        .collect()
                }
            }
        } else if let Some(pick) = pick {
            // Picking a link's other end: only nodes it can join.
            let found = if query.trim().is_empty() {
                self.recent()
            } else {
                self.find(query)
            };
            found
                .into_iter()
                .filter(|f| pick.accepts(f))
                .map(Item::Node)
                .collect()
        } else if query.trim().is_empty() {
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
            items.extend(
                self.find_docs(query)
                    .into_iter()
                    .take(ACTIONS)
                    .map(Item::Doc),
            );
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
        let index = self.palette.as_ref()?.selected;
        self.palette_run_at(index)
    }

    /// Run item `index` (a click) and close the palette.
    pub fn palette_run_at(&mut self, index: usize) -> Option<Ask> {
        let p = self.palette.take()?;
        let item = p.items.into_iter().nth(index)?;
        match (p.link, p.doc_pick, item) {
            (Some(pick), _, Item::Node(other)) => {
                self.link(pick.node, pick.choice, other.id);
                None
            }
            (_, Some(DocPick::ForNode(key)), Item::Doc(d)) => {
                self.link_doc(&key, &d.path, true);
                None
            }
            (_, Some(DocPick::ForDoc(path)), Item::Node(n)) => {
                self.link_doc(&n.key, &path, true);
                None
            }
            (_, _, item) => self.run_item(item),
        }
    }

    fn run_item(&mut self, item: Item) -> Option<Ask> {
        match item {
            Item::Node(found) => {
                self.reveal(&found);
                None
            }
            Item::Doc(d) => {
                self.open_doc(&d.path);
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
                    self.toggle_dismissed();
                    None
                }
                Run::ToggleEmpty => {
                    self.toggle_empty();
                    None
                }
                Run::FitGraph => {
                    self.graph_fit();
                    None
                }
                Run::PickLink(choice) => {
                    self.open_link_picker(choice);
                    Some(Ask::PickLink)
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
            k => View::Of(k),
        };
        // The inbox shows it already, if it waits for the human.
        let in_view = self.pane().all.iter().any(|r| r.id == found.id);
        if !(self.pane().view == View::Inbox && in_view) {
            self.set_view(view);
        }
        if found.kind.is_dismissed(found.state) && !self.display().show_dismissed {
            self.toggle_dismissed();
        }
        if !self.pane().rows.iter().any(|r| r.id == found.id) {
            let view = self.pane().view;
            let d = self
                .displays
                .entry(view)
                .or_insert_with(|| super::Display::for_view(view));
            d.filter.clear();
            d.priorities.clear();
            d.states.clear();
            self.collapsed.retain(|(v, _)| *v != view);
            self.arrange();
        }
        self.pane_mut().selected = Some(found.id);
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
                self.warn(e.to_string());
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
                .filter(|&s| !kind.is_dismissed(s))
                .collect();
            all.extend(p.ws.list(kind, &states).unwrap_or_default());
        }
        all.sort_by(|a, b| b.updated.cmp(&a.updated));
        all.iter().take(RECENT).map(Found::from).collect()
    }

    /// What can be done to the selected node: its legal moves, then priority and editing.
    fn node_actions(&self) -> Vec<Action> {
        let Some(row) = self.acted_on() else {
            return Vec::new();
        };
        let key = Some(row.key.clone());
        let mut out = Vec::new();
        let mut decided = false;
        for c in self.choices(row.kind, row.state) {
            let primary = c.decides && !matches!(c.to, State::Rejected | State::Retired);
            let shortcut = if c.always {
                Some(super::keys::STANDING)
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
        for choice in link_choices(row.kind, &self.declared_rels()) {
            out.push(Action {
                label: format!("Add link: {}", choice.label.to_lowercase()),
                context: key.clone(),
                shortcut: None,
                run: Run::PickLink(choice),
            });
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
            (View::Activity, "G A"),
            (View::Tasks, "G T"),
            (View::Lessons, "G L"),
            (View::Decisions, "G D"),
            (View::Findings, "G F"),
            (View::Docs, "G O"),
            (View::Graph, "G R"),
            (View::Terminal, "G X"),
        ] {
            if v != self.pane().view {
                out.push(plain(
                    &format!("Go to {}", v.title()),
                    Some(keys),
                    Run::Go(v),
                ));
            }
        }
        // The kinds the project declares, as views of their own.
        for v in self
            .views()
            .into_iter()
            .filter(|v| matches!(v, View::Of(_)))
        {
            if v != self.pane().view {
                out.push(plain(&format!("Go to {}", v.title()), None, Run::Go(v)));
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
            "Open another project…",
            Some("O"),
            Run::Ask(Ask::OpenFolder),
        ));
        if self.pane().view.has_board() {
            let label = match self.pane().layout {
                Layout::List => "Switch to board",
                Layout::Board => "Switch to list",
            };
            out.push(plain(label, Some("B"), Run::ToggleLayout));
        }
        let d = self.display();
        let board = self.pane().view.has_board() && self.pane().layout == Layout::Board;
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
        if self.pane().view == View::Graph {
            out.push(plain("Zoom to fit", Some("0"), Run::FitGraph));
        }
        if self.pane().view.kind().is_some() {
            let label = if d.show_empty {
                "Hide empty groups"
            } else {
                "Show empty groups"
            };
            out.push(plain(
                label,
                Some(super::keys::EMPTY_GROUPS),
                Run::ToggleEmpty,
            ));
        }
        if self.pane().view.shows_dismissed() {
            let label = if d.show_dismissed {
                "Hide dismissed work"
            } else {
                "Show dismissed work"
            };
            out.push(plain(
                label,
                Some(super::keys::DISMISSED),
                Run::ToggleDismissed,
            ));
        }
        out.push(plain(
            "Split pane right",
            Some(super::keys::SPLIT_RIGHT),
            Run::Ask(Ask::SplitRight),
        ));
        out.push(plain(
            "Split pane down",
            Some(super::keys::SPLIT_DOWN),
            Run::Ask(Ask::SplitDown),
        ));
        if self.panes.len() > 1 {
            out.push(plain(
                "Close pane",
                Some(super::keys::CLOSE_PANE),
                Run::Ask(Ask::ClosePane),
            ));
            out.push(plain(
                "Next pane",
                Some(super::keys::NEXT_PANE),
                Run::Ask(Ask::NextPane),
            ));
        }
        out.push(plain(
            "New terminal",
            Some(super::keys::NEW_TERMINAL),
            Run::Ask(Ask::NewTerminal),
        ));
        out.push(plain(
            "Keyboard shortcuts",
            Some(super::keys::HELP),
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
