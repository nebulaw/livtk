//! The activity grid above the feed, as GitHub draws contributions: a year of days in weeks,
//! each day's colour by how much happened, with the range, who and what to count, a summary,
//! and a click on a day filtering the feed below. What it shows is `model::grid`'s.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::icons::{Icon, icon};
use super::theme::*;
use super::{LivtkApp, Menu, check, menu_panel, option_row};
use crate::model::grid::{self, Cell, Span, What, Who};

/// A day's square and the gap between squares.
const CELL: f32 = 11.;
const GAP: f32 = 3.;
/// Room at the left for the weekdays' names.
const SIDE: f32 = 28.;

impl LivtkApp {
    /// The grid's panel: a header (the summary, the range, who and what, and the fold), then,
    /// unfolded, the grid with its months, weekdays and legend.
    pub(super) fn activity_grid(&mut self, cx: &mut Context<Self>) -> Div {
        let today = grid::today();
        let g = &self.model.grid;
        let drawn = g.drawn(today);
        let folded = g.folded;
        let s = &drawn.summary;
        let summary = format!(
            "{} {} in {} · busiest {} · streak {} (longest {}) · {} done · {} decided",
            s.total,
            match g.what {
                What::All => "events",
                What::Done => "done",
                What::Decisions => "decisions",
                What::Notes => "notes",
            },
            span_label(g.span),
            s.busiest
                .map_or_else(|| "none".to_owned(), |(d, n)| format!("{} ({n})", short(d))),
            days(s.current),
            days(s.longest),
            s.done,
            s.decisions,
        );
        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(28.))
            .text_xs()
            .text_color(color(TEXT_3))
            .child(
                div()
                    .id("grid-fold")
                    .debug_selector(|| "grid-fold".into())
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .hover(|h| h.bg(color(HOVER)))
                    .child(icon(
                        if folded {
                            Icon::CaretRight
                        } else {
                            Icon::CaretDown
                        },
                        10.,
                        TEXT_4,
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.model.grid.folded = !this.model.grid.folded;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .debug_selector(|| "grid-summary".into())
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(summary),
            )
            .child(self.grid_span_button(cx))
            .child(self.grid_who(cx))
            .child(self.grid_what_button(cx));
        let mut panel = div()
            .debug_selector(|| "activity-grid".into())
            .flex_none()
            .px_4()
            .pt_2()
            .pb_1()
            .border_b_1()
            .border_color(color(BORDER))
            .flex()
            .flex_col()
            .gap_1()
            .child(header);
        if !folded {
            panel = panel.child(self.grid_body(&drawn, cx));
        }
        panel
    }

    /// The squares, with the months above, the weekdays beside and the legend below; scrolled
    /// to its latest weeks when the window is narrower than a year.
    fn grid_body(&mut self, drawn: &grid::Drawn, cx: &mut Context<Self>) -> Div {
        let span = self.model.grid.span;
        if self.grid_scrolled_for != Some(span) {
            self.grid_scroll
                .scroll_to_item(drawn.weeks.len().saturating_sub(1));
            self.grid_scrolled_for = Some(span);
        }
        let step = CELL + GAP;
        let mut months = div()
            .relative()
            .h(px(14.))
            .text_xs()
            .text_color(color(TEXT_4));
        for (w, name) in &drawn.months {
            months = months.child(div().absolute().left(px(*w as f32 * step)).child(*name));
        }
        let mut weeks = div().flex().gap(px(GAP));
        for week in &drawn.weeks {
            let mut col = div().flex().flex_col().gap(px(GAP));
            for cell in week {
                col = col.child(match cell {
                    Some(c) => self.grid_cell(c, cx).into_any_element(),
                    None => div().size(px(CELL)).into_any_element(),
                });
            }
            weeks = weeks.child(col);
        }
        let mut side = div()
            .w(px(SIDE))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .pt(px(14.))
            .text_size(px(9.))
            .text_color(color(TEXT_4));
        for i in 0..7 {
            side = side.child(
                div()
                    .h(px(CELL))
                    .flex()
                    .items_center()
                    .child(grid::weekday_name(i)),
            );
        }
        let mut legend = div()
            .flex()
            .items_center()
            .gap(px(GAP))
            .text_xs()
            .text_color(color(TEXT_4))
            .child(div().pr_1().child("Less"));
        for level in 0..=4 {
            legend = legend.child(div().size(px(CELL)).rounded_sm().bg(level_color(level)));
        }
        legend = legend.child(div().pl_1().child("More"));
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div().flex().child(side).child(
                    div()
                        .id("grid-scroll")
                        .flex_1()
                        .min_w_0()
                        .overflow_x_scroll()
                        .track_scroll(&self.grid_scroll)
                        .child(div().flex().flex_col().child(months).child(weeks)),
                ),
            )
            .child(div().flex().justify_end().child(legend))
    }

    /// One day: its square, a tooltip with what happened, a click to show it in the feed
    /// (Shift: the days from the one picked before).
    fn grid_cell(&self, c: &Cell, cx: &mut Context<Self>) -> Stateful<Div> {
        let day = c.day;
        let k = &c.counts;
        let mut tip = format!(
            "{} · {} {}",
            short(day),
            c.count,
            if c.count == 1 { "event" } else { "events" }
        );
        let parts: Vec<String> = [
            (k.created, "created"),
            (k.moves, "moves"),
            (k.done, "done"),
            (k.decisions, "decisions"),
            (k.notes, "notes"),
            (k.links, "links"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect();
        if !parts.is_empty() {
            tip.push_str(": ");
            tip.push_str(&parts.join(", "));
        }
        let tip = SharedString::from(tip);
        let name = format!("grid-{day}");
        div()
            .id(SharedString::from(name.clone()))
            .debug_selector(move || name.clone())
            .size(px(CELL))
            .rounded_sm()
            .bg(level_color(c.level))
            .when(c.picked, |d| d.border_1().border_color(color(TEXT)))
            .hover(|h| h.border_1().border_color(color(TEXT_3)))
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                this.model.grid_pick(day, e.modifiers().shift);
                cx.notify();
            }))
    }

    /// The range: the last 12 months, or a year the log has.
    fn grid_span_button(&self, cx: &mut Context<Self>) -> Div {
        let span = self.model.grid.span;
        let open = self.menu == Some(Menu::GridSpan);
        let button = grid_button("grid-span", span_label(span), open).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.toggle_menu(Menu::GridSpan, window, cx);
            }),
        );
        div().relative().child(button).when(open, |d| {
            let mut panel = menu_panel(180.).debug_selector(|| "grid-span-menu".into());
            let years = self.model.grid.years(grid::today());
            for choice in std::iter::once(Span::Recent).chain(years.into_iter().map(Span::Year)) {
                let id = format!("grid-span-{}", span_label(choice));
                panel = panel.child(
                    option_row(SharedString::from(id.clone()))
                        .debug_selector(move || id.clone())
                        .child(span_label(choice))
                        .child(check(span == choice))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.model.grid.span = choice;
                            this.model.grid_changed();
                            this.close_menu(window, cx);
                        })),
                );
            }
            d.child(self.popover(Menu::GridSpan, true, 26., panel, cx))
        })
    }

    /// Whose activity counts: All, You, Claude.
    fn grid_who(&self, cx: &mut Context<Self>) -> Div {
        let who = self.model.grid.who;
        let mut row = div()
            .flex()
            .p(px(2.))
            .gap(px(2.))
            .rounded_lg()
            .bg(color(SURFACE));
        for (choice, label) in [
            (Who::All, "All"),
            (Who::Human, "You"),
            (Who::Claude, "Claude"),
        ] {
            let id = format!("grid-who-{label}");
            let on = who == choice;
            row = row.child(
                div()
                    .id(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
                    .h(px(20.))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .when(on, |d| d.bg(color(SELECTED)).text_color(color(TEXT)))
                    .when(!on, |d| d.hover(|h| h.text_color(color(TEXT_2))))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.grid.who = choice;
                        this.model.grid_changed();
                        cx.notify();
                    })),
            );
        }
        row
    }

    /// What activity counts: all of it, work done, decisions, notes.
    fn grid_what_button(&self, cx: &mut Context<Self>) -> Div {
        let what = self.model.grid.what;
        let open = self.menu == Some(Menu::GridWhat);
        let button = grid_button("grid-what", what_label(what), open).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.toggle_menu(Menu::GridWhat, window, cx);
            }),
        );
        div().relative().child(button).when(open, |d| {
            let mut panel = menu_panel(180.).debug_selector(|| "grid-what-menu".into());
            for choice in [What::All, What::Done, What::Decisions, What::Notes] {
                let id = format!("grid-what-{}", what_label(choice));
                panel = panel.child(
                    option_row(SharedString::from(id.clone()))
                        .debug_selector(move || id.clone())
                        .child(what_label(choice))
                        .child(check(what == choice))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.model.grid.what = choice;
                            this.model.grid_changed();
                            this.close_menu(window, cx);
                        })),
                );
            }
            d.child(self.popover(Menu::GridWhat, true, 26., panel, cx))
        })
    }

    /// The days the feed is filtered to, with a way back to every day.
    pub(super) fn grid_pick_chip(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (a, b) = self.model.grid.pick?;
        let label = if a == b {
            short(a)
        } else {
            format!("{} – {}", short(a), short(b))
        };
        Some(
            div().px_4().pt_2().flex().child(
                div()
                    .id("grid-unpick")
                    .debug_selector(|| "grid-unpick".into())
                    .h(px(22.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_full()
                    .border_1()
                    .border_color(color(BORDER_STRONG))
                    .text_xs()
                    .text_color(color(TEXT_2))
                    .hover(|h| h.bg(color(HOVER)))
                    .child(label)
                    .child(icon(Icon::Close, 12., TEXT_4))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.model.grid_unpick();
                        cx.notify();
                    })),
            ),
        )
    }
}

/// A small button that opens a menu.
fn grid_button(id: &'static str, label: String, open: bool) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .h(px(22.))
        .px_2()
        .flex()
        .items_center()
        .gap_1()
        .rounded_md()
        .border_1()
        .border_color(color(BORDER))
        .when(open, |d| d.bg(color(SELECTED)))
        .when(!open, |d| d.hover(|h| h.bg(color(HOVER))))
        .child(label)
        .child(icon(Icon::CaretDown, 8., TEXT_4))
}

/// A level's colour: a faint square for nothing, then the accent, deeper with each step.
fn level_color(level: u8) -> Hsla {
    match level {
        0 => with_alpha(TEXT, 0.06),
        1 => with_alpha(ACCENT, 0.35),
        2 => with_alpha(ACCENT, 0.55),
        3 => with_alpha(ACCENT, 0.8),
        _ => color(ACCENT_HOVER),
    }
}

fn span_label(span: Span) -> String {
    match span {
        Span::Recent => "Last 12 months".to_owned(),
        Span::Year(y) => y.to_string(),
    }
}

fn what_label(what: What) -> String {
    match what {
        What::All => "All activity",
        What::Done => "Work done",
        What::Decisions => "Decisions",
        What::Notes => "Notes",
    }
    .to_owned()
}

/// "Oct 16".
fn short(d: jiff::civil::Date) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {}",
        MONTHS[usize::try_from(d.month() - 1).unwrap_or(0)],
        d.day()
    )
}

fn days(n: u64) -> String {
    if n == 1 {
        "1 day".to_owned()
    } else {
        format!("{n} days")
    }
}
