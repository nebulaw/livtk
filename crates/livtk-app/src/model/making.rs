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
            .find(|m| view.kind() == Some(m.kind()))
            .unwrap_or(Making::Task)
    }

    /// Whether `view` lists what the form makes.
    pub fn lists(view: View) -> bool {
        Making::ALL.iter().any(|m| view.kind() == Some(m.kind()))
    }

    pub const fn kind(self) -> Kind {
        match self {
            Making::Task => Kind::Task,
            Making::Project => Kind::Project,
        }
    }

    /// What the form and the palette call making one.
    pub const fn title(self) -> &'static str {
        match self {
            Making::Task => "New task",
            Making::Project => "New project",
        }
    }

    /// The key that opens the form for it, from anywhere.
    pub const fn key(self) -> &'static str {
        match self {
            Making::Task => "N",
            Making::Project => "P",
        }
    }

    /// The form's button.
    pub const fn create(self) -> &'static str {
        match self {
            Making::Task => "Create task",
            Making::Project => "Create project",
        }
    }

    /// What its title asks for, and its body.
    pub const fn placeholders(self) -> (&'static str, &'static str) {
        match self {
            Making::Task => (
                "Task title",
                "Add a plan: goal, approach, acceptance check…",
            ),
            Making::Project => (
                "The outcome, in a line",
                "Why it matters and how it gets there…",
            ),
        }
    }

    /// Where a new one starts, said beside the form's title.
    pub const fn starts(self) -> &'static str {
        match self {
            Making::Task => "starts in Backlog",
            Making::Project => "active, unless you say otherwise",
        }
    }

    /// Its selectors: the form's card and button, and an empty list's offer to make one.
    pub const fn ids(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Making::Task => ("new-task-card", "create-task", "empty-new-task"),
            Making::Project => ("new-project-card", "create-project", "empty-new-project"),
        }
    }
}

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
                if p.done_when.trim().is_empty() {
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
