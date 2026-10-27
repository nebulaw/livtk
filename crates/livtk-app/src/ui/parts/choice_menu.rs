//! A menu that picks one: a field to type into on top (what it finds, or a new value), then its
//! choices, the chosen one ticked, the highlighted one lit for the keys. What the choices are and
//! what a pick does belong to the caller: the menu only draws them and says which was picked.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::super::theme::*;
use super::super::{check, menu_divider, menu_panel, option_row};

/// One choice: what it leads with, what it says, a few quiet words after, and whether it is the
/// one chosen.
pub(in crate::ui) struct ChoiceRow {
    pub lead: Option<AnyElement>,
    pub label: String,
    pub sub: Option<String>,
    pub on: bool,
}

/// What a pick does: given the picked row's index.
pub(in crate::ui) type OnPick = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// A menu that picks one. `build` gives its panel, to put in a popover.
pub(in crate::ui) struct ChoiceMenu {
    id: &'static str,
    width: f32,
    field: Option<AnyElement>,
    rows: Vec<ChoiceRow>,
    highlight: Option<usize>,
    divider_before: Option<usize>,
    empty: &'static str,
    on_pick: OnPick,
}

impl ChoiceMenu {
    /// The menu `id` (its debug selector; each row is `<id>-<index>`), `width` wide.
    pub(in crate::ui) fn new(id: &'static str, width: f32, on_pick: OnPick) -> Self {
        ChoiceMenu {
            id,
            width,
            field: None,
            rows: Vec::new(),
            highlight: None,
            divider_before: None,
            empty: "Nothing matches",
            on_pick,
        }
    }

    /// The field on top that finds a choice, or types a new one.
    pub(in crate::ui) fn field(mut self, field: impl IntoElement) -> Self {
        self.field = Some(field.into_any_element());
        self
    }

    pub(in crate::ui) fn rows(mut self, rows: Vec<ChoiceRow>) -> Self {
        self.rows = rows;
        self
    }

    /// The row the keys are on.
    pub(in crate::ui) fn highlight(mut self, at: Option<usize>) -> Self {
        self.highlight = at;
        self
    }

    /// A line before row `at`: the rows after it are another sort of choice.
    pub(in crate::ui) fn divider_before(mut self, at: usize) -> Self {
        self.divider_before = Some(at);
        self
    }

    /// What it says when nothing matches what was typed.
    pub(in crate::ui) fn empty(mut self, text: &'static str) -> Self {
        self.empty = text;
        self
    }

    pub(in crate::ui) fn build(self) -> Div {
        let ChoiceMenu {
            id,
            width,
            field,
            rows,
            highlight,
            divider_before,
            empty,
            on_pick,
        } = self;
        let none = rows.is_empty();
        let mut panel = menu_panel(width)
            .debug_selector(move || id.into())
            .when_some(field, |p, f| {
                p.child(
                    div()
                        .h(px(32.))
                        .px(px(6.))
                        .mb_1()
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(color(BORDER))
                        .child(f),
                )
            });
        for (i, row) in rows.into_iter().enumerate() {
            if divider_before == Some(i) {
                panel = panel.child(menu_divider());
            }
            let sel = format!("{id}-{i}");
            let pick = on_pick.clone();
            panel = panel.child(
                option_row(SharedString::from(sel.clone()))
                    .debug_selector(move || sel.clone())
                    .when(highlight == Some(i), |r| {
                        r.bg(color(SELECTED)).text_color(color(TEXT))
                    })
                    .children(row.lead)
                    .child(div().flex_1().min_w_0().truncate().child(row.label))
                    .children(row.sub.map(|s| {
                        div()
                            .flex_none()
                            .max_w(px(width * 0.45))
                            .truncate()
                            .text_xs()
                            .text_color(color(TEXT_4))
                            .child(s)
                    }))
                    .child(check(row.on))
                    .on_click(move |_, window, cx| pick(i, window, cx)),
            );
        }
        panel.when(none, |p| {
            p.child(
                div()
                    .h(px(30.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .text_color(color(TEXT_4))
                    .child(empty),
            )
        })
    }
}
