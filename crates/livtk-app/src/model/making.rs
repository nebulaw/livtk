//! Making a node from the app: what kinds the new-node form makes, how each is named and offered,
//! and the one way a filled form becomes a node. A kind the form makes is a row in `Making`; its
//! fields are its `New` variant; its rules stay in the core.

use livtk_core::{Actor, Command, Kind, NewProject, Rel};

use super::{Model, View};

/// A kind of node the app's form makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Making {
    Task,
    Project,
}

impl Making {
    /// Every kind the form makes, in the order they are offered.
    pub const ALL: [Making; 2] = [Making::Task, Making::Project];

    /// The kind the form makes from `view`: the one it lists, when the form makes that kind;
    /// else a task, as everywhere.
    pub fn for_view(view: View) -> Making {
        Making::ALL
            .into_iter()
            .find(|m| view.kind() == Some(m.spec().kind))
            .unwrap_or(Making::Task)
    }

    /// Whether `view` lists what the form makes.
    pub fn lists(view: View) -> bool {
        Making::ALL
            .iter()
            .any(|m| view.kind() == Some(m.spec().kind))
    }

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
    /// Where a new one starts, said beside the form's title.
    pub starts: &'static str,
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
    body_hint: "Add a plan: goal, approach, acceptance check…",
    starts: "starts in Backlog",
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
    body_hint: "Why it matters and how it gets there…",
    starts: "active, unless you say otherwise",
    card_id: "new-project-card",
    create_id: "create-project",
    empty_id: "empty-new-project",
};

/// A filled form: what to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum New {
    Task {
        title: String,
        body: String,
        priority: u8,
    },
    Project(NewProject),
}

impl Model {
    /// The projects a new one may be part of: every one not done or dropped, by key and title.
    pub fn project_choices(&self) -> Vec<(String, String)> {
        self.project()
            .and_then(|p| p.ws.projects().ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|s| {
                !matches!(
                    s.state,
                    livtk_core::State::Done | livtk_core::State::Dropped
                )
            })
            .map(|s| (s.key, s.title))
            .collect()
    }

    /// Make `new` as the human, in the open project: then show it, selected, in its own view.
    /// Whether it was made; why not is the notice.
    pub fn create(&mut self, new: New) -> bool {
        let Some(project) = self.projects.get_mut(self.current) else {
            return false;
        };
        let before = project.ws.newest_event().ok().flatten();
        let (kind, made) = match new {
            New::Task {
                title,
                body,
                priority,
            } => (
                Kind::Task,
                project.ws.execute(
                    Command::Create {
                        kind: Kind::Task,
                        title,
                        body,
                        priority: Some(priority),
                        links: Vec::<(Rel, String)>::new(),
                    },
                    &Actor::Human,
                ),
            ),
            New::Project(p) => {
                // The form asks what done means; the CLI leaves it to the body.
                if !p.says_done() {
                    self.warn("Say what done means: a project needs its Done when.");
                    return false;
                }
                (Kind::Project, project.ws.new_project(&p, &Actor::Human))
            }
        };
        match made {
            Ok(events) => {
                if !project.ws.config().on.is_empty() {
                    self.wrote.push((project.dir.clone(), before));
                }
                self.toasts.clear_errors();
                let created = events.first().map(|e| e.op.subjects().0);
                // Into its own view, wherever that shows: its pane, or this one.
                self.set_view(View::listing(kind));
                self.reload();
                if let Some(id) = created {
                    self.select(id);
                    self.open_detail();
                }
                true
            }
            Err(e) => {
                self.warn(e.to_string());
                false
            }
        }
    }
}
