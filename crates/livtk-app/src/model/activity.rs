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
        if self.view == View::Activity
            && let Some(p) = self.projects.get(self.current)
        {
            self.activity_seen_before = p.ws.activity_seen().unwrap_or(None);
        }
    }

    /// Load the newest events, as many as are shown already, and mark them seen: the human is
    /// looking at them.
    pub(super) fn load_activity(&mut self) {
        let Some(p) = self.projects.get(self.current) else {
            return;
        };
        let want = self.activity.len().max(PAGE);
        match p.ws.activity(None, want) {
            Ok(events) => {
                self.activity_more = events.len() == want;
                if let Some(newest) = events.first()
                    && let Err(e) = p.ws.set_activity_seen(newest.event.id)
                {
                    self.notice = Some(e.to_string());
                }
                // The highlight stays on the event it was on as newer ones arrive above it.
                let on = self.activity.get(self.activity_index).map(|a| a.event.id);
                self.activity = events;
                self.activity_index = on
                    .and_then(|id| self.activity.iter().position(|a| a.event.id == id))
                    .unwrap_or(0);
            }
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    /// "Show older": the next page.
    pub fn activity_older(&mut self) {
        let Some(p) = self.projects.get(self.current) else {
            return;
        };
        let before = self.activity.last().map(|a| a.event.id);
        match p.ws.activity(before, PAGE) {
            Ok(older) => {
                self.activity_more = older.len() == PAGE;
                self.activity.extend(older);
            }
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    pub(super) fn activity_select_by(&mut self, delta: isize) {
        if !self.activity.is_empty() {
            self.activity_index = self
                .activity_index
                .saturating_add_signed(delta)
                .min(self.activity.len() - 1);
            if self.detail.is_some() {
                self.open_detail();
            }
        }
    }

    /// Highlight event `index`, and open its node if the panel is open.
    pub fn activity_select(&mut self, index: usize) {
        if index < self.activity.len() {
            self.activity_index = index;
            if self.detail.is_some() {
                self.open_detail();
            }
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
    }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}
