//! The activity grid above the feed, after the Contour design: a year of days in weeks on a
//! black card, each day a square greyer to whiter by how much happened, each month ringed by
//! a fine contour, the one under the pointer brighter. A click anchors a day, a second closes
//! the range, a month's name picks the month; the pick (ringed white) filters the feed below.
//! The squares and lines are painted on one canvas, and one pointer handler finds the day or
//! month under the pointer, so hovering costs a frame only when that changes.

use std::cell::Cell as Shared;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use jiff::civil::Date;

use super::icons::{Icon, icon};
use super::theme::*;
use super::{LivtkApp, Menu, check, menu_panel, option_row};
use crate::model::grid::{self, Cell, What, Who};

/// A day's square, the gap between squares, and the pitch from one to the next.
const S: f32 = 13.;
const G: f32 = 5.;
const P: f32 = S + G;
/// A square's corners.
const RAD: f32 = 3.;
/// Room around the squares for the contours, which run in the gaps.
const PAD: f32 = 8.;
/// The months' row above the squares, and its height with its gap.
const LABEL_H: f32 = 16.;
const TOP: f32 = LABEL_H + 12.;
/// The weekdays' column at the left, with its gap.
const SIDE: f32 = 28.;
const SIDE_GAP: f32 = 12.;
/// The squares' greys on the black card, nothing to the most.
const RAMP: [u32; grid::LEVELS] = [0x1a1a1a, 0x333333, 0x595959, 0x8c8c8c, 0xc2c2c2, TEXT];
/// A month's contour, and the one hovered.
const LINE: u32 = 0x3d3d3d;
const LINE_HOVER: u32 = 0xc2c2c2;
/// The tooltip's height, its room above the day, and the width it is centred in.
const TIP_H: f32 = 46.;
const TIP_LIFT: f32 = 10.;
const TIP_W: f32 = 260.;
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// A line the canvas strokes: its outline, colour, width, and whether dashed.
struct Stroke {
    path: Vec<Vec<(f32, f32)>>,
    color: Hsla,
    width: f32,
    dashed: bool,
}

/// What the pointer is over in the grid.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Day(Date),
    Month(usize),
}

/// The grid as last drawn and where, for the pointer handler.
#[derive(Default)]
pub(super) struct GridFrame {
    drawn: Option<Rc<grid::Drawn>>,
    origin: Rc<Shared<Point<Pixels>>>,
}

impl LivtkApp {
    /// The grid's panel: a header (the fold, who and what), then, unfolded, the grid's card.
    pub(super) fn activity_grid(&mut self, cx: &mut Context<Self>) -> Div {
        let g = &self.model.grid;
        let drawn = g.drawn(grid::today());
        let folded = g.folded;
        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(28.))
            .text_sm()
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
            .child(div().flex_1().child("Last 12 months"))
            .child(self.grid_who(cx))
            .child(self.grid_what_button(cx));
        let mut panel = div()
            .debug_selector(|| "activity-grid".into())
            .flex_none()
            .px_4()
            .pt_2()
            .pb_3()
            .border_b_1()
            .border_color(color(BORDER))
            .flex()
            .flex_col()
            .gap_2()
            .child(header);
        if !folded {
            panel = panel.child(self.grid_card(drawn, cx));
        }
        panel
    }

    /// The black card: the weekdays, the months, the squares and their contours, and the
    /// tooltip. Scrolled to its latest weeks the first time, when the window is narrower than a
    /// year.
    fn grid_card(&mut self, drawn: grid::Drawn, cx: &mut Context<Self>) -> Div {
        if !self.grid_scrolled {
            self.grid_scroll.set_offset(point(px(-1.0e6), px(0.)));
            self.grid_scrolled = true;
        }
        let drawn = Rc::new(drawn);
        self.grid_frame.drawn = Some(drawn.clone());
        let mono = gpui_component::Theme::global(cx).mono_font_family.clone();
        let today = grid::today();
        let pick = self.model.grid.pick;
        let hover = match self.grid_hover {
            Some(Target::Day(d)) => Some(d),
            _ => None,
        };
        let hover_month = match self.grid_hover {
            Some(Target::Month(i)) => Some(i),
            Some(Target::Day(d)) => drawn
                .months
                .iter()
                .position(|m| m.first <= d && d <= m.last),
            None => None,
        };
        let width = drawn.weeks.len() as f32 * P - G;

        let mut squares = div()
            .id("grid-squares")
            .debug_selector(|| "grid-squares".into())
            .relative()
            .w(px(width + 2. * PAD))
            .h(px(TOP + 7. * P - G + PAD))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                let now = this.grid_target(e.position);
                if this.grid_hover != now {
                    this.grid_hover = now;
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, on: &bool, _, cx| {
                if !on && this.grid_hover.take().is_some() {
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    match this.grid_target(e.position) {
                        Some(Target::Day(day)) if day <= grid::today() => {
                            this.model.grid_pick(day);
                            this.grid_clicked = Some(day);
                        }
                        Some(Target::Month(i)) => {
                            let Some(m) =
                                this.grid_frame.drawn.as_ref().and_then(|d| d.months.get(i))
                            else {
                                return;
                            };
                            if m.first > grid::today() {
                                return;
                            }
                            let (first, last) = (m.first, m.last);
                            this.model.grid_pick_span(first, last);
                            this.grid_clicked = None;
                        }
                        _ => return,
                    }
                    cx.notify();
                }),
            );
        for (i, m) in drawn.months.iter().enumerate() {
            let picked = pick == Some((m.first, m.last.min(today)));
            let lit = picked || hover_month == Some(i);
            squares = squares.child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(PAD + m.label_week as f32 * P))
                    .h(px(LABEL_H))
                    .whitespace_nowrap()
                    .text_size(px(11.))
                    .font_weight(if picked {
                        FontWeight::BOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(color(if lit { TEXT } else { TEXT_3 }))
                    .child(m.name),
            );
        }
        let strokes = self.grid_strokes(&drawn, hover, hover_month);
        squares = squares.child(paint_grid(
            drawn.clone(),
            strokes,
            hover,
            today,
            self.grid_frame.origin.clone(),
        ));

        let mut side = div()
            .w(px(SIDE))
            .flex_none()
            .relative()
            .mt(px(TOP))
            .h(px(7. * P - G));
        for r in [0, 2, 4, 6] {
            side = side.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(r as f32 * P))
                    .text_size(px(10.))
                    .line_height(px(13.))
                    .font_family(mono.clone())
                    .text_color(color(TEXT_3))
                    .child(WEEKDAYS[r]),
            );
        }
        let mut grid_row = div().relative().flex().gap(px(SIDE_GAP)).child(side).child(
            div()
                .id("grid-scroll")
                .min_w_0()
                .overflow_x_scroll()
                .track_scroll(&self.grid_scroll)
                .child(squares),
        );
        // The tooltip tells of the day under the pointer, or else the day last clicked while
        // the pick it made holds.
        let clicked = self
            .grid_clicked
            .filter(|d| pick.is_some_and(|(a, b)| a <= *d && *d <= b));
        if let Some(d) = hover.or(clicked)
            && let Some((w, r)) = drawn.at(d)
        {
            let c = &drawn.weeks[w][r];
            let x = SIDE
                + SIDE_GAP
                + f32::from(self.grid_scroll.offset().x)
                + PAD
                + w as f32 * P
                + S / 2.;
            grid_row = grid_row.child(self.grid_tip(c, x, TOP + r as f32 * P, today, &mono));
        }
        div().flex().justify_center().child(
            div()
                .max_w_full()
                .min_w_0()
                .p(px(20.))
                .rounded(px(14.))
                .bg(color(0x000000))
                .border_1()
                .border_color(color(BORDER))
                .child(grid_row),
        )
    }

    /// What is under the pointer at `at` (in the window): a day's square, its gaps around it
    /// included, or a month's name.
    fn grid_target(&self, at: Point<Pixels>) -> Option<Target> {
        let drawn = self.grid_frame.drawn.as_ref()?;
        let o = self.grid_frame.origin.get();
        let (x, y) = (f32::from(at.x - o.x), f32::from(at.y - o.y));
        if (0. ..LABEL_H).contains(&y) {
            let i = drawn
                .months
                .iter()
                .rposition(|m| PAD + m.label_week as f32 * P <= x)?;
            return (x < PAD + drawn.months[i].label_week as f32 * P + 2. * P)
                .then_some(Target::Month(i));
        }
        let col = ((x - PAD + G / 2.) / P).floor();
        let row = ((y - TOP + G / 2.) / P).floor();
        if col < 0. || row < 0. || row >= 7. {
            return None;
        }
        let week = drawn.weeks.get(col as usize)?;
        Some(Target::Day(week[row as usize].day))
    }

    /// The lines over the squares: each month's contour, the hovered month's brighter, the
    /// range a hovered day would close (dashed), and the pick.
    fn grid_strokes(
        &self,
        drawn: &grid::Drawn,
        hover: Option<Date>,
        hover_month: Option<usize>,
    ) -> Vec<Stroke> {
        let g = &self.model.grid;
        let span = |a: Date, b: Date| -> Vec<Vec<(f32, f32)>> {
            match (drawn.at(a), drawn.at(b)) {
                (Some(a), Some(b)) => span_outline(a, b),
                _ => Vec::new(),
            }
        };
        let mut strokes: Vec<Stroke> = drawn
            .months
            .iter()
            .map(|m| Stroke {
                path: span(m.first, m.last),
                color: color(LINE),
                width: 1.,
                dashed: false,
            })
            .collect();
        if let Some(m) = hover_month.and_then(|i| drawn.months.get(i)) {
            strokes.push(Stroke {
                path: span(m.first, m.last),
                color: color(LINE_HOVER),
                width: 1.5,
                dashed: false,
            });
        }
        if let (Some((a, _)), true, Some(h)) = (g.pick, g.anchored, hover)
            && h != a
        {
            strokes.push(Stroke {
                path: span(a.min(h), a.max(h)),
                color: color(TEXT),
                width: 1.5,
                dashed: true,
            });
        }
        if let Some((a, b)) = g.pick {
            strokes.push(Stroke {
                path: span(a, b),
                color: color(TEXT),
                width: 2.,
                dashed: false,
            });
        }
        strokes
    }

    /// The tooltip over a day: what happened, and when.
    fn grid_tip(&self, c: &Cell, x: f32, y: f32, today: Date, mono: &SharedString) -> Div {
        let count = count_label(c, self.model.grid.what);
        let count = if c.day == today {
            format!("Today · {count}")
        } else {
            count
        };
        div()
            .absolute()
            .left(px(x - TIP_W / 2.))
            .top(px(y - TIP_LIFT - TIP_H))
            .w(px(TIP_W))
            .flex()
            .flex_col()
            .items_center()
            .child(
                // As the menus are drawn: a raised surface, a hairline, a shadow.
                div()
                    .debug_selector(|| "grid-tip".into())
                    .h(px(TIP_H))
                    .px(px(12.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(2.))
                    .rounded_lg()
                    .bg(color(SURFACE))
                    .border_1()
                    .border_color(color(BORDER_STRONG))
                    .shadow_lg()
                    .whitespace_nowrap()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(TEXT))
                            .child(count),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_family(mono.clone())
                            .text_color(color(TEXT_3))
                            .child(day_label(c.day, true)),
                    ),
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

/// The squares, today's ring, the hovered square grown over its neighbours, then the lines;
/// noting where it was painted, for the pointer handler.
fn paint_grid(
    drawn: Rc<grid::Drawn>,
    strokes: Vec<Stroke>,
    hover: Option<Date>,
    today: Date,
    origin: Rc<Shared<Point<Pixels>>>,
) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let o = bounds.origin;
            origin.set(o);
            let square = |w: usize, r: usize, grow: f32| {
                Bounds::new(
                    o + point(px(PAD + w as f32 * P - grow), px(TOP + r as f32 * P - grow)),
                    size(px(S + 2. * grow), px(S + 2. * grow)),
                )
            };
            let paint = |window: &mut Window, b: Bounds<Pixels>, c: &Cell| {
                if c.future {
                    window.paint_quad(quad(
                        b,
                        px(RAD),
                        color(0x000000),
                        px(1.),
                        color(LINE),
                        BorderStyle::Solid,
                    ));
                } else {
                    window.paint_quad(
                        gpui_kit::fill(b, color(RAMP[usize::from(c.level).min(grid::LEVELS - 1)]))
                            .corner_radii(px(RAD)),
                    );
                }
            };
            for (w, week) in drawn.weeks.iter().enumerate() {
                for (r, c) in week.iter().enumerate() {
                    paint(window, square(w, r, 0.), c);
                    if c.day == today {
                        window.paint_quad(quad(
                            square(w, r, 3.5),
                            px(RAD + 3.5),
                            transparent_black(),
                            px(1.5),
                            color(TEXT),
                            BorderStyle::Solid,
                        ));
                    }
                }
            }
            if let Some((w, r)) = hover.and_then(|d| drawn.at(d)) {
                paint(window, square(w, r, S * 0.15), &drawn.weeks[w][r]);
            }
            for s in &strokes {
                for shape in &s.path {
                    let mut b = PathBuilder::stroke(px(s.width));
                    if s.dashed {
                        b = b.dash_array(&[px(3.), px(3.)]);
                    }
                    rounded(&mut b, o, shape, 4.);
                    if let Ok(path) = b.build() {
                        window.paint_path(path, s.color);
                    }
                }
            }
        },
    )
    .absolute()
    .inset_0()
}

/// The outline around the days from `a` to `b` (each a week and a weekday), in the squares'
/// units: one shape, or two when the days only touch at a week's turn.
fn span_outline((c0, r0): (usize, usize), (c1, r1): (usize, usize)) -> Vec<Vec<(f32, f32)>> {
    let (c0, r0, c1, r1) = (c0 as f32, r0 as f32, c1 as f32, r1 as f32);
    let shapes = if c0 == c1 {
        vec![vec![
            (c0, r0),
            (c0 + 1., r0),
            (c0 + 1., r1 + 1.),
            (c0, r1 + 1.),
        ]]
    } else if c1 == c0 + 1. && r1 < r0 {
        vec![
            vec![(c0, r0), (c0 + 1., r0), (c0 + 1., 7.), (c0, 7.)],
            vec![(c1, 0.), (c1 + 1., 0.), (c1 + 1., r1 + 1.), (c1, r1 + 1.)],
        ]
    } else {
        let mut p = Vec::new();
        if r0 > 0. {
            p.extend([(c0, r0), (c0 + 1., r0), (c0 + 1., 0.)]);
        } else {
            p.push((c0, 0.));
        }
        p.push((c1 + 1., 0.));
        if r1 < 6. {
            p.extend([(c1 + 1., r1 + 1.), (c1, r1 + 1.), (c1, 7.)]);
        } else {
            p.push((c1 + 1., 7.));
        }
        p.push((c0, 7.));
        vec![p]
    };
    // Into the squares' gaps.
    shapes
        .into_iter()
        .map(|s| {
            s.into_iter()
                .map(|(c, r)| (PAD + c * P - G / 2., TOP + r * P - G / 2.))
                .collect()
        })
        .collect()
}

/// A closed shape through `pts`, each corner rounded by up to `rad`.
fn rounded(b: &mut PathBuilder, o: Point<Pixels>, pts: &[(f32, f32)], rad: f32) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    let at = |(x, y): (f32, f32)| o + point(px(x), px(y));
    let toward = |c: (f32, f32), to: (f32, f32)| {
        let d = ((to.0 - c.0).powi(2) + (to.1 - c.1).powi(2)).sqrt();
        let r = rad.min(d / 2.);
        if d == 0. {
            c
        } else {
            (c.0 + (to.0 - c.0) / d * r, c.1 + (to.1 - c.1) / d * r)
        }
    };
    let before: Vec<_> = (0..n)
        .map(|i| toward(pts[i], pts[(i + n - 1) % n]))
        .collect();
    let after: Vec<_> = (0..n).map(|i| toward(pts[i], pts[(i + 1) % n])).collect();
    b.move_to(at(before[0]));
    for j in 0..n {
        b.curve_to(at(after[j]), at(pts[j]));
        b.line_to(at(before[(j + 1) % n]));
    }
    b.close();
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

/// "Fri, 17 Oct", with its year if asked: "Fri, 17 Oct 2026".
fn day_label(d: Date, year: bool) -> String {
    let wd = WEEKDAYS[usize::from(d.weekday().to_monday_zero_offset().unsigned_abs())];
    let s = format!("{wd}, {} {}", d.day(), grid::month_name(d));
    if year { format!("{s} {}", d.year()) } else { s }
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
