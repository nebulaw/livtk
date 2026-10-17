//! The app's notifications: what it tells the human in the window, from an error to an update
//! ready. Each has a level, an optional title, a markdown text and up to two actions; they
//! stack, newest first, a few shown and the rest counted; the same one again counts up instead
//! of stacking; each leaves on its own after a time set by its level (an error stays until it
//! is closed), never while the pointer holds it. Raised through one call, so a policy (muted
//! sources, other times) can come later without touching what raises them.

use std::time::{Duration, Instant};

use super::Arrival;

/// What a notification is about: an error (a bug), a warning (a bell), a node waiting for the
/// human (the inbox), an update ready (a meteor), anything else told (a message) or done
/// (sparkles).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Bug,
    Bell,
    Inbox,
    Meteor,
    Message,
    Sparkles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}

impl Level {
    /// How long a notification of this level stays before it starts to leave; never, for an
    /// error.
    pub fn stays(self) -> Option<Duration> {
        match self {
            Level::Info | Level::Success => Some(Duration::from_secs(6)),
            Level::Warning => Some(Duration::from_secs(10)),
            Level::Error => None,
        }
    }
}

/// What a notification's button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Open what arrived in an inbox.
    Open(Arrival),
    /// Install the staged update.
    RestartToUpdate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub act: Act,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    /// Tells each apart, the same one counted again included: the view keys its animations by it.
    pub id: u64,
    pub level: Level,
    /// What it is about, which picks its icon.
    pub mark: Mark,
    /// Where it comes from ("livtk", a project) and what it is ("Update"), shown first as a
    /// breadcrumb; by its level when not given.
    pub from: Option<String>,
    pub what: Option<String>,
    pub title: Option<String>,
    /// Markdown.
    pub text: String,
    pub actions: Vec<Button>,
    /// How many times it was raised.
    pub count: usize,
    /// What raised it, when that can take it back (a project whose sync failed, once it works
    /// again).
    pub source: Option<String>,
    /// Fading out since then, if it is leaving.
    pub leaving: Option<Instant>,
    since: Instant,
    held: bool,
}

impl Toast {
    fn of(level: Level, text: impl Into<String>) -> Toast {
        Toast {
            id: 0,
            level,
            mark: match level {
                Level::Error => Mark::Bug,
                Level::Warning => Mark::Bell,
                Level::Info => Mark::Message,
                Level::Success => Mark::Sparkles,
            },
            from: None,
            what: None,
            title: None,
            text: text.into(),
            actions: Vec::new(),
            count: 1,
            source: None,
            leaving: None,
            since: Instant::now(),
            held: false,
        }
    }

    pub fn info(text: impl Into<String>) -> Toast {
        Toast::of(Level::Info, text)
    }

    pub fn success(text: impl Into<String>) -> Toast {
        Toast::of(Level::Success, text)
    }

    pub fn warning(text: impl Into<String>) -> Toast {
        Toast::of(Level::Warning, text)
    }

    pub fn error(text: impl Into<String>) -> Toast {
        Toast::of(Level::Error, text)
    }

    pub fn title(mut self, title: impl Into<String>) -> Toast {
        self.title = Some(title.into());
        self
    }

    /// What it is about, for its icon.
    pub fn mark(mut self, mark: Mark) -> Toast {
        self.mark = mark;
        self
    }

    /// Where it comes from and what it is: "tasks", "Sync failed".
    pub fn about(mut self, from: impl Into<String>, what: impl Into<String>) -> Toast {
        self.from = Some(from.into());
        self.what = Some(what.into());
        self
    }

    /// Its breadcrumb: where it comes from (the app, unless told) and what it is (by its
    /// level, unless told).
    pub fn crumbs(&self) -> (&str, &str) {
        let from = self.from.as_deref().unwrap_or("livtk");
        let what = self.what.as_deref().unwrap_or(match self.level {
            Level::Error => "Error",
            Level::Warning => "Warning",
            Level::Info => "Notice",
            Level::Success => "Done",
        });
        (from, what)
    }

    /// What raised it, so it can be taken back when that no longer holds.
    pub fn source(mut self, source: impl Into<String>) -> Toast {
        self.source = Some(source.into());
        self
    }

    /// A button, of the two at most a notification has.
    pub fn action(mut self, label: impl Into<String>, act: Act) -> Toast {
        if self.actions.len() < 2 {
            self.actions.push(Button {
                label: label.into(),
                act,
            });
        }
        self
    }

    pub fn is_error(&self) -> bool {
        self.level == Level::Error
    }

    fn same_as(&self, other: &Toast) -> bool {
        self.level == other.level && self.title == other.title && self.text == other.text
    }
}

/// How many notifications show at once; the rest are counted.
pub const SHOWN: usize = 3;
/// How many are kept at most: beyond, the oldest go.
pub const KEPT: usize = 20;

/// The notifications, oldest first.
#[derive(Debug, Default)]
pub struct Toasts {
    list: Vec<Toast>,
    raised: u64,
}

impl Toasts {
    /// Raise `t`: the same one already there counts up, starts its time again and comes to the
    /// top, keeping its id (so what the pointer and Show more hold of it stays with it); else
    /// it is added, the oldest going beyond `KEPT`. Its id.
    pub fn raise(&mut self, mut t: Toast) -> u64 {
        if let Some(i) = self.list.iter().position(|o| o.same_as(&t)) {
            let mut old = self.list.remove(i);
            old.count += 1;
            old.since = Instant::now();
            old.leaving = None;
            old.actions = t.actions;
            let id = old.id;
            self.list.push(old);
            return id;
        }
        self.raised += 1;
        t.id = self.raised;
        self.list.push(t);
        let over = self.list.len().saturating_sub(KEPT);
        self.list.drain(..over);
        self.raised
    }

    /// Whether notification `id` is still there.
    pub fn contains(&self, id: u64) -> bool {
        self.list.iter().any(|t| t.id == id)
    }

    /// Take back what `source` raised: what it told of no longer holds.
    pub fn clear_source(&mut self, source: &str) {
        self.list.retain(|t| t.source.as_deref() != Some(source));
    }

    /// The ones shown, newest first, and how many more there are.
    pub fn shown(&self) -> (Vec<&Toast>, usize) {
        let shown: Vec<&Toast> = self.list.iter().rev().take(SHOWN).collect();
        let more = self.list.len().saturating_sub(SHOWN);
        (shown, more)
    }

    /// The newest.
    pub fn latest(&self) -> Option<&Toast> {
        self.list.last()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn dismiss(&mut self, id: u64) {
        self.list.retain(|t| t.id != id);
    }

    pub fn dismiss_all(&mut self) {
        self.list.clear();
    }

    /// Close the errors: what they told of no longer holds (a move was made after all).
    pub fn clear_errors(&mut self) {
        self.list.retain(|t| !t.is_error());
    }

    /// The pointer came over notification `id` (`held`), which keeps it, a leaving one
    /// included, or went away at `now`, which gives it its whole time again.
    pub fn hold(&mut self, id: u64, held: bool, now: Instant) {
        if let Some(t) = self.list.iter_mut().find(|t| t.id == id) {
            t.held = held;
            if held {
                t.leaving = None;
            } else {
                t.since = now;
            }
        }
    }

    /// Let time pass to `now`: each shown whose time is up starts to leave, unless held, and is
    /// gone `fade` after; one waiting below the shown ones is not seen yet, so its time waits.
    /// Whether anything changed, so the view repaints only then.
    pub fn age(&mut self, now: Instant, fade: Duration) -> bool {
        let mut changed = false;
        let below = self.list.len().saturating_sub(SHOWN);
        for t in &mut self.list[..below] {
            if t.leaving.is_none() {
                t.since = now;
            }
        }
        for t in &mut self.list[below..] {
            let Some(stays) = t.level.stays() else {
                continue;
            };
            if t.held || t.leaving.is_some() {
                continue;
            }
            if now.duration_since(t.since) >= stays {
                t.leaving = Some(now);
                changed = true;
            }
        }
        let before = self.list.len();
        self.list.retain(|t| {
            t.leaving
                .is_none_or(|at| t.held || now.duration_since(at) < fade)
        });
        changed || self.list.len() != before
    }
}
