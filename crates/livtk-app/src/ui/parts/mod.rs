//! The design system's parts: each one look, from theme tokens only, named in docs/design.md.
//! Views compose them and never restyle them.

mod foot_row;

pub(super) use foot_row::{FootRow, Lead, Tone};
