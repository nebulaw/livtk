//! Links, as every list of them reads: grouped under their relation's heading, each node named
//! the same way (its state mark, key and title), a click opening it, an end (the × that takes
//! the link back) answering its own clicks. A node's panel and the new-node form draw theirs
//! with it; a node named anywhere else is one of its rows.

use std::rc::Rc;

use gpui_kit::*;
use livtk_core::{Kind, State};

use super::super::detail::group_label;
use super::super::node_ref::{link_row, link_title};
use super::super::theme::*;

/// What a click on a node does: open it, wherever its caller opens nodes.
pub(in crate::ui) type OnOpen = Rc<dyn Fn(&mut Window, &mut App)>;

/// A node in a list of links.
pub(in crate::ui) struct LinkItem {
    /// Its selector, for tests to find it by: unique in the list.
    pub selector: String,
    pub key: String,
    pub kind: Kind,
    pub state: State,
    pub title: String,
    /// What ends its row: the × that takes the link back.
    pub end: Option<Stateful<Div>>,
    pub open: OnOpen,
}

/// A node named in a list: its state mark, key and title; a click opens it, and only it.
pub(in crate::ui) fn node_row(item: LinkItem) -> Stateful<Div> {
    let LinkItem {
        selector,
        key,
        kind,
        state,
        title,
        end,
        open,
    } = item;
    link_row(selector)
        .hover(|h| h.bg(color(HOVER)))
        .child(state_mark(kind, state))
        .child(div().flex_none().text_color(color(TEXT_4)).child(key))
        .child(link_title(title))
        .children(end)
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            open(window, cx);
        })
}

/// Links grouped by relation, each group under its heading, in the order given.
#[derive(Default)]
pub(in crate::ui) struct LinkList {
    groups: Vec<(String, Vec<LinkItem>)>,
}

impl LinkList {
    pub(in crate::ui) fn new() -> Self {
        LinkList::default()
    }

    /// A group: its heading and its nodes. An empty one is left out.
    pub(in crate::ui) fn group(mut self, heading: impl Into<String>, items: Vec<LinkItem>) -> Self {
        if !items.is_empty() {
            self.groups.push((heading.into(), items));
        }
        self
    }

    pub(in crate::ui) fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub(in crate::ui) fn build(self) -> Div {
        let mut out = div().flex().flex_col().gap_3();
        for (heading, items) in self.groups {
            out = out.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(group_label(&heading))
                    .children(items.into_iter().map(node_row)),
            );
        }
        out
    }
}
