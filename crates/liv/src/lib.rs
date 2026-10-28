//! Liv: livtk's app, built from scratch on `livtk-ui` (proposal 0003). While it is built, it lives
//! beside the app in use (`livtk.app`) and never touches it: its own identity, folders and log.

pub mod identity;
pub mod log_file;
pub mod settings;
pub mod updates;
