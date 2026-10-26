//! What a list of one kind offers: one description per kind, so every part of the app that asks
//! what a view can do (the dock, its menus, the palette, the board, an empty list's text) reads
//! it here rather than checking which view it is. It sits beside `Making`, which says how the
//! form makes a kind; a list offers to make one where the form makes its kind.

use livtk_core::Kind;

use super::keys;
use super::making::Making;
use super::{Grouping, Ordering, View};

/// What a list of one kind offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListSpec {
    pub kind: Kind,
    /// What the dock's main button and an empty list make: the kind itself, where the form
    /// makes it.
    pub making: Option<Making>,
    /// Laid out as a board too, one column per state.
    pub board: bool,
    /// Its nodes have a priority: grouped, ordered and filtered by it.
    pub priority: bool,
    /// Grouped by the project each node is part of.
    pub by_project: bool,
    /// Its nodes sit inside one another (a sub-task, a sub-project): shown nested.
    pub nests: bool,
    /// One of its nodes, in a sentence: "task", "project".
    pub noun: &'static str,
}

impl ListSpec {
    /// The description of a list of `kind`.
    pub fn of(kind: Kind) -> ListSpec {
        let builtin = |k: Kind| kind == k;
        ListSpec {
            kind,
            making: Making::ALL.into_iter().find(|m| m.spec().kind == kind),
            // The kinds whose work moves through states as it goes: tasks, projects, declared
            // kinds. Lessons, decisions and findings are records, read as a list.
            board: !(builtin(Kind::Lesson) || builtin(Kind::Decision) || builtin(Kind::Finding)),
            priority: builtin(Kind::Task),
            by_project: builtin(Kind::Task),
            nests: builtin(Kind::Task) || builtin(Kind::Project),
            noun: kind.as_str(),
        }
    }

    /// Whether the list offers `grouping`.
    pub fn offers_grouping(&self, grouping: Grouping) -> bool {
        match grouping {
            Grouping::Priority => self.priority,
            Grouping::Project => self.by_project,
            Grouping::State | Grouping::None => true,
        }
    }

    /// Whether the list offers `ordering`.
    pub fn offers_ordering(&self, ordering: Ordering) -> bool {
        ordering != Ordering::Priority || self.priority
    }

    /// The nesting option's name: "Nest sub-tasks", "Nest sub-projects".
    pub fn nest_label(&self) -> String {
        format!("Nest sub-{}s", self.noun)
    }

    /// An empty list's way on, in a sentence: clear the filter, press `keys` to `reveal` what
    /// hides, or make one where the list can. The list and the board both say it.
    pub fn empty_text(&self, keys: &str, reveal: &str) -> String {
        match self.making {
            Some(m) => format!(
                "Clear the filter, press {keys} {reveal}, or {} for a new {}.",
                m.spec().key,
                self.noun
            ),
            None => format!("Clear the filter, or press {keys} {reveal}."),
        }
    }

    /// The empty list's text: dismissed work is what may hide.
    pub fn empty_list_text(&self) -> String {
        self.empty_text(keys::DISMISSED, "to show dismissed work")
    }

    /// The empty board's text: empty columns are what may hide.
    pub fn empty_board_text(&self) -> String {
        self.empty_text(keys::EMPTY_GROUPS, "for every column")
    }
}

impl View {
    /// What the view's list offers, where it lists one kind; the inbox, the feed and the other
    /// views have no such list.
    pub fn listing_spec(self) -> Option<ListSpec> {
        self.kind().map(ListSpec::of)
    }

    /// Whether the view's nodes have a priority to filter, group and order by: as its kind's
    /// list says; a view of mixed kinds (the inbox) has them.
    pub fn has_priority(self) -> bool {
        self.listing_spec().is_none_or(|s| s.priority)
    }

    /// Whether the view offers `grouping`, as `has_priority`.
    pub fn offers_grouping(self, grouping: Grouping) -> bool {
        self.listing_spec()
            .is_none_or(|s| s.offers_grouping(grouping))
    }

    /// Whether the view offers `ordering`, as `offers_grouping`.
    pub fn offers_ordering(self, ordering: Ordering) -> bool {
        self.listing_spec()
            .is_none_or(|s| s.offers_ordering(ordering))
    }
}
