//! Text styles as tokens: each role's size, line, weight, colour and the room around it, in one
//! table. A piece of text and its editor both take their look from here, so reading and editing
//! line up by construction. A new role (a heading level, a quote, code) is a row here.

use super::*;

/// What a piece of text is, for its look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TextRole {
    /// A node's title, at the top of its panel.
    Title,
    /// A node's body.
    Body,
    /// A quiet line about a node: when it was made and changed.
    Meta,
}

/// How a role's text looks, and the room around it.
#[derive(Clone, Copy, Debug)]
pub(super) struct TextStyle {
    pub size: Pixels,
    /// The line's height: the same reading and editing, so nothing moves.
    pub line: Pixels,
    pub weight: FontWeight,
    pub color: u32,
    /// The room around it, across and down: a hover's background reaches the edge of it.
    pub pad_x: Pixels,
    pub pad_y: Pixels,
}

impl TextRole {
    pub(super) const fn style(self) -> TextStyle {
        match self {
            TextRole::Title => TextStyle {
                size: px(20.),
                line: px(TITLE_LINE),
                weight: FontWeight::SEMIBOLD,
                color: TEXT,
                pad_x: px(8.),
                pad_y: px(2.),
            },
            TextRole::Body => TextStyle {
                size: px(14.),
                line: px(22.),
                weight: FontWeight::NORMAL,
                color: TEXT_2,
                pad_x: px(8.),
                pad_y: px(8.),
            },
            TextRole::Meta => TextStyle {
                size: px(12.),
                line: px(18.),
                weight: FontWeight::NORMAL,
                color: TEXT_4,
                pad_x: px(0.),
                pad_y: px(0.),
            },
        }
    }
}

impl TextStyle {
    /// `el` in this style: its text, and the room around it (hanging into the margin by it, so
    /// the text lines up with what is around it and a hover's background has room).
    pub(super) fn frame(self, el: Div) -> Div {
        el.mx(-self.pad_x)
            .px(self.pad_x)
            .py(self.pad_y)
            .text_size(self.size)
            .line_height(self.line)
            .font_weight(self.weight)
            .text_color(color(self.color))
    }
}
