//! A markdown source read and edited block by block: every block drawn by the app's markdown
//! renderer, and the one being edited in its place as its own source, in its own style (a
//! heading at its size, a code block in its box). A click puts the caret where it fell in the
//! source, markup and all, as the renderer knows each piece of text's place in it. What the
//! source belongs to, and how an edit is kept, is the owner's: a task's body now, a doc's
//! section later.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui_component::text::{TextView, TextViewState};
use livtk_core::md::{self, Block, BlockKind};

use super::super::*;
use super::inline_edit::{InlineEdit, Lines, ReadHeights};
use super::markdown::{body_style, markdown, task_lists};
use super::style::{BLOCK_GAP, TextRole};

/// The block being edited, as it was when it opened: where it is among the blocks, its bytes
/// in the source, and its kind. A new block (in an empty source, or after the last) has an
/// empty range at the end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct BlockAt {
    pub index: usize,
    pub range: Range<usize>,
    pub kind: BlockKind,
    /// Its text when it opened, to tell whether the source moved under it.
    pub text: String,
}

impl BlockAt {
    fn of(source: &str, index: usize, block: &Block) -> Self {
        BlockAt {
            index,
            range: block.range.clone(),
            kind: block.kind,
            text: source[block.range.clone()].to_owned(),
        }
    }

    /// `source`'s last block, if it has any.
    pub(in crate::ui) fn last(source: &str) -> Option<Self> {
        let blocks = md::blocks(source);
        let i = blocks.len().checked_sub(1)?;
        Some(BlockAt::of(source, i, &blocks[i]))
    }

    /// A new block, after `source`'s last.
    pub(in crate::ui) fn new_after(source: &str) -> Self {
        BlockAt {
            index: md::blocks(source).len(),
            range: source.len()..source.len(),
            kind: BlockKind::Paragraph,
            text: String::new(),
        }
    }

    /// The block `edge` of this one in `source` (which may have changed since it opened, by
    /// its own edit): the last that ends before it starts, or the first that starts after its
    /// `len` bytes. `None` past the first or last.
    pub(in crate::ui) fn beside(&self, source: &str, len: usize, edge: Edge) -> Option<Self> {
        let blocks = md::blocks(source);
        let found = match edge {
            Edge::Above => blocks
                .iter()
                .enumerate()
                .rev()
                .find(|(_, b)| b.range.end <= self.range.start),
            Edge::Below => blocks
                .iter()
                .enumerate()
                .find(|(_, b)| b.range.start >= self.range.start + len),
        };
        found.map(|(i, b)| BlockAt::of(source, i, b))
    }

    /// `source` with this block's text `text`: spliced where it was if the source still holds
    /// it there; added after the last if it is new.
    pub(in crate::ui) fn write(&self, source: &str, text: &str) -> String {
        if self.range.is_empty() && self.text.is_empty() {
            return md::append(source, text);
        }
        if text.trim_end_matches(['\n', '\r']) == self.text {
            return source.to_owned();
        }
        if source.get(self.range.clone()) == Some(self.text.as_str()) {
            return md::splice(source, self.range.clone(), text);
        }
        // The source moved under the edit (written elsewhere meanwhile): the block where it is
        // now, if it is still there; kept after the last otherwise, never lost.
        match self.nearest_in(source, &md::blocks(source)) {
            Some(i) => md::splice(source, md::blocks(source)[i].range.clone(), text),
            None => md::append(source, text),
        }
    }

    /// Which of `blocks` (of `source`) holds this block's text now, nearest its old place.
    fn nearest_in(&self, source: &str, blocks: &[Block]) -> Option<usize> {
        blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| source[b.range.clone()] == self.text)
            .min_by_key(|(i, _)| i.abs_diff(self.index))
            .map(|(i, _)| i)
    }

    /// Whether this is a new block, after the last.
    fn is_new(&self) -> bool {
        self.range.is_empty() && self.text.is_empty()
    }

    /// The look a block of this kind reads and edits in.
    pub(in crate::ui) fn role(&self) -> TextRole {
        match self.kind {
            BlockKind::Heading { level } => TextRole::Heading(level),
            BlockKind::Code { .. } | BlockKind::Math => TextRole::Code,
            _ => TextRole::Body,
        }
    }
}

pub(in crate::ui) use super::inline_edit::Edge;

/// A block's text as drawn alone: with the body's link definitions after it, when it is a kind
/// that may hold a link (a code block's text is its own; an unclosed fence would take them in).
fn with_context(text: &str, kind: BlockKind, context: &str) -> String {
    let links = matches!(
        kind,
        BlockKind::Paragraph
            | BlockKind::Heading { .. }
            | BlockKind::List { .. }
            | BlockKind::Quote
            | BlockKind::Table
            | BlockKind::Footnote
    );
    if links && !context.is_empty() {
        format!("{text}\n\n{context}")
    } else {
        text.to_owned()
    }
}

/// Each block's view state, kept between frames by its selector, so a click's place in the
/// source can be read back from it.
#[derive(Clone, Default)]
pub(in crate::ui) struct BlockViews(Rc<RefCell<HashMap<SharedString, Entity<TextViewState>>>>);

impl BlockViews {
    fn state(&self, key: &SharedString, text: &str, cx: &mut App) -> Entity<TextViewState> {
        let found = self.0.borrow().get(key).cloned();
        match found {
            Some(state) => {
                state.update(cx, |s, cx| s.set_text(text, cx));
                state
            }
            None => {
                let state = cx.new(|cx| TextViewState::markdown(text, cx).selectable(true));
                self.0.borrow_mut().insert(key.clone(), state.clone());
                state
            }
        }
    }
}

/// A source's blocks, as configured: their selectors' prefix, what an empty source says, how
/// wide a diagram may be.
pub(in crate::ui) struct Blocks {
    pub prefix: &'static str,
    pub placeholder: &'static str,
    pub width: f32,
}

impl Blocks {
    /// The editor of `at`'s block, the gap to the next its own.
    pub(in crate::ui) fn edit(&self, at: &BlockAt) -> InlineEdit {
        InlineEdit::new(
            at.role(),
            self.read_id(at.index),
            format!("{}-block-editor", self.prefix),
        )
        .lines(Lines::Many)
        .walks(true)
        .below(BLOCK_GAP)
        .placeholder(self.placeholder)
    }

    fn read_id(&self, index: usize) -> SharedString {
        format!("{}-block-{index}", self.prefix).into()
    }

    /// `source`, block by block; `editing`, the block being edited and its field; `open` runs
    /// on the double-click that opens a block, with the bytes of it the double-click picked.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::ui) fn render(
        &self,
        source: &str,
        editing: Option<(&BlockAt, &Entity<TextareaState>)>,
        open: impl Fn(&mut LivtkApp, BlockAt, Option<Range<usize>>, &mut Window, &mut Context<LivtkApp>)
        + 'static,
        views: &BlockViews,
        heights: &ReadHeights,
        cx: &mut Context<LivtkApp>,
    ) -> Div {
        let open = Rc::new(open);
        let blocks = md::blocks(source);
        let context = md::context(source, &blocks);
        let prefix = self.prefix;
        let mut out = div()
            .debug_selector(move || format!("{prefix}-blocks"))
            .flex()
            .flex_col()
            // The last block's gap is the body's own end.
            .mb(-BLOCK_GAP);
        // The block the edit stands in for as drawn: the one holding its text, nearest its old
        // place (the source may have moved under it); its old place if none does.
        let edited_at = editing
            .filter(|(edited, _)| !edited.is_new())
            .map(|(edited, _)| edited.nearest_in(source, &blocks).unwrap_or(edited.index));
        for (i, block) in blocks.iter().enumerate() {
            let at = BlockAt::of(source, i, block);
            if let Some((edited, field)) = editing
                && edited_at == Some(i)
            {
                out = out.child(self.edit(edited).editing(field, heights, cx));
                continue;
            }
            // Drawn alone, with the link definitions any block may name.
            let text = &source[block.range.clone()];
            let shown = with_context(text, block.kind, &context);
            let key = self.read_id(i);
            let state = views.state(&key, &shown, cx);
            let (len, located, open) = (text.len(), state.clone(), open.clone());
            out = out.child(
                self.edit(&at).reading(
                    task_lists(
                        markdown(TextView::new(&state), self.width),
                        block.range.start,
                        context.clone(),
                        self.width,
                    )
                    .style(body_style()),
                    // The word the double-click picked, in the block's source, as the renderer maps
                    // it: past any markup around it.
                    move |cx| {
                        let range = located.read(cx).selected_source_range()?;
                        (range.end <= len).then_some(range)
                    },
                    move |this, caret, window, cx| open(this, at.clone(), caret, window, cx),
                    heights,
                    cx,
                ),
            );
        }
        // After the last: a new block being written, or the placeholder of an empty source.
        if let Some((edited, field)) = editing
            && (edited.is_new() || edited_at.is_some_and(|i| i >= blocks.len()))
        {
            out = out.child(self.edit(edited).editing(field, heights, cx));
        } else if blocks.is_empty() {
            let at = BlockAt::new_after(source);
            out = out.child(self.edit(&at).reading(
                div().text_color(color(TEXT_4)).child(self.placeholder),
                |_| None,
                move |this, caret, window, cx| open(this, at.clone(), caret, window, cx),
                heights,
                cx,
            ));
        }
        out
    }
}
