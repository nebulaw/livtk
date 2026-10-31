//! Updates for livtk's apps. What never changes is the contract: a versioned [`Manifest`]
//! describing a build, signed ([`sign`]), and the swap that puts a staged build in place of the
//! installed one and rolls back if it does not start. Where updates come from varies behind
//! [`Source`]: a [`LocalFolder`] on this machine, or a repository's releases on GitHub
//! ([`github`]), read through [`fetch::Fetch`] (the real HTTPS client is the `http` feature).
//!
//! An update is never installed on its own: the app finds one, stages and verifies it in the
//! background, and offers "Restart to Update"; only then does the helper swap.

mod error;
pub mod fetch;
pub mod github;
pub mod manifest;
pub mod sign;
pub mod source;
pub mod stage;
pub mod swap;

pub use error::{Error, Result};
pub use manifest::{Arch, FORMAT, Manifest, Platform};
pub use source::{Artifact, Found, LocalFolder, Source};
