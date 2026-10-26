//! Text styles as tokens: each role's size, line, weight, colour and the room around it, in one
//! table. A piece of text and its editor both take their look from here, and the markdown
//! renderer takes its heading sizes from here too, so reading and editing line up by
//! construction. A new role (a quote, a table's cell) is a row here.

use super::super::TITLE_LINE;
use super::super::theme::*;
use gpui_kit::*;

/// What a piece of text is, for its look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TextRole {
    /// A node's title, at the top of its panel.
    Title,
    /// A node's body: a paragraph, a list, a quote, a table.
    Body,
    /// A heading in a body, 1 to 6.
    Heading(u8),
    /// A code block in a body, fenced or indented.
    Code,
    /// A quiet line about a node: when it was made and changed.
    Meta,
}

/// How a role's text looks, and the room around it.
#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct TextStyle {
    pub size: Pixels,
    /// The line's height: the same reading and editing, so nothing moves.
    pub line: Pixels,
    pub weight: FontWeight,
    pub color: u32,
    /// The room around it, across and down: a hover's background reaches the edge of it.
    pub pad_x: Pixels,
    pub pad_y: Pixels,
    /// Set in the theme's monospace face, on the code background in a border (a code block, as
    /// the renderer draws one).
    pub code: bool,
}

/// The body's text size, which a heading's is measured against.
const BODY_SIZE: f32 = 14.;
/// The headings' sizes, 1 to 6.
const HEADING_SIZES: [f32; 6] = [20., 17., 15., 14., 13., 12.];
/// The markdown renderer's heading line heights, relative to their size, 1 to 6 (its own,
/// fixed): a heading's editor takes the same line.
const HEADING_LINES: [f32; 6] = [1.25, 1.35, 1.35, 1.35, 1.35, 1.35];
/// The renderer's code block: its text size, line (relative), and room across and down.
const CODE_SIZE: f32 = 13.;
const CODE_LINE: f32 = 1.6;
const CODE_PAD_X: f32 = 16.;
const CODE_PAD_Y: f32 = 14.;
/// The gap between a body's blocks.
pub(in crate::ui) const BLOCK_GAP: Pixels = px(12.);

/// A heading's size at `level` (1 to 6; deeper reads as 6).
pub(in crate::ui) fn heading_size(level: u8) -> Pixels {
    px(HEADING_SIZES[heading_ix(level)])
}

fn heading_ix(level: u8) -> usize {
    usize::from(level.clamp(1, 6) - 1)
}

impl TextRole {
    pub(in crate::ui) fn style(self) -> TextStyle {
        let plain = TextStyle {
            size: px(BODY_SIZE),
            line: px(22.),
            weight: FontWeight::NORMAL,
            color: TEXT_2,
            pad_x: px(8.),
            pad_y: px(0.),
            code: false,
        };
        match self {
            TextRole::Title => TextStyle {
                size: px(20.),
                line: px(TITLE_LINE),
                weight: FontWeight::SEMIBOLD,
                color: TEXT,
                pad_y: px(2.),
                ..plain
            },
            TextRole::Body => plain,
            TextRole::Heading(level) => {
                let size = HEADING_SIZES[heading_ix(level)];
                TextStyle {
                    size: px(size),
                    line: px(size * HEADING_LINES[heading_ix(level)]),
                    weight: FontWeight::SEMIBOLD,
                    ..plain
                }
            }
            TextRole::Code => TextStyle {
                size: px(CODE_SIZE),
                line: px(CODE_SIZE * CODE_LINE),
                pad_x: px(CODE_PAD_X),
                pad_y: px(CODE_PAD_Y),
                code: true,
                ..plain
            },
            TextRole::Meta => TextStyle {
                size: px(12.),
                line: px(18.),
                color: TEXT_4,
                pad_x: px(0.),
                ..plain
            },
        }
    }
}

impl TextStyle {
    /// This style around text the markdown renderer draws: a code block's box and room are the
    /// renderer's own then, so only its editor draws them.
    pub(in crate::ui) fn reading(self) -> TextStyle {
        if self.code {
            TextStyle {
                pad_x: px(0.),
                pad_y: px(0.),
                code: false,
                ..self
            }
        } else {
            self
        }
    }

    /// `el` in this style: its text, and the room around it. Plain text hangs into the margin by
    /// its room, so it lines up with what is around it; a code block keeps its room inside,
    /// as the renderer draws it.
    pub(in crate::ui) fn frame(self, el: Div, cx: &App) -> Div {
        let el = el
            .px(self.pad_x)
            .py(self.pad_y)
            .text_size(self.size)
            .line_height(self.line)
            .font_weight(self.weight)
            .text_color(color(self.color));
        if self.code {
            // On the theme's accent, which the renderer paints code on (the app's hover colour).
            el.font_family(gpui_component::Theme::global(cx).mono_font_family.clone())
                .bg(color(HOVER))
                .border_1()
                .border_color(color(BORDER))
        } else {
            el.mx(-self.pad_x)
        }
    }
}
