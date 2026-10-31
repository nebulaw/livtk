//! A date picker: a field to type a date in words, the quick picks, a month's grid, and Clear.
//! What the dates are (the grid, the picks, what typed text means) is `model::dates`; this only
//! draws them and says what was picked, or which month to show.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use jiff::civil::Date;

use crate::model::dates::{Month, Quick, Range, WeekStart, short};

use super::super::icons::{Icon, icon};
use super::super::theme::*;
use super::super::{menu_divider, menu_panel, option_row};

/// What a pick does: a date, or none (Clear).
pub(in crate::ui) type OnDate = Rc<dyn Fn(Option<Date>, &mut Window, &mut App)>;
/// What moving to another month does.
pub(in crate::ui) type OnMonth = Rc<dyn Fn(Month, &mut Window, &mut App)>;

const DAY: f32 = 30.;

/// A date picker. `build` gives its panel, to put in a popover.
pub(in crate::ui) struct DatePicker {
    id: &'static str,
    today: Date,
    month: Month,
    cursor: Option<Date>,
    chosen: Option<Date>,
    range: Range,
    start: WeekStart,
    quick: &'static [Quick],
    field: Option<AnyElement>,
    unread: Option<String>,
    on_pick: OnDate,
    on_month: OnMonth,
}

impl DatePicker {
    /// The picker `id` (its debug selector; a day is `<id>-day-<date>`, a quick pick
    /// `<id>-quick-<index>`, Clear `<id>-clear`), showing `month`.
    pub(in crate::ui) fn new(
        id: &'static str,
        today: Date,
        month: Month,
        on_pick: OnDate,
        on_month: OnMonth,
    ) -> Self {
        DatePicker {
            id,
            today,
            month,
            cursor: None,
            chosen: None,
            range: Range::default(),
            start: WeekStart::Monday,
            quick: &[],
            field: None,
            unread: None,
            on_pick,
            on_month,
        }
    }

    /// The day the keys are on.
    pub(in crate::ui) fn cursor(mut self, cursor: Option<Date>) -> Self {
        self.cursor = cursor;
        self
    }

    /// The date set now: lit, and Clear offered.
    pub(in crate::ui) fn chosen(mut self, chosen: Option<Date>) -> Self {
        self.chosen = chosen;
        self
    }

    /// The dates it may pick between; days outside are dimmed and not picked.
    pub(in crate::ui) fn range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }

    pub(in crate::ui) fn week_start(mut self, start: WeekStart) -> Self {
        self.start = start;
        self
    }

    /// The quick picks it offers, in order.
    pub(in crate::ui) fn quick(mut self, quick: &'static [Quick]) -> Self {
        self.quick = quick;
        self
    }

    /// The field to type a date in.
    pub(in crate::ui) fn field(mut self, field: impl IntoElement) -> Self {
        self.field = Some(field.into_any_element());
        self
    }

    /// What typed text could not be read as, said under the field.
    pub(in crate::ui) fn unread(mut self, unread: Option<String>) -> Self {
        self.unread = unread;
        self
    }

    pub(in crate::ui) fn build(self) -> Div {
        let DatePicker {
            id,
            today,
            month,
            cursor,
            chosen,
            range,
            start,
            quick,
            field,
            unread,
            on_pick,
            on_month,
        } = self;
        let mut panel = menu_panel(7. * DAY + 16.)
            .debug_selector(move || id.into())
            .when_some(field, |p, f| {
                p.child(
                    div()
                        .h(px(32.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(color(BORDER))
                        .child(f),
                )
            })
            .when_some(unread, |p, text| {
                p.child(
                    div()
                        .debug_selector(move || format!("{id}-unread"))
                        .px(px(6.))
                        .py_1()
                        .text_xs()
                        .text_color(color(RED))
                        .child(text),
                )
            });
        // The quick picks: what each says, and the date it is.
        for (i, q) in quick.iter().enumerate() {
            let d = q.from(today, start);
            if !range.contains(d) {
                continue;
            }
            let sel = format!("{id}-quick-{i}");
            let pick = on_pick.clone();
            panel = panel.child(
                option_row(SharedString::from(sel.clone()))
                    .debug_selector(move || sel.clone())
                    .child(div().flex_1().child(q.label()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(color(TEXT_4))
                            .child(short(d, today)),
                    )
                    .on_click(move |_, window, cx| pick(Some(d), window, cx)),
            );
        }
        if !quick.is_empty() {
            panel = panel.child(menu_divider());
        }
        // The month: its name, and the way to the one before and after.
        let step = |to: i32, glyph: Icon, name: &'static str| {
            let go = on_month.clone();
            div()
                .id(SharedString::from(format!("{id}-{name}")))
                .debug_selector(move || format!("{id}-{name}"))
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_lg()
                .hover(|h| h.bg(color(HOVER)))
                .child(icon(glyph, 14., TEXT_3))
                .on_click(move |_, window, cx| go(month.plus(to), window, cx))
        };
        panel = panel.child(
            div()
                .h(px(30.))
                .px(px(6.))
                .flex()
                .items_center()
                .child(
                    div()
                        .debug_selector(move || format!("{id}-month"))
                        .flex_1()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color(TEXT_2))
                        .child(month.title()),
                )
                .child(step(-1, Icon::ChevronUp, "prev"))
                .child(step(1, Icon::ChevronDown, "next")),
        );
        // The grid: the days' initials, then each week, the months either side left blank.
        let cell = || div().size(px(DAY)).flex().items_center().justify_center();
        panel = panel.child(
            div()
                .px(px(4.))
                .flex()
                .children(start.initials().into_iter().map(|d| {
                    cell()
                        .h(px(22.))
                        .text_xs()
                        .text_color(color(TEXT_4))
                        .child(d)
                })),
        );
        for week in month.grid(start) {
            let mut row = div().px(px(4.)).flex();
            for day in week {
                let Some(d) = day else {
                    row = row.child(cell());
                    continue;
                };
                let inside = range.contains(d);
                let sel = format!("{id}-day-{d}");
                let pick = on_pick.clone();
                let (is_chosen, is_cursor) = (chosen == Some(d), cursor == Some(d));
                row = row.child(
                    cell()
                        .id(SharedString::from(sel.clone()))
                        .debug_selector(move || sel.clone())
                        .rounded_lg()
                        .text_xs()
                        .text_color(color(if !inside {
                            BORDER_STRONG
                        } else if d == today {
                            ACCENT_HOVER
                        } else {
                            TEXT_3
                        }))
                        .when(is_cursor && !is_chosen, |c| {
                            c.bg(color(SELECTED)).text_color(color(TEXT))
                        })
                        .when(is_chosen, |c| c.bg(color(ACCENT)).text_color(color(TEXT)))
                        .when(inside && !is_chosen, |c| {
                            c.hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
                        })
                        .child(d.day().to_string())
                        .when(inside, |c| {
                            c.on_click(move |_, window, cx| pick(Some(d), window, cx))
                        }),
                );
            }
            panel = panel.child(row);
        }
        if chosen.is_some() {
            let clear = on_pick.clone();
            let sel = format!("{id}-clear");
            panel = panel.child(menu_divider()).child(
                option_row(SharedString::from(sel.clone()))
                    .debug_selector(move || sel.clone())
                    .text_color(color(TEXT_3))
                    .child("Clear the date")
                    .on_click(move |_, window, cx| clear(None, window, cx)),
            );
        }
        panel
    }
}
