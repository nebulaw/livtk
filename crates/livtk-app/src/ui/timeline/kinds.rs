//! Every kind of event, in one table: the mark it wears on the rail (one shape, a 20px circle,
//! its glyph in it) and the line it says. The panel's timeline, the Activity feed and an agent's
//! work all read it; a new kind of event is a row here.

use livtk_core::event::{Event, Op};
use livtk_core::{Kind, State};

use super::super::icons::{Icon, icon};
use super::super::theme::*;
use super::super::{PRIORITY_NAMES, created_as, title_case};
use crate::model::timeline::{Mark, RunStep};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

/// A mark's size on the rail, and its glyph's.
pub(in crate::ui) const MARK: f32 = 20.;
const GLYPH: f32 = 12.;

/// What a mark holds.
enum Glyph {
    Icon(Icon, u32),
    /// The state moved to, as its mark.
    State(State),
    /// Who wrote it: you, or Claude.
    Avatar(bool),
}

fn glyph(mark: Mark) -> Glyph {
    match mark {
        Mark::Created => Glyph::Icon(Icon::NewTask, TEXT_3),
        Mark::Moved(s) => Glyph::State(s),
        Mark::Changed => Glyph::Icon(Icon::Edit, TEXT_3),
        Mark::Linked | Mark::Unlinked => Glyph::Icon(Icon::Link, TEXT_3),
        Mark::Doc => Glyph::Icon(Icon::Docs, TEXT_3),
        Mark::Session => Glyph::Icon(Icon::Sessions, TEXT_3),
        Mark::Note { human } => Glyph::Avatar(human),
        Mark::Run(RunStep::Started | RunStep::Ended) => Glyph::Icon(Icon::Claude, TEXT_2),
        Mark::Run(RunStep::Asks) => Glyph::Icon(Icon::Inbox, ACCENT),
        Mark::Run(RunStep::Answered) => Glyph::Icon(Icon::Check, TEXT_2),
    }
}

/// `mark` as drawn on the rail, for a node of `kind`: one circle, its glyph in it. `ahead`, a
/// state not reached yet: hollow, faint.
pub(in crate::ui) fn mark_view(mark: Mark, kind: Kind, ahead: bool) -> Div {
    let ring = div()
        .size(px(MARK))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center();
    match glyph(mark) {
        Glyph::Avatar(human) => ring
            .bg(color(if human { ACCENT } else { BORDER_STRONG }))
            .child(icon(
                if human { Icon::You } else { Icon::Claude },
                GLYPH,
                TEXT,
            )),
        Glyph::State(s) => ring
            .bg(color(CONTENT))
            .child(state_mark(kind, s))
            .when(ahead, |d| d.opacity(0.45)),
        Glyph::Icon(i, c) => ring
            .bg(color(CONTENT))
            .border_1()
            .border_color(color(BORDER_STRONG))
            .child(icon(i, GLYPH, c)),
    }
}

/// A text as one line: its first line that says anything, so a row never shows an empty line
/// nor runs onto the rows below.
pub(in crate::ui) fn first_line(text: &str) -> &str {
    text.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default()
}

/// What `e` did, as a line after who did it, on the node keyed `key` now.
pub(in crate::ui) fn line(e: &Event, key: &str) -> String {
    match &e.op {
        Op::NodeCreate { .. } => match created_as(e, key) {
            Some(first) => format!("created it as {first}"),
            None => "created it".to_owned(),
        },
        Op::NodeSet { data, .. } => {
            let mut parts = Vec::new();
            if let Some(s) = data.state {
                parts.push(format!("moved it to {}", title_case(s.as_str())));
            }
            if data.title.is_some() {
                parts.push("renamed it".into());
            }
            if data.body.is_some() {
                parts.push("edited the plan".into());
            }
            if let Some(p) = data.priority {
                parts.push(format!(
                    "set priority to {}",
                    PRIORITY_NAMES[usize::from(p.get().clamp(1, 4) - 1)]
                ));
            }
            if data.props.is_some() {
                parts.push("changed its scope".into());
            }
            parts.join(", ")
        }
        Op::NodeNote { data, .. } => data.text.clone(),
        Op::EdgeAdd { data } => format!("linked: {}", data.rel),
        Op::EdgeRemove { data } => format!("unlinked: {}", data.rel),
        Op::DocLink { data, .. } => format!("linked the doc {}", data.path),
        Op::DocUnlink { data, .. } => format!("unlinked the doc {}", data.path),
        Op::NodeSession { data, .. } => livtk_core::format::claim(data),
    }
}

/// Whether `e` reads as what was said (a card, its markdown rendered), not as a line: a note, a
/// run's question, and a run's end (Claude's last word on it, often several lines).
pub(in crate::ui) fn is_said(mark: Mark) -> bool {
    matches!(
        mark,
        Mark::Note { .. } | Mark::Run(RunStep::Asks | RunStep::Ended)
    )
}

/// Where a line of the timeline sticks while the panel is scrolled past it: to the panel's top,
/// its bottom, or nowhere. Rows stuck at one edge stack, in the timeline's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Stick {
    #[allow(
        dead_code,
        reason = "a kind that never sticks; every stickable one sticks for now"
    )]
    None,
    Top,
    Bottom,
}

/// A session's lane's first mark (where it branches off): stuck while its lane runs on in view,
/// so whose work is below stays said.
pub(in crate::ui) const LANE_START: Stick = Stick::Top;
/// The note being written: stuck while its place on the rail is out of view, always in reach.
pub(in crate::ui) const COMPOSER: Stick = Stick::Bottom;
