//! The activity grid in the bottom bar: a year of what happened in the project, drawn by
//! `contour_grid`, with who and what to count; the days picked in it filter the Activity feed.
//! What it counts is `model::grid`'s.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::contour_grid::{ContourGrid, Picked};
use super::icons::{Icon, icon};
use super::theme::*;
use super::{LivtkApp, Menu, check, menu_panel, option_row};
use crate::model::contour::Cell;
use crate::model::grid::{self, What, Who};

impl LivtkApp {
    /// The grid in the bottom bar: a header (what it shows, who and what to count), then the
    /// grid itself, bare on the bar's black.
    pub(super) fn activity_grid(&mut self, cx: &mut Context<Self>) -> Div {
        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(28.))
            .text_sm()
            .text_color(color(TEXT_3))
            .child(div().flex_1().child("Activity · last 12 months"))
            .child(self.grid_who(cx))
            .child(self.grid_what_button(cx));
        let today = grid::today();
        let g = &self.model.grid;
        let what = g.what;
        div()
            .debug_selector(|| "activity-grid".into())
            .size_full()
            .px_4()
            .pt_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(header)
            .child(
                ContourGrid::new(
                    "grid",
                    self.grid_state.clone(),
                    g.drawn(today),
                    g.pick,
                    today,
                    move |c| count_label(c, what),
                    cx.listener(|this, picked: &Picked, _, cx| {
                        match *picked {
                            Picked::Day(day) => this.model.grid_pick(day),
                            Picked::Month(first, last) => this.model.grid_pick_span(first, last),
                        }
                        cx.notify();
                    }),
                )
                .bare(),
            )
    }

    /// Whose activity counts: All, You, Claude.
    fn grid_who(&self, cx: &mut Context<Self>) -> Div {
        let who = self.model.grid.who;
        let mut row = div()
            .flex()
            .p(px(2.))
            .gap(px(2.))
            .rounded_lg()
            .bg(color(SURFACE))
            .border_1()
            .border_color(color(BORDER));
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
            d.child(self.popover_up(Menu::GridWhat, true, panel, cx))
        })
    }

    /// The days the feed is filtered to, with a way back to every day.
    pub(super) fn grid_pick_chip(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (a, b) = self.model.grid.pick.span?;
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
                    .h(px(20.))
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
        .h(px(24.))
        .px_2()
        .flex()
        .items_center()
        .gap_1()
        .rounded_lg()
        .border_1()
        .when(open, |d| {
            d.bg(color(SELECTED)).border_color(color(BORDER_STRONG))
        })
        .when(!open, |d| {
            d.border_color(color(BORDER)).hover(|h| h.bg(color(HOVER)))
        })
        .child(label)
        .child(icon(Icon::CaretDown, 10., TEXT_4))
}

/// What a day's count says, in the grid's what: "No activity", "1 event", "4 decisions".
fn count_label(c: &Cell, what: What) -> String {
    if c.future {
        return "Not yet".to_owned();
    }
    if c.count == 0 {
        return "No activity".to_owned();
    }
    let (one, many) = match what {
        What::All => ("event", "events"),
        What::Done => ("done", "done"),
        What::Decisions => ("decision", "decisions"),
        What::Notes => ("note", "notes"),
    };
    format!("{} {}", c.count, if c.count == 1 { one } else { many })
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
