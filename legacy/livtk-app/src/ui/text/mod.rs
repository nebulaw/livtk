//! Text, read and edited alike: the style tokens every piece of text takes its look from
//! (`style`), a piece of text edited in place (`inline_edit`), the app's one markdown renderer
//! (`markdown`), and a markdown source read and edited block by block (`blocks`). A title, a
//! task's body and a doc all go through these, so they read, edit and grow alike.

mod blocks;
mod inline_edit;
mod markdown;
mod style;

pub(super) use blocks::{BlockAt, BlockViews, Blocks, Edge};
pub(super) use inline_edit::{InlineEdit, ReadHeights};
pub(super) use markdown::markdown;
pub(super) use style::TextRole;
