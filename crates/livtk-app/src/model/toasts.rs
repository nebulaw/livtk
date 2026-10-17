//! The app's notifications: what it tells the human in the window, from an error to an update
//! ready. Each has a level, an optional title, a markdown text and up to two actions; they
//! stack, newest first, a few shown and the rest counted; the same one again counts up instead
//! of stacking; each leaves on its own after a time set by its level (an error stays until it
//! is closed), never while the pointer holds it. Raised through one call, so a policy (muted
//! sources, other times) can come later without touching what raises them.

use std::time::{Duration, Instant};

use super::Arrival;

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
    pub title: Option<String>,
    /// Markdown.
    pub text: String,
    pub actions: Vec<Button>,
    /// How many times it was raised.
    pub count: usize,
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
            title: None,
            text: text.into(),
            actions: Vec::new(),
            count: 1,
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

/// The notifications, oldest first.
#[derive(Debug, Default)]
pub struct Toasts {
    list: Vec<Toast>,
    raised: u64,
}

impl Toasts {
    /// Raise `t`: the same one already there counts up and starts its time again, newest;
    /// else it is added. Its id.
    pub fn raise(&mut self, mut t: Toast) -> u64 {
        self.raised += 1;
        if let Some(i) = self.list.iter().position(|o| o.same_as(&t)) {
            let mut old = self.list.remove(i);
            old.count += 1;
            old.since = Instant::now();
            old.leaving = None;
            old.actions = t.actions;
            old.id = self.raised;
            self.list.push(old);
        } else {
            t.id = self.raised;
            self.list.push(t);
        }
        self.raised
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

    /// Let time pass to `now`: each whose time is up starts to leave, unless held, and is gone
    /// `fade` after. Whether anything changed, so the view repaints only then.
    pub fn age(&mut self, now: Instant, fade: Duration) -> bool {
        let mut changed = false;
        for t in &mut self.list {
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
