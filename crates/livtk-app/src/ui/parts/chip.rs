//! A property chip, as the new-node form's row of them has it: a 24px pill with a lead (an icon
//! or a mark) and a value. Unset, it says the property's name, quiet and dashed; open, it is lit
//! while its menu shows. The caller puts the click on it.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::super::theme::*;

/// How tall a chip is.
pub(in crate::ui) const CHIP_H: f32 = 24.;

/// A property chip. `build` gives the pill to put a click on.
pub(in crate::ui) struct Chip {
    id: SharedString,
    lead: Option<AnyElement>,
    label: SharedString,
    set: bool,
    open: bool,
    still: bool,
}

impl Chip {
    /// The chip `id` (its debug selector too), saying `label`.
    pub(in crate::ui) fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Chip {
            id: id.into(),
            lead: None,
            label: label.into(),
            set: true,
            open: false,
            still: false,
        }
    }

    /// What the chip leads with: the value's mark, or the property's icon.
    pub(in crate::ui) fn lead(mut self, lead: impl IntoElement) -> Self {
        self.lead = Some(lead.into_any_element());
        self
    }

    /// Unset: its label is the property's name, quiet, and its edge dashed.
    pub(in crate::ui) fn unset(mut self, unset: bool) -> Self {
        self.set = !unset;
        self
    }

    /// Its menu shows.
    pub(in crate::ui) fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Only says something: no hover, nothing to click.
    pub(in crate::ui) fn still(mut self, still: bool) -> Self {
        self.still = still;
        self
    }

    pub(in crate::ui) fn build(self) -> Stateful<Div> {
        let Chip {
            id,
            lead,
            label,
            set,
            open,
            still,
        } = self;
        let name = id.clone();
        div()
            .id(id)
            .debug_selector(move || name.to_string())
            .h(px(CHIP_H))
            .px(px(9.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .rounded_full()
            .border_1()
            .text_xs()
            .border_color(color(BORDER_STRONG))
            .when(!set, |d| d.border_dashed().text_color(color(TEXT_4)))
            .when(set, |d| d.text_color(color(TEXT_2)))
            .when(open, |d| d.bg(color(SELECTED)).text_color(color(TEXT)))
            .when(!open && !still, |d| d.hover(|h| h.bg(color(HOVER))))
            .children(lead)
            .child(div().max_w(px(220.)).truncate().child(label))
    }
}
