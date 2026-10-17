//! livtk's updates. What never changes is the contract: a versioned [`Manifest`] describing a
//! build, and the small helper program (`livtk-update`) that swaps the installed app for a
//! staged one and rolls back if it does not start. Where updates come from is what is meant to
//! change: a [`LocalFolder`] now, a feed over HTTPS later, both behind [`Source`].
//!
//! An update is never installed on its own: the app finds one, stages and verifies it in the
//! background, and offers "Restart to Update"; only then does the helper swap.

mod error;
pub mod manifest;
pub mod paths;
pub mod source;
pub mod stage;
pub mod swap;

pub use error::{Error, Result};
pub use manifest::{Arch, FORMAT, Manifest, Platform};
pub use source::{Found, LocalFolder, Source};
