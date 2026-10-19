//! The activity feed: every event in the project, newest first, with what is new since the
//! human last looked marked. The view draws `feed()`; the rest is here.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use livtk_core::event::{Event, Op};
use livtk_core::{Activity, Actor};
use uuid::Uuid;

use super::{Model, View};

/// Events loaded at a time.
pub const PAGE: usize = 200;

/// A line of the feed as drawn: a day's heading, or the event at an index of `activity`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedRow {
    Day(String),
    Event(usize),
}

impl Model {
    /// Opening the feed: remember the mark as it was, so this visit shows what is new.
    pub(super) fn enter_activity(&mut self) {
        self.activity.clear();
        self.activity_index = 0;
        self.activity_more = false;
        if self.pane().view == View::Activity
            && let Some(p) = self.projects.get(self.current)
        {
            self.activity_seen_before = p.ws.activity_seen().unwrap_or(None);
        }
    }

    /// The grid's counts per day, for the bottom bar, whichever view is open.
    pub(super) fn load_grid(&mut self) {
        let Some(p) = self.projects.get(self.current) else {
            return;
        };
        match p.ws.activity_by_day() {
            Ok(days) => self.grid.set_days(&days),
            Err(e) => self.warn(e.to_string()),
        }
    }

    /// Load the newest events, as many as are shown already (of the days picked in the grid, if
    /// any), and mark them seen: the human is looking at them.
    pub(super) fn load_activity(&mut self) {
        let Some(p) = self.projects.get(self.current) else {
            return;
        };
        let want = self.activity.len().max(PAGE);
        let page = match self.grid.pick.span.and_then(super::grid::pick_bounds) {
            Some((from, to)) => p.ws.activity_in(&from, &to, None, want),
            None => p.ws.activity(None, want),
        };
        match page {
            Ok(events) => {
                self.activity_more = events.len() == want;
                // Only the unfiltered feed's newest is the newest the human has seen.
                if self.grid.pick.span.is_none()
                    && let Some(newest) = events.first()
                    && let Err(e) = p.ws.set_activity_seen(newest.event.id)
                {
                    self.warn(e.to_string());
                }
                // The highlight stays on the event it was on as newer ones arrive above it.
                let on = self.activity.get(self.activity_index).map(|a| a.event.id);
                self.activity = events;
                self.activity_index = on
                    .and_then(|id| self.activity.iter().position(|a| a.event.id == id))
                    .unwrap_or(0);
            }
            Err(e) => self.warn(e.to_string()),
        }
    }

    /// "Show older": the next page.
    pub fn activity_older(&mut self) {
        let Some(p) = self.projects.get(self.current) else {
            return;
        };
        let before = self.activity.last().map(|a| a.event.id);
        let page = match self.grid.pick.span.and_then(super::grid::pick_bounds) {
            Some((from, to)) => p.ws.activity_in(&from, &to, before, PAGE),
            None => p.ws.activity(before, PAGE),
        };
        match page {
            Ok(older) => {
                self.activity_more = older.len() == PAGE;
                self.activity.extend(older);
            }
            Err(e) => self.warn(e.to_string()),
        }
    }

    /// A day clicked in the grid: it anchors a range, or closes the one anchored. The feed
    /// shows the days picked only.
    pub fn grid_pick(&mut self, day: jiff::civil::Date) {
        self.grid.pick.click(day);
        self.reload_feed();
    }

    /// A month picked in the grid, to today at most.
    pub fn grid_pick_span(&mut self, first: jiff::civil::Date, last: jiff::civil::Date) {
        self.grid.pick.take(first, last.min(super::grid::today()));
        self.reload_feed();
    }

    /// The feed for every day again.
    pub fn grid_unpick(&mut self) {
        if self.grid.pick.clear() {
            self.reload_feed();
        }
    }

    /// The grid's who or what changed: it counts again.
    pub fn grid_changed(&mut self) {
        self.load_grid();
    }

    /// The feed again from its newest, for the days picked now: loaded at once in the Activity
    /// view (where the human sees it, and so has seen it), else when it is next opened.
    fn reload_feed(&mut self) {
        self.activity.clear();
        self.activity_index = 0;
        if self.pane().view == View::Activity {
            self.load_activity();
        }
    }

    pub(super) fn activity_select_by(&mut self, delta: isize) {
        if !self.activity.is_empty() {
            self.activity_index = self
                .activity_index
                .saturating_add_signed(delta)
                .min(self.activity.len() - 1);
            self.follow_detail();
        }
    }

    /// Highlight event `index`, and open its node if the panel is open.
    pub fn activity_select(&mut self, index: usize) {
        if index < self.activity.len() {
            self.activity_index = index;
            self.follow_detail();
        }
    }

    /// The node the highlighted event is about.
    pub(super) fn activity_node(&self) -> Option<Uuid> {
        self.activity
            .get(self.activity_index)
            .map(|a| a.event.op.subjects().0)
    }

    /// Whether an event is new on this visit: someone other than the human did it after the
    /// mark the feed was opened with.
    pub fn is_new(&self, a: &Activity) -> bool {
        a.event.by != Actor::Human.to_string()
            && self
                .activity_seen_before
                .is_none_or(|seen| a.event.id > seen)
    }

    /// The feed as drawn: each day's heading before its first event, in the system's time zone.
    pub fn feed(&self) -> Vec<FeedRow> {
        let tz = TimeZone::system();
        let today = jiff::Timestamp::now().to_zoned(tz.clone()).date();
        let mut rows = Vec::with_capacity(self.activity.len() + 8);
        let mut last: Option<Date> = None;
        for (i, a) in self.activity.iter().enumerate() {
            let day = a
                .event
                .at
                .parse::<jiff::Timestamp>()
                .map(|t| t.to_zoned(tz.clone()).date())
                .ok();
            if day != last {
                rows.push(FeedRow::Day(
                    day.map_or_else(|| "Earlier".to_owned(), |d| day_label(d, today)),
                ));
                last = day;
            }
            rows.push(FeedRow::Event(i));
        }
        rows
    }
}

/// "Today", "Yesterday", a weekday within the week, else the date, as Linear heads its days.
pub fn day_label(day: Date, today: Date) -> String {
    let days = (today - day).get_days();
    match days {
        0 => "Today".to_owned(),
        1 => "Yesterday".to_owned(),
        2..=6 => day.strftime("%A").to_string(),
        _ if day.year() == today.year() => day.strftime("%B %-d").to_string(),
        _ => day.strftime("%B %-d, %Y").to_string(),
    }
}

/// The key a node was created with, when `e` creates it and it is keyed otherwise now: two
/// branches created the same key, and replay gave this node the next free one.
pub fn created_as<'a>(e: &'a Event, key_now: &str) -> Option<&'a str> {
    match &e.op {
        Op::NodeCreate { data, .. } if data.key != key_now => Some(&data.key),
        _ => None,
    }
}

/// What an event did, in a few words, for its line in the feed.
pub fn describe(a: &Activity) -> String {
    match &a.event.op {
        Op::NodeCreate { .. } => match a.node.as_ref().and_then(|n| created_as(&a.event, &n.key)) {
            Some(first) => format!("created as {first}"),
            None => "created".to_owned(),
        },
        Op::NodeSet { data, .. } => {
            let mut parts = Vec::new();
            if let Some(s) = data.state {
                parts.push(format!("moved to {}", capitalised(s.as_str())));
            }
            if data.title.is_some() {
                parts.push("renamed".to_owned());
            }
            if data.body.is_some() {
                parts.push("edited the plan".to_owned());
            }
            if let Some(p) = data.priority {
                let name =
                    ["urgent", "high", "medium", "low"][usize::from(p.get().clamp(1, 4) - 1)];
                parts.push(format!("set priority to {name}"));
            }
            if data.props.is_some() {
                parts.push("changed the scope".to_owned());
            }
            if parts.is_empty() {
                "changed".to_owned()
            } else {
                parts.join(", ")
            }
        }
        Op::NodeNote { data, .. } => {
            let first = data.text.lines().next().unwrap_or_default();
            let short: String = first.chars().take(80).collect();
            let more = short.len() < first.len() || data.text.lines().nth(1).is_some();
            format!("noted “{short}{}”", if more { "…" } else { "" })
        }
        Op::EdgeAdd { data } | Op::EdgeRemove { data } => {
            let verb = if matches!(a.event.op, Op::EdgeAdd { .. }) {
                "linked"
            } else {
                "unlinked"
            };
            let other = a
                .other
                .as_ref()
                .map_or("a removed node", |b| b.key.as_str());
            format!("{verb}: {} {other}", data.rel.as_str().replace('_', " "))
        }
        Op::DocLink { data, .. } => format!("linked the doc {}", data.path),
        Op::DocUnlink { data, .. } => format!("unlinked the doc {}", data.path),
    }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// A note as markdown that keeps its line breaks: a note is written line by line (Shift+Enter),
/// where markdown would join single lines into one. Code blocks and blank lines are left as
/// they are.
pub fn note_markdown(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::with_capacity(text.len() + lines.len() * 2);
    let mut code = false;
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            code = !code;
        }
        out.push_str(line);
        if i + 1 < lines.len() {
            let next = lines[i + 1];
            if !code && !line.trim().is_empty() && !next.trim().is_empty() {
                out.push_str("  ");
            }
            out.push('\n');
        }
    }
    out
}

/// The indent Tab adds to a line that is no list item: two spaces.
pub const INDENT: &str = "  ";

/// A markdown list item's indent and the width of its marker with the space after it (`- `,
/// `* `, `+ `, `1. `, `12) `): where its text starts, which is where a sub-item must start.
fn list_item(line: &str) -> Option<(usize, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let width = if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        2
    } else if (1..=9).contains(&digits)
        && (rest[digits..].starts_with(". ") || rest[digits..].starts_with(") "))
    {
        digits + 2
    } else {
        return None;
    };
    Some((indent, width))
}

/// Tab or Shift+Tab in a body or a note, on every line the selection `sel` touches (the
/// cursor's line when nothing is selected). A list item is indented to the text of the item
/// above it, which makes it that item's sub-item in markdown, or outdented to the level of the
/// item it was under; any other line by [`INDENT`]. The edit, as the range of `text` to
/// replace and its replacement, and the selection after it.
pub fn reindent(
    text: &str,
    sel: std::ops::Range<usize>,
    out: bool,
) -> (std::ops::Range<usize>, String, std::ops::Range<usize>) {
    let start = text[..sel.start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[sel.end..]
        .find('\n')
        .map_or(text.len(), |i| sel.end + i);
    // The list items above the block, as they stand: (indent, marker width).
    let mut above: Vec<(usize, usize)> = text[..start].lines().filter_map(list_item).collect();
    let mut new = String::with_capacity(end - start + 16);
    let (mut first_shift, mut total) = (0isize, 0isize);
    for (i, line) in text[start..end].split('\n').enumerate() {
        if i > 0 {
            new.push('\n');
        }
        let have = line.len() - line.trim_start_matches(' ').len();
        let item = list_item(line);
        let want = match (item, out) {
            // Under the text of the nearest item above at most as deep as this one.
            (Some(_), false) => above
                .iter()
                .rev()
                .find(|(ind, _)| *ind <= have)
                .map_or(have + INDENT.len(), |(ind, w)| (ind + w).max(have + 1)),
            // Back to the level of the nearest item above that is shallower.
            (Some(_), true) => above
                .iter()
                .rev()
                .map(|(ind, _)| *ind)
                .find(|ind| *ind < have)
                .unwrap_or(0),
            (None, false) => have + INDENT.len(),
            (None, true) => have.saturating_sub(INDENT.len()),
        };
        new.push_str(&" ".repeat(want));
        new.push_str(&line[have..]);
        if let Some((_, w)) = item {
            above.push((want, w));
        }
        let shift = want as isize - have as isize;
        if i == 0 {
            first_shift = shift;
        }
        total += shift;
    }
    let moved = |at: usize, by: isize| at.saturating_add_signed(by).max(start);
    let after = if sel.is_empty() {
        let at = moved(sel.start, first_shift);
        at..at
    } else {
        moved(sel.start, first_shift)..moved(sel.end, total)
    };
    (start..end, new, after)
}
