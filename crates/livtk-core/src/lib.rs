//! livtk: durable tasks and memory for Claude Code and its human.
//!
//! The truth is an append-only event log (`.livtk/events.jsonl`, committed with the project).
//! A SQLite store holds every event plus the current state built from them, and can always be
//! rebuilt from the log. Every surface goes through [`Workspace`].

pub mod agents;
pub mod claude;
pub mod docs;
mod error;
pub mod event;
pub mod export;
pub mod format;
mod log;
pub mod md;
pub mod model;
pub mod on;
pub mod permits;
pub mod registry;
#[cfg(feature = "embed")]
pub mod rows;
pub mod run;
pub mod search;
pub mod sessions;
pub mod signal;
mod store;
mod workspace;

pub use error::{Error, Result};
pub use event::{Event, Op};
pub use model::{Actor, Health, Kind, KindSpec, Rel, RelSpec, State};
pub use search::Hit;
pub use store::{Brief, DayEvents, Node, SessionSummary, SessionUse, Snapshot, Summary, Unclaimed};
pub use workspace::{
    ACTIVE_AT_ONCE, Activity, BUILDS_AT_ONCE, BUILDS_MAX, Command, Config, DIR_NAME, Flow, Gate,
    Gates, HOOK_NAMES, Hooks, Lead, Links, Memory, NewProject, PROMPT_SECTIONS, Progress, Queue,
    RECALL_MAX, Runs, SESSION_SECTIONS, Scope, Sections, StateTime, UNSEEN_CAP, UPDATE_EVERY_DAYS,
    Update, Waiting, Workspace, done_when, find_dir, link_allowed,
};
