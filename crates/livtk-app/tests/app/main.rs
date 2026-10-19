//! livtk-app's tests, as one test binary: GPUI is linked once, not once per file.
//!
//! What is tested: the model's logic and rules, how keys and clicks behave in the window
//! (headless), and a few layout invariants. How things look (colours, sizes, animation) is
//! judged in the running app, not here.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

mod activity;
mod applog;
mod docs;
mod flow;
mod graph;
mod launch;
mod links;
mod markdown;
mod model;
mod palette;
mod panes;
mod saved;
mod style;
mod terminal;
mod ui;
