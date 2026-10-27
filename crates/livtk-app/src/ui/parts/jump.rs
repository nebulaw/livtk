//! A jump: a quiet round button over a long scroll, taking it to one end. Its arrow says which
//! way; a count on it says how much has arrived at that end since it was last seen; its tooltip
//! names it and its keys. The caller shows it only while that end is far off, and puts the click
//! on it.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::super::icons::{Icon, icon_hover};
use super::super::theme::*;

/// How wide a jump is.
pub(in crate::ui) const JUMP: f32 = 32.;

/// A jump. `build` gives the button to put a click on.
pub(in crate::ui) struct Jump {
    id: &'static str,
    glyph: Icon,
    tip: SharedString,
    count: usize,
}

impl Jump {
    /// The jump `id` (its debug selector too), its arrow `glyph`, named `tip`.
    pub(in crate::ui) fn new(id: &'static str, glyph: Icon, tip: impl Into<SharedString>) -> Self {
        Jump {
            id,
            glyph,
            tip: tip.into(),
            count: 0,
        }
    }

    /// How much has arrived at its end unseen: shown on it, in the accent, while more than none.
    pub(in crate::ui) fn count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }

    pub(in crate::ui) fn build(self) -> Stateful<Div> {
        let Jump {
            id,
            glyph,
            tip,
            count,
        } = self;
        // Its own hover group, named by its id: its arrow brightens with it alone.
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .group(id)
            .relative()
            .size(px(JUMP))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(color(SURFACE))
            .border_1()
            .border_color(color(BORDER_STRONG))
            .shadow_lg()
            .hover(|h| h.bg(color(HOVER)))
            .child(icon_hover(glyph, 14., TEXT_2, id, TEXT))
            .when(count > 0, |d| {
                d.child(
                    div()
                        .debug_selector(move || format!("{id}-count"))
                        .absolute()
                        .top(px(-4.))
                        .right(px(-4.))
                        .min_w(px(16.))
                        .h(px(16.))
                        .px(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(color(ACCENT))
                        .text_color(color(TEXT))
                        .text_size(px(10.))
                        .child(count.to_string()),
                )
            })
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
    }
}
