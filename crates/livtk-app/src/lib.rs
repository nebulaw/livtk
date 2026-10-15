//! The livtk desktop app. `model` holds all state and behaviour without any UI, so it is tested
//! as plain Rust; `ui` renders it with GPUI and forwards input to it.

pub mod model;
pub mod ui;
