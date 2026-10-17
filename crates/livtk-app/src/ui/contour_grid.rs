//! A year of days on a black card, after the Contour design: each day a square greyer to
//! whiter by its level, each month ringed by a fine contour (brighter while the pointer is in
//! it), today ringed white, the day under the pointer grown, and a tooltip telling of it. A
//! click anchors a day, a second closes the range, a month's name takes the month; the pick is
//! ringed white, and handed to the owner, which decides what it means.
//!
//! What it shows is laid out by `model::contour`, for any count by day. The squares and lines
//! are painted on one canvas and one pointer handler finds what is under the pointer, so
//! hovering costs a frame only when that changes. A component of its own, as the state flow is:
//! the owner keeps a `ContourState` (observed, so a hover draws again) and builds a
//! `ContourGrid` from it each frame.

use std::cell::{Cell as Shared, RefCell};
use std::rc::Rc;

use gpui_kit::*;
use jiff::civil::Date;

use super::theme::*;
use crate::model::contour::{self, Cell, Drawn, Pick};

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
const RAMP: [u32; contour::LEVELS] = [0x1a1a1a, 0x333333, 0x595959, 0x8c8c8c, 0xc2c2c2, TEXT];
/// A month's contour, and the one hovered.
const LINE: u32 = 0x3d3d3d;
const LINE_HOVER: u32 = 0xc2c2c2;
/// The tooltip's height, its room above the day, and the width it is centred in.
const TIP_H: f32 = 46.;
const TIP_LIFT: f32 = 10.;
const TIP_W: f32 = 260.;
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// What a click picked: a day (anchoring a range, or closing the one anchored), or a month,
/// first to last day (today at most).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picked {
    Day(Date),
    Month(Date, Date),
}

/// What the pointer is over in the grid.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Day(Date),
    Month(usize),
}

/// The grid as last drawn and where, for the pointer handler.
#[derive(Default)]
struct Frame {
    drawn: RefCell<Option<Rc<Drawn>>>,
    origin: Shared<Point<Pixels>>,
}

impl Frame {
    /// What is under the pointer at `at` (in the window): a day's square, its gaps around it
    /// included, or a month's name.
    fn target(&self, at: Point<Pixels>) -> Option<Target> {
        let drawn = self.drawn.borrow().clone()?;
        let o = self.origin.get();
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
}

/// What the grid remembers between frames: what the pointer is over, the day last clicked
/// (which the tooltip tells of while nothing is hovered), and its scrolling. The owner keeps
/// one, as a field owns its input state.
#[derive(Default)]
pub struct ContourState {
    hover: Option<Target>,
    clicked: Option<Date>,
    scroll: ScrollHandle,
    scrolled: bool,
    frame: Rc<Frame>,
}

type OnPick = Rc<dyn Fn(&Picked, &mut Window, &mut App)>;
type Tip = Rc<dyn Fn(&Cell) -> String>;

/// The grid, built each frame from what it shows.
#[derive(IntoElement)]
pub struct ContourGrid {
    id: SharedString,
    state: Entity<ContourState>,
    drawn: Rc<Drawn>,
    pick: Pick,
    today: Date,
    tip: Tip,
    on_pick: OnPick,
    /// Without its black card: on a surface that is already one.
    bare: bool,
}

impl ContourGrid {
    /// The grid `id` (its parts' selectors start with it) of `drawn` on `today`, `pick` ringed;
    /// `tip` says what a day's count is ("3 events"), and `on_pick` hears a click.
    pub fn new(
        id: impl Into<SharedString>,
        state: Entity<ContourState>,
        drawn: Drawn,
        pick: Pick,
        today: Date,
        tip: impl Fn(&Cell) -> String + 'static,
        on_pick: impl Fn(&Picked, &mut Window, &mut App) + 'static,
    ) -> Self {
        ContourGrid {
            id: id.into(),
            state,
            drawn: Rc::new(drawn),
            pick,
            today,
            tip: Rc::new(tip),
            on_pick: Rc::new(on_pick),
            bare: false,
        }
    }

    /// Without its black card, for a surface that is already one.
    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }

    /// The lines over the squares: each month's contour, the hovered month's brighter, the
    /// range a hovered day would close (dashed), and the pick.
    fn strokes(&self, hover: Option<Date>, hover_month: Option<usize>) -> Vec<Stroke> {
        let drawn = &self.drawn;
        let span = |a: Date, b: Date| -> Vec<Vec<(f32, f32)>> {
            match (drawn.at(a), drawn.at(b)) {
                (Some(a), Some(b)) => span_outline(a, b),
                _ => Vec::new(),
            }
        };
        let line = |path, hex: u32, width: f32, dashed: bool| Stroke {
            path,
            color: color(hex),
            width,
            dashed,
        };
        let mut strokes: Vec<Stroke> = drawn
            .months
            .iter()
            .map(|m| line(span(m.first, m.last), LINE, 1., false))
            .collect();
        if let Some(m) = hover_month.and_then(|i| drawn.months.get(i)) {
            strokes.push(line(span(m.first, m.last), LINE_HOVER, 1.5, false));
        }
        if let (Some((a, _)), true, Some(h)) = (self.pick.span, self.pick.anchored, hover)
            && h != a
        {
            strokes.push(line(span(a.min(h), a.max(h)), TEXT, 1.5, true));
        }
        if let Some((a, b)) = self.pick.span {
            strokes.push(line(span(a, b), TEXT, 2., false));
        }
        strokes
    }

    /// The tooltip over a day, centred on `x` above `y`: what it counts, and when.
    fn tip(&self, c: &Cell, x: f32, y: f32, mono: &SharedString) -> Div {
        let count = (self.tip)(c);
        let count = if c.day == self.today {
            format!("Today · {count}")
        } else {
            count
        };
        let id = format!("{}-tip", self.id);
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
                    .debug_selector(move || id.clone())
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
                            .child(day_label(c.day)),
                    ),
            )
    }
}

impl RenderOnce for ContourGrid {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // Scrolled to its latest weeks the first time, when narrower than a year.
        let (hover, clicked, scroll, frame) = self.state.update(cx, |s, _| {
            if !s.scrolled {
                s.scroll.set_offset(point(px(-1.0e6), px(0.)));
                s.scrolled = true;
            }
            (s.hover, s.clicked, s.scroll.clone(), s.frame.clone())
        });
        *frame.drawn.borrow_mut() = Some(self.drawn.clone());
        let mono = gpui_component::Theme::global(cx).mono_font_family.clone();
        let drawn = self.drawn.clone();
        let today = self.today;
        let hover_day = match hover {
            Some(Target::Day(d)) => Some(d),
            _ => None,
        };
        let hover_month = match hover {
            Some(Target::Month(i)) => Some(i),
            Some(Target::Day(d)) => drawn
                .months
                .iter()
                .position(|m| m.first <= d && d <= m.last),
            None => None,
        };
        let width = drawn.weeks.len() as f32 * P - G;

        let squares_id = format!("{}-squares", self.id);
        let mut squares = div()
            .id(SharedString::from(squares_id.clone()))
            .debug_selector(move || squares_id.clone())
            .relative()
            .w(px(width + 2. * PAD))
            .h(px(TOP + 7. * P - G + PAD))
            .on_mouse_move({
                let (state, frame) = (self.state.clone(), frame.clone());
                move |e: &MouseMoveEvent, _, cx| {
                    let now = frame.target(e.position);
                    state.update(cx, |s, cx| {
                        if s.hover != now {
                            s.hover = now;
                            cx.notify();
                        }
                    });
                }
            })
            .on_hover({
                let state = self.state.clone();
                move |on: &bool, _, cx| {
                    if !on {
                        state.update(cx, |s, cx| {
                            if s.hover.take().is_some() {
                                cx.notify();
                            }
                        });
                    }
                }
            })
            .on_mouse_down(MouseButton::Left, {
                let (state, frame, on_pick) =
                    (self.state.clone(), frame.clone(), self.on_pick.clone());
                move |e: &MouseDownEvent, window, cx| {
                    let picked = match frame.target(e.position) {
                        Some(Target::Day(day)) if day <= today => Picked::Day(day),
                        Some(Target::Month(i)) => {
                            let Some(m) = frame
                                .drawn
                                .borrow()
                                .as_ref()
                                .and_then(|d| d.months.get(i).map(|m| (m.first, m.last)))
                            else {
                                return;
                            };
                            if m.0 > today {
                                return;
                            }
                            Picked::Month(m.0, m.1.min(today))
                        }
                        _ => return,
                    };
                    state.update(cx, |s, cx| {
                        s.clicked = match picked {
                            Picked::Day(d) => Some(d),
                            Picked::Month(..) => None,
                        };
                        cx.notify();
                    });
                    on_pick(&picked, window, cx);
                }
            });
        for (i, m) in drawn.months.iter().enumerate() {
            let picked = self.pick.span == Some((m.first, m.last.min(today)));
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
        let strokes = self.strokes(hover_day, hover_month);
        squares = squares.child(paint_grid(
            drawn.clone(),
            strokes,
            hover_day,
            today,
            frame.clone(),
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
        let mut row = div().relative().flex().gap(px(SIDE_GAP)).child(side).child(
            div()
                .id(SharedString::from(format!("{}-scroll", self.id)))
                .min_w_0()
                .overflow_x_scroll()
                .track_scroll(&scroll)
                .child(squares),
        );
        // The tooltip tells of the day under the pointer, or else the day last clicked while
        // the pick it made holds.
        let clicked = clicked.filter(|d| self.pick.span.is_some_and(|(a, b)| a <= *d && *d <= b));
        if let Some(d) = hover_day.or(clicked)
            && let Some((w, r)) = drawn.at(d)
        {
            let x = SIDE + SIDE_GAP + f32::from(scroll.offset().x) + PAD + w as f32 * P + S / 2.;
            row = row.child(self.tip(&drawn.weeks[w][r], x, TOP + r as f32 * P, &mono));
        }
        let holder = div().max_w_full().min_w_0();
        let holder = if self.bare {
            holder
        } else {
            holder
                .p(px(20.))
                .rounded(px(14.))
                .bg(color(0x000000))
                .border_1()
                .border_color(color(BORDER))
        };
        div().flex().justify_center().child(holder.child(row))
    }
}

/// A line the canvas strokes: its outline, colour, width, and whether dashed.
struct Stroke {
    path: Vec<Vec<(f32, f32)>>,
    color: Hsla,
    width: f32,
    dashed: bool,
}

/// The squares, today's ring, the hovered square grown over its neighbours, then the lines;
/// noting where it was painted, for the pointer handler.
fn paint_grid(
    drawn: Rc<Drawn>,
    strokes: Vec<Stroke>,
    hover: Option<Date>,
    today: Date,
    frame: Rc<Frame>,
) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let o = bounds.origin;
            frame.origin.set(o);
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
                        gpui_kit::fill(
                            b,
                            color(RAMP[usize::from(c.level).min(contour::LEVELS - 1)]),
                        )
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

/// "Fri, 17 Oct 2026".
fn day_label(d: Date) -> String {
    let wd = WEEKDAYS[usize::from(d.weekday().to_monday_zero_offset().unsigned_abs())];
    format!("{wd}, {} {} {}", d.day(), contour::month_name(d), d.year())
}
