//! livtk: durable tasks and memory for Claude Code and its human.
//!
//! The truth is an append-only event log (`.livtk/events.jsonl`, committed with the project).
//! A SQLite store holds every event plus the current state built from them, and can always be
//! rebuilt from the log. Every surface goes through [`Workspace`].

mod error;
pub mod event;
mod log;
pub mod model;
pub mod registry;
pub mod search;
mod store;
mod workspace;

pub use error::{Error, Result};
pub use event::{Event, Op};
pub use model::{Actor, Kind, Rel, State};
pub use search::Hit;
pub use store::{Brief, Node, Snapshot, Summary};
pub use workspace::{
    Command, Config, DIR_NAME, Hooks, Links, Memory, RECALL_MAX, Scope, Workspace, find_dir,
};
