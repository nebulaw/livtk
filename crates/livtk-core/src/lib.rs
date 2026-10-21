//! livtk: durable tasks and memory for Claude Code and its human.
//!
//! The truth is an append-only event log (`.livtk/events.jsonl`, committed with the project).
//! A SQLite store holds every event plus the current state built from them, and can always be
//! rebuilt from the log. Every surface goes through [`Workspace`].

pub mod docs;
mod error;
pub mod event;
pub mod export;
mod log;
pub mod model;
pub mod on;
pub mod registry;
#[cfg(feature = "embed")]
pub mod rows;
pub mod search;
pub mod signal;
mod store;
mod workspace;

pub use error::{Error, Result};
pub use event::{Event, Op};
pub use model::{Actor, Health, Kind, KindSpec, Rel, RelSpec, State};
pub use search::Hit;
pub use store::{Brief, DayEvents, Node, Snapshot, Summary};
pub use workspace::{
    ACTIVE_AT_ONCE, Activity, Command, Config, DIR_NAME, Gate, Gates, HOOK_NAMES, Hooks, Links,
    Memory, PROMPT_SECTIONS, Progress, RECALL_MAX, SESSION_SECTIONS, Scope, Sections, UNSEEN_CAP,
    UPDATE_EVERY_DAYS, Update, Waiting, Workspace, find_dir, link_allowed,
};
