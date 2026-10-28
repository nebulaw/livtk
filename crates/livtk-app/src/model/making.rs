//! Making a node from the app: what kinds the new-node form makes, how each is named and offered,
//! and the one way a filled form becomes a node. A kind the form makes is a row in `Making`; the
//! properties it sets are rows of `Prop`, its `Draft` holds them; its rules stay in the core.

use jiff::civil::Date;
use livtk_core::{Actor, Command, Kind, NewProject, Rel};

use super::{Found, LinkChoice, Model, View};

/// A kind of node the app's form makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Making {
    Task,
    Project,
}

impl Making {
    /// Every kind the form makes, in the order they are offered.
    pub const ALL: [Making; 2] = [Making::Task, Making::Project];

    /// Everything about making it, in one row.
    pub const fn spec(self) -> &'static Spec {
        match self {
            Making::Task => &TASK,
            Making::Project => &PROJECT,
        }
    }
}

/// How a kind the form makes is named, offered and found: one row each.
#[derive(Debug)]
pub struct Spec {
    pub kind: Kind,
    /// What the form and the palette call making one.
    pub title: &'static str,
    /// The key that opens the form for it, from anywhere.
    pub key: &'static str,
    /// The form's button.
    pub create: &'static str,
    /// What its title and its body ask for.
    pub title_hint: &'static str,
    pub body_hint: &'static str,
    /// The properties its chips set, in their order.
    pub props: &'static [Prop],
    /// Its selectors: the form's card and button, and an empty list's offer to make one.
    pub card_id: &'static str,
    pub create_id: &'static str,
    pub empty_id: &'static str,
}

const TASK: Spec = Spec {
    kind: Kind::Task,
    title: "New task",
    key: "N",
    create: "Create task",
    title_hint: "Task title",
    body_hint: "Add a plan: the goal, the approach, how you'll know it's done…",
    props: &[Prop::State, Prop::Priority, Prop::Project, Prop::Agent],
    card_id: "new-task-card",
    create_id: "create-task",
    empty_id: "empty-new-task",
};

const PROJECT: Spec = Spec {
    kind: Kind::Project,
    title: "New project",
    key: "P",
    create: "Create project",
    title_hint: "The outcome, in a line",
    body_hint: "Why it matters, and how it gets there…",
    props: &[Prop::Status, Prop::Owner, Prop::Target, Prop::PartOf],
    card_id: "new-project-card",
    create_id: "create-project",
    empty_id: "empty-new-project",
};

/// What the form's menus offer that comes from the store: worked out once per change of the
/// log (DEC-7).
#[derive(Clone, Debug, Default)]
pub struct Choices {
    projects: Vec<(String, String)>,
    owners: Vec<String>,
}

/// A property the form sets with a chip: what a kind offers, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prop {
    /// A task's state to start in: Backlog, or Planned when its plan is already written.
    State,
    Priority,
    /// A task's project.
    Project,
    /// The agent a task is given to.
    Agent,
    /// A project's status: Active (committed to) or Proposed.
    Status,
    Owner,
    Target,
    /// The project a project is part of.
    PartOf,
}

impl Prop {
    /// What its chip says while it is unset.
    pub const fn name(self) -> &'static str {
        match self {
            Prop::State => "State",
            Prop::Priority => "Priority",
            Prop::Project => "Project",
            Prop::Agent => "Agent",
            Prop::Status => "Status",
            Prop::Owner => "Owner",
            Prop::Target => "Target date",
            Prop::PartOf => "Part of",
        }
    }

    /// Its selector, as tests find it: `prop-<name>`.
    pub const fn id(self) -> &'static str {
        match self {
            Prop::State => "prop-state",
            Prop::Priority => "prop-priority",
            Prop::Project => "prop-project",
            Prop::Agent => "prop-agent",
            Prop::Status => "prop-status",
            Prop::Owner => "prop-owner",
            Prop::Target => "prop-target",
            Prop::PartOf => "prop-part-of",
        }
    }
}

/// A link the form makes once its node is made: by which relation and direction, to which node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub choice: LinkChoice,
    pub to: Found,
}

impl Pending {
    /// The one parent a node has: its project, or the task it is part of.
    fn is_parent(&self) -> bool {
        self.choice.rel == Rel::Parent && self.choice.outgoing
    }
}

/// The form being filled: every property its chips set, and its links. Its title and body are
/// the view's fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub making: Making,
    /// A task starts planned (its plan already written) rather than in the backlog.
    pub planned: bool,
    pub priority: u8,
    /// A task's project, or the project a project is part of: by key and title. One parent, so
    /// a "Part of…" link takes its place, and it takes theirs.
    pub parent: Option<(String, String)>,
    pub agent: Option<String>,
    /// A project committed to at once (active), as the human may; else proposed.
    pub active: bool,
    pub owner: Option<String>,
    pub target: Option<Date>,
    pub links: Vec<Pending>,
    /// Once made, the form stays open, emptied, its chips kept, for the next one.
    pub more: bool,
}

impl Draft {
    pub fn new(making: Making) -> Draft {
        Draft {
            making,
            planned: false,
            priority: 3,
            parent: None,
            agent: None,
            active: true,
            owner: None,
            target: None,
            links: Vec::new(),
            more: false,
        }
    }

    /// Set the parent chip: a "Part of…" link goes, as a node has one parent.
    pub fn set_parent(&mut self, parent: Option<(String, String)>) {
        if parent.is_some() {
            self.links.retain(|l| !l.is_parent());
        }
        self.parent = parent;
    }

    /// Add a link, once: a "Part of…" one takes the parent chip's place.
    pub fn add_link(&mut self, link: Pending) {
        if self
            .links
            .iter()
            .any(|l| l.choice == link.choice && l.to.id == link.to.id)
        {
            return;
        }
        if link.is_parent() {
            self.links.retain(|l| !l.is_parent());
            self.parent = None;
        }
        self.links.push(link);
    }

    pub fn remove_link(&mut self, i: usize) {
        if i < self.links.len() {
            self.links.remove(i);
        }
    }

    /// Ready for the next one: the chips kept, the links (each about the last one) gone.
    fn again(&mut self) {
        self.links.clear();
    }
}

impl Model {
    /// The projects a new one may be part of: every one not done or dropped, by key and title.
    pub fn project_choices(&self) -> Vec<(String, String)> {
        self.form_choices().projects
    }

    /// The owners the project's projects have, each once, in order: the form offers them.
    pub fn owner_choices(&self) -> Vec<String> {
        self.form_choices().owners
    }

    /// The form's choices, worked out once per change of the log.
    fn form_choices(&self) -> Choices {
        if let Some(c) = self.choices.borrow().as_ref() {
            return c.clone();
        }
        let c = self.work_out_choices();
        *self.choices.borrow_mut() = Some(c.clone());
        c
    }

    fn work_out_choices(&self) -> Choices {
        let Some(p) = self.project() else {
            return Choices::default();
        };
        let all = p.ws.projects().unwrap_or_default();
        let projects = all
            .iter()
            .filter(|s| {
                !matches!(
                    s.state,
                    livtk_core::State::Done | livtk_core::State::Dropped
                )
            })
            .map(|s| (s.key.clone(), s.title.clone()))
            .collect();
        let mut owners: Vec<String> = Vec::new();
        for s in &all {
            let owner =
                p.ws.node(&s.key)
                    .ok()
                    .and_then(|n| {
                        n.props
                            .get("owner")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned)
                    })
                    .filter(|o| !o.trim().is_empty());
            if let Some(o) = owner.filter(|o| !owners.contains(o)) {
                owners.push(o);
            }
        }
        owners.sort();
        Choices { projects, owners }
    }

    /// Start the form making a node of kind `making`; one already open stays as it is.
    pub fn open_draft(&mut self, making: Making) {
        if self.draft.is_none() {
            self.draft = Some(Draft::new(making));
        }
    }

    pub fn close_draft(&mut self) {
        self.draft = None;
    }

    /// Make the draft, titled `title` with `body`, as the human, in the open project: its
    /// properties and links with it. Then show it, selected, in its own view; or, with Create
    /// more, keep the form open for the next. Whether it was made; why not is the notice.
    pub fn create(&mut self, title: &str, body: &str) -> bool {
        let Some(draft) = self.draft.clone() else {
            return false;
        };
        let Some(project) = self.projects.get_mut(self.current) else {
            return false;
        };
        let before = project.ws.newest_event().ok().flatten();
        let kind = draft.making.spec().kind;
        let made = match draft.making {
            Making::Task => {
                // Links from it are made with it; a project is its parent.
                let mut links: Vec<(Rel, String)> = draft
                    .links
                    .iter()
                    .filter(|l| l.choice.outgoing)
                    .map(|l| (l.choice.rel, l.to.key.clone()))
                    .collect();
                links.extend(draft.parent.iter().map(|(k, _)| (Rel::Parent, k.clone())));
                project.ws.execute(
                    Command::Create {
                        kind: Kind::Task,
                        title: title.to_owned(),
                        body: body.to_owned(),
                        priority: Some(draft.priority),
                        links,
                    },
                    &Actor::Human,
                )
            }
            Making::Project => project.ws.new_project(
                &NewProject {
                    title: title.to_owned(),
                    body: body.to_owned(),
                    done_when: String::new(),
                    target: draft.target.map(|d| d.to_string()),
                    owner: draft.owner.clone(),
                    part_of: draft.parent.as_ref().map(|(k, _)| k.clone()),
                    commit: draft.active,
                },
                &Actor::Human,
            ),
        };
        let events = match made {
            Ok(events) => events,
            Err(e) => {
                self.warn(e.to_string());
                return false;
            }
        };
        let Some(id) = events.first().map(|e| e.op.subjects().0) else {
            return false;
        };
        // What can only follow its making: where it starts, its agent, links to it (and a
        // project's links, which its making does not take).
        let node = id.to_string();
        let mut then: Vec<Command> = Vec::new();
        if draft.making == Making::Task && draft.planned {
            then.push(Command::Move {
                node: node.clone(),
                to: livtk_core::State::Planned,
            });
        }
        if let Some(agent) = draft.agent.clone().filter(|_| draft.making == Making::Task) {
            then.push(Command::SetAgent {
                node: node.clone(),
                agent,
            });
        }
        for l in &draft.links {
            let after = !l.choice.outgoing || draft.making == Making::Project;
            if after {
                let (src, dst) = if l.choice.outgoing {
                    (node.clone(), l.to.id.to_string())
                } else {
                    (l.to.id.to_string(), node.clone())
                };
                then.push(Command::Link {
                    src,
                    rel: l.choice.rel,
                    dst,
                });
            }
        }
        let mut failed = None;
        for cmd in then {
            if let Err(e) = project.ws.execute(cmd, &Actor::Human) {
                failed.get_or_insert(e.to_string());
            }
        }
        if !project.ws.config().on.is_empty() {
            self.wrote.push((project.dir.clone(), before));
        }
        self.toasts.clear_errors();
        // Made, though a part of it was refused: said, and the node shown as it is.
        if let Some(e) = failed {
            self.warn(format!("Made, but not all of it: {e}"));
        }
        // Into its own view, wherever that shows: its pane, or this one.
        self.set_view(View::listing(kind));
        self.reload();
        self.select(id);
        if draft.more {
            if let Some(d) = self.draft.as_mut() {
                d.again();
            }
        } else {
            self.draft = None;
            self.open_detail();
        }
        true
    }
}

/// What a choice in a chip's menu sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    Planned(bool),
    Priority(u8),
    Parent(Option<(String, String)>),
    Agent(Option<String>),
    Active(bool),
    Owner(Option<String>),
    Target(Option<Date>),
    /// A relation to add a link by: the node is picked next.
    Link(LinkChoice),
}

/// One choice of a chip's menu: what it says, a few quiet words after it, whether it is the
/// one chosen, and what it sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    pub sub: Option<String>,
    pub on: bool,
    pub pick: Pick,
}

/// The priorities by name, from 1.
pub const PRIORITIES: [&str; 4] = ["Urgent", "High", "Medium", "Low"];

impl Model {
    /// The choices of `prop`'s menu that `query` finds (all for an empty one), the chosen one
    /// ticked. An owner can also be one typed in.
    pub fn prop_choices(&self, prop: Prop, query: &str) -> Vec<Choice> {
        let Some(d) = self.draft.as_ref() else {
            return Vec::new();
        };
        let c = |label: &str, sub: Option<&str>, on: bool, pick: Pick| Choice {
            label: label.to_owned(),
            sub: sub.map(str::to_owned),
            on,
            pick,
        };
        let all = match prop {
            Prop::State => vec![
                c("Backlog", Some("to plan"), !d.planned, Pick::Planned(false)),
                c(
                    "Planned",
                    Some("its plan written, waits for your ack"),
                    d.planned,
                    Pick::Planned(true),
                ),
            ],
            Prop::Priority => PRIORITIES
                .iter()
                .zip(1u8..)
                .map(|(name, p)| c(name, None, d.priority == p, Pick::Priority(p)))
                .collect(),
            Prop::Project | Prop::PartOf => std::iter::once(c(
                if prop == Prop::Project {
                    "No project"
                } else {
                    "Part of none"
                },
                None,
                d.parent.is_none(),
                Pick::Parent(None),
            ))
            .chain(self.project_choices().into_iter().map(|(k, t)| {
                let on = d.parent.as_ref().is_some_and(|(p, _)| *p == k);
                c(&t, Some(&k), on, Pick::Parent(Some((k.clone(), t.clone()))))
            }))
            .collect(),
            Prop::Agent => std::iter::once(c(
                "Default",
                Some("livtk's own for the stage"),
                d.agent.is_none(),
                Pick::Agent(None),
            ))
            .chain(self.agents.files.iter().map(|f| {
                let name = f.run_name();
                c(
                    &name,
                    None,
                    d.agent.as_deref() == Some(name.as_str()),
                    Pick::Agent(Some(name.clone())),
                )
            }))
            .collect(),
            Prop::Status => vec![
                c(
                    "Active",
                    Some("committed: work on it now"),
                    d.active,
                    Pick::Active(true),
                ),
                c(
                    "Proposed",
                    Some("an idea, not yet committed"),
                    !d.active,
                    Pick::Active(false),
                ),
            ],
            Prop::Owner => {
                std::iter::once(c("No owner", None, d.owner.is_none(), Pick::Owner(None)))
                    .chain(self.owner_choices().into_iter().map(|o| {
                        let on = d.owner.as_deref() == Some(o.as_str());
                        c(&o, None, on, Pick::Owner(Some(o.clone())))
                    }))
                    .collect()
            }
            // The date picker offers its own.
            Prop::Target => Vec::new(),
        };
        let q = query.trim();
        let mut found: Vec<Choice> = if q.is_empty() {
            all
        } else {
            all.into_iter()
                .filter(|ch| {
                    super::fuzzy(q, &ch.label).is_some()
                        || ch
                            .sub
                            .as_deref()
                            .is_some_and(|s| super::fuzzy(q, s).is_some())
                })
                .collect()
        };
        // A new owner, as typed, when no one has that name yet.
        if prop == Prop::Owner
            && !q.is_empty()
            && !found.iter().any(|ch| ch.label.eq_ignore_ascii_case(q))
        {
            found.push(c(
                q,
                Some("new owner"),
                false,
                Pick::Owner(Some(q.to_owned())),
            ));
        }
        found
    }

    /// The relations a link can be added by from a node of `kind`, that `query` finds: one list
    /// for the panel's Add link and the form's.
    pub fn link_relations_for(&self, kind: Kind, query: &str) -> Vec<Choice> {
        super::link_choices(kind, &self.declared_rels())
            .into_iter()
            .filter(|l| query.trim().is_empty() || super::fuzzy(query.trim(), l.label).is_some())
            .map(|l| Choice {
                label: l.label.to_owned(),
                sub: None,
                on: false,
                pick: Pick::Link(l),
            })
            .collect()
    }

    /// The relations a link can be added by from what the form makes.
    pub fn link_relations(&self, query: &str) -> Vec<Choice> {
        self.draft.as_ref().map_or_else(Vec::new, |d| {
            self.link_relations_for(d.making.spec().kind, query)
        })
    }

    /// Set what `pick` sets on the draft. A relation opens the palette for the link's other
    /// end; whether it did.
    pub fn apply_pick(&mut self, pick: Pick) -> bool {
        if let Pick::Link(choice) = pick {
            self.open_draft_link_picker(choice);
            return self.palette.is_some();
        }
        let Some(d) = self.draft.as_mut() else {
            return false;
        };
        match pick {
            Pick::Planned(p) => d.planned = p,
            Pick::Priority(p) => d.priority = p.clamp(1, 4),
            Pick::Parent(p) => d.set_parent(p),
            Pick::Agent(a) => d.agent = a,
            Pick::Active(a) => d.active = a,
            Pick::Owner(o) => d.owner = o.map(|o| o.trim().to_owned()).filter(|o| !o.is_empty()),
            Pick::Target(t) => d.target = t,
            Pick::Link(_) => {}
        }
        false
    }

    /// What `prop`'s chip says while it is set; `None`, unset (the chip says its name).
    pub fn prop_value(&self, prop: Prop, today: Date) -> Option<String> {
        let d = self.draft.as_ref()?;
        match prop {
            Prop::State => Some(if d.planned { "Planned" } else { "Backlog" }.to_owned()),
            Prop::Priority => PRIORITIES
                .get(usize::from(d.priority.clamp(1, 4) - 1))
                .map(|s| (*s).to_owned()),
            Prop::Project | Prop::PartOf => d.parent.as_ref().map(|(_, t)| t.clone()),
            Prop::Agent => d.agent.clone(),
            Prop::Status => Some(if d.active { "Active" } else { "Proposed" }.to_owned()),
            Prop::Owner => d.owner.clone(),
            Prop::Target => d.target.map(|t| super::dates::short(t, today)),
        }
    }
}
