//! A node's activity on a rail: every event a mark on one line, the work of each Claude session
//! stepping onto a lane of its own, small changes in a row folded, the note being written as
//! the rail's next point, and the states still ahead dashed after it. A dot travels the rail to
//! where the node is now. What goes where is `model::timeline`'s; how each kind of event looks
//! and reads is `kinds`'s; this draws them.

mod kinds;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_component::text::TextView;
use livtk_core::Event;
use uuid::Uuid;

use super::*;
use crate::model::timeline::{Item, Mark, timeline};
pub(super) use kinds::line;
use kinds::{COMPOSER, LANE_START, MARK, Stick, is_said, mark_view};

/// Where lane 0 runs in the gutter, and how far each lane sits from the one before.
const RAIL_X: f32 = 10.;
const LANE: f32 = 18.;
/// How long the dot takes to travel to a new event, at most.
const TRAVEL: Duration = Duration::from_millis(900);
/// The room between a mark and its row's text.
const TEXT_GAP: f32 = 8.;
/// How round the rail's bends are.
const BEND: f32 = 8.;
/// A stuck row's height: one line beside its mark, and room under it.
const STUCK_H: f32 = MARK + 12.;
/// The rail's dashes, ahead: dash and gap.
const DASH: f32 = 4.;
const GAP: f32 = 5.;

/// A point the rail passes, recorded as its row is laid out, in window coordinates.
#[derive(Clone, Copy, Debug)]
struct RailPoint {
    x: Pixels,
    y: Pixels,
    spot: Spot,
}

/// Where a row's mark sits on the rail: its lane, and which part of the rail.
#[derive(Clone, Copy, Debug)]
struct Spot {
    lane: usize,
    part: Part,
}

impl Spot {
    const fn on(lane: usize, part: Part) -> Spot {
        Spot { lane, part }
    }
}

/// Which part of the rail a point is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    /// What happened.
    Past,
    /// The note being written: the next point.
    Next,
    /// A state not reached yet.
    Ahead,
}

/// The rail's state between frames: the points this frame laid out, and the dot's travel.
#[derive(Default)]
pub(super) struct Rail {
    points: Vec<(usize, RailPoint)>,
    /// Each row's bounds this frame, by its place on the rail.
    rows: Vec<(usize, Bounds<Pixels>)>,
    /// Each lane's span on the rail: its first row (where it branches off) and its last.
    spans: Vec<(usize, usize)>,
    /// The composer's row, and its height as it last showed in place.
    composer: Option<usize>,
    composer_h: Pixels,
    /// What sticks now: the lanes' first rows stuck at the top, in order; the composer at the
    /// bottom. Worked out from the last frame's rows and the panel's view.
    stuck_top: Vec<usize>,
    stuck_bottom: Vec<usize>,
    composer_stuck: bool,
    /// Which item of the timeline each row is, by its place on the rail.
    items: Vec<(usize, usize)>,
    /// The node shown, and its latest event: a new one sends the dot on.
    node: Option<Uuid>,
    latest: Option<Uuid>,
    /// The dot's travel: from where along the rail, since when; at rest at the end when none.
    from: Option<Pixels>,
    since: Option<Instant>,
    /// The rail's length up to now, as last drawn.
    reached: Pixels,
}

pub(super) type RailState = Rc<RefCell<Rail>>;

impl LivtkApp {
    /// The node's activity on its rail, oldest first, the note composer at the rail's next
    /// point.
    pub(super) fn timeline_view(&mut self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let n = &d.node;
        let unfolded = &self.unfolded;
        let t = timeline(&d.history, n.kind, n.state, |k| unfolded.contains(&k));
        let now = t.now();
        // A new event since the last frame sends the dot on, from where it was.
        let latest = d.history.last().map(|e| e.id);
        let travels = {
            let mut rail = self.rail.borrow_mut();
            if rail.node != Some(n.id) {
                rail.node = Some(n.id);
                rail.latest = latest;
                rail.since = None;
                rail.from = None;
                false
            } else if rail.latest != latest {
                rail.latest = latest;
                rail.from = Some(rail.reached);
                rail.since = Some(Instant::now());
                true
            } else {
                rail.since.is_some()
            }
        };
        // Each lane's first and last row, and the composer's: what may stick.
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut open: Vec<Option<usize>> = Vec::new();
        let mut composer_at = None;
        let mut rows = div().relative().flex().flex_col().gap_3();
        // The rail, drawn first, under the marks; it reads the points the rows lay out.
        rows = rows.child(self.rail_canvas(cx));
        let mut order = 0;
        let mut items = Vec::new();
        for (k, item) in t.items.iter().enumerate() {
            items.push((order, k));
            let row = match item {
                Item::Event {
                    at,
                    mark,
                    lane,
                    opens,
                    closes,
                } => {
                    let e = &d.history[*at];
                    let new = travels && Some(e.id) == latest;
                    if *lane > 0 {
                        if open.len() < *lane {
                            open.resize(*lane, None);
                        }
                        if *opens || open[*lane - 1].is_none() {
                            open[*lane - 1] = Some(order);
                            spans.push((order, order));
                        }
                        if let Some(span) = spans
                            .iter_mut()
                            .rev()
                            .find(|s| Some(s.0) == open[*lane - 1])
                        {
                            span.1 = order;
                        }
                        if *closes {
                            open[*lane - 1] = None;
                        }
                    }
                    let content = self.event_content(d, e, *mark, cx);
                    self.rail_row(
                        order,
                        Spot::on(*lane, Part::Past),
                        mark_view(*mark, n.kind, false),
                        new,
                        content,
                    )
                }
                Item::Fold { lane, count, key } => {
                    let key = *key;
                    let label = div()
                        .id(SharedString::from(format!("timeline-fold-{key}")))
                        .debug_selector(move || format!("timeline-fold-{key}"))
                        .h(px(MARK))
                        .flex()
                        .items_center()
                        .text_xs()
                        .text_color(color(TEXT_4))
                        .hover(|h| h.text_color(color(TEXT_2)))
                        .cursor_pointer()
                        .child(format!(
                            "{count} more change{}",
                            if *count == 1 { "" } else { "s" }
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.unfolded.insert(key);
                            cx.notify();
                        }));
                    let dot = div().size(px(6.)).rounded_full().bg(color(TEXT_4));
                    self.rail_row(
                        order,
                        Spot::on(*lane, Part::Past),
                        dot,
                        false,
                        label.into_any_element(),
                    )
                }
                Item::Ahead(s) => {
                    let label = div()
                        .h(px(MARK))
                        .flex()
                        .items_center()
                        .text_color(color(TEXT_4))
                        .child(title_case(s.as_str()))
                        .into_any_element();
                    self.rail_row(
                        order,
                        Spot::on(0, Part::Ahead),
                        mark_view(Mark::Moved(*s), n.kind, true),
                        false,
                        label,
                    )
                }
            };
            // The composer: the rail's next point, after the last that happened.
            if Some(k) == now {
                rows = rows.child(row);
                order += 1;
                composer_at = Some(order);
                rows = rows.child(self.composer_row(order, cx));
            } else {
                rows = rows.child(row);
            }
            order += 1;
        }
        if now.is_none() {
            composer_at = Some(order);
            rows = rows.child(self.composer_row(order, cx));
        }
        {
            let mut rail = self.rail.borrow_mut();
            rail.spans = spans;
            rail.composer = composer_at;
            rail.items = items;
        }
        rows.debug_selector(|| "timeline".into())
    }

    /// What sticks, worked out from where the rows fell in the panel's `view` this frame: a
    /// lane's first row while its lane runs on in view, the composer while its place is below
    /// it, each at the edge `kinds` says, stacked. A change draws the panel again.
    pub(super) fn timeline_probe(&self) -> impl IntoElement {
        let rail = self.rail.clone();
        canvas(
            move |view, window, _| {
                let mut r = rail.borrow_mut();
                let row = |o: usize| r.rows.iter().find(|(k, _)| *k == o).map(|(_, b)| *b);
                // What may stick: its row, the span of rows it heads, and where it sticks.
                let mut candidates: Vec<(usize, usize, usize, Stick)> = r
                    .spans
                    .iter()
                    .map(|&(o, c)| (o, o, c, LANE_START))
                    .collect();
                if let Some(c) = r.composer {
                    candidates.push((c, 0, c, COMPOSER));
                }
                let h = px(STUCK_H);
                let (mut top, mut bottom) = (Vec::new(), Vec::new());
                let mut edge = view.top();
                for &(at, _, to, stick) in &candidates {
                    let (Some(b), Some(t)) = (row(at), row(to)) else {
                        continue;
                    };
                    match stick {
                        // Scrolled up past, what it heads still in view under the stack.
                        Stick::Top if b.top() < edge && t.bottom() > edge + h => {
                            top.push(at);
                            edge += h;
                        }
                        _ => {}
                    }
                }
                let mut edge = view.bottom();
                for &(at, from, _, stick) in candidates.iter().rev() {
                    let (Some(b), Some(f)) = (row(at), row(from)) else {
                        continue;
                    };
                    // Below the view while what it follows has begun in it, as a sticky
                    // element in its container.
                    if stick == Stick::Bottom && b.bottom() > edge && f.top() < edge {
                        bottom.push(at);
                        edge -= b.size.height.max(h);
                    }
                }
                bottom.reverse();
                let composer_stuck = r
                    .composer
                    .is_some_and(|c| top.contains(&c) || bottom.contains(&c));
                // The composer's height in place, kept while it is stuck.
                if !r.composer_stuck
                    && let Some(b) = r.composer.and_then(row)
                {
                    r.composer_h = b.size.height;
                }
                if top != r.stuck_top
                    || bottom != r.stuck_bottom
                    || composer_stuck != r.composer_stuck
                {
                    r.stuck_top = top;
                    r.stuck_bottom = bottom;
                    r.composer_stuck = composer_stuck;
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }

    /// The rows stuck to the panel's edges, over its content: at the top, the lanes whose work
    /// is in view; at the bottom, the composer while its place is below.
    pub(super) fn timeline_stuck(&mut self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let n = &d.node;
        let (top, bottom, items, composer) = {
            let r = self.rail.borrow();
            (
                r.stuck_top.clone(),
                r.stuck_bottom.clone(),
                r.items.clone(),
                r.composer,
            )
        };
        let unfolded = &self.unfolded;
        let t = timeline(&d.history, n.kind, n.state, |k| unfolded.contains(&k));
        let stuck_row = |order: usize, this: &mut Self, cx: &mut Context<Self>| -> Option<Div> {
            let (mark, content) = if Some(order) == composer {
                (
                    this.composer_mark(),
                    this.note_composer(cx).into_any_element(),
                )
            } else {
                let &(_, k) = items.iter().find(|(o, _)| *o == order)?;
                let Item::Event { at, mark, .. } = &t.items[k] else {
                    return None;
                };
                let e = &d.history[*at];
                (
                    mark_view(*mark, n.kind, false),
                    this.event_content(d, e, *mark, cx),
                )
            };
            Some(
                div()
                    .debug_selector(move || format!("timeline-stuck-{order}"))
                    .occlude()
                    .flex()
                    .items_start()
                    .gap(px(TEXT_GAP))
                    .py(px(6.))
                    .bg(color(WINDOW))
                    .child(div().flex_none().ml(px(RAIL_X - MARK / 2.)).child(mark))
                    .child(div().flex_1().min_w_0().child(content)),
            )
        };
        let tops: Vec<Div> = top.iter().filter_map(|&o| stuck_row(o, self, cx)).collect();
        let bottoms: Vec<Div> = bottom
            .iter()
            .filter_map(|&o| stuck_row(o, self, cx))
            .collect();
        // The rail runs on through what is stuck, into the rows under it: no edge between.
        let rail = |accent: bool| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(20. + RAIL_X - 0.75))
                .w(px(1.5))
                .bg(if accent {
                    with_alpha(ACCENT, 0.4)
                } else {
                    with_alpha(TEXT_4, 0.9)
                })
        };
        let (has_top, has_bottom) = (!tops.is_empty(), !bottoms.is_empty());
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .justify_between()
            .child(
                div()
                    .relative()
                    .px_5()
                    .bg(color(WINDOW))
                    .when(has_top, |d| d.child(rail(false)))
                    .children(tops),
            )
            .child(
                div()
                    .relative()
                    .px_5()
                    .pb_2()
                    .bg(color(WINDOW))
                    // Down from the rail above, in the accent, to the note being written.
                    .when(has_bottom, |d| d.child(rail(true).bottom(px(24.))))
                    .children(bottoms),
            )
    }

    /// One row of the timeline: its mark on its lane in the gutter, its content beside it. The
    /// mark's centre is recorded as the row is laid out, for the rail to pass through.
    #[allow(clippy::too_many_arguments)]
    fn rail_row(&self, order: usize, spot: Spot, mark: Div, new: bool, content: AnyElement) -> Div {
        let x = RAIL_X + LANE * spot.lane as f32;
        let rail = self.rail.clone();
        let mark = div()
            .debug_selector(move || format!("timeline-mark-{order}"))
            .absolute()
            .top_0()
            .left(px(x - MARK / 2.))
            .size(px(MARK))
            .flex()
            .items_center()
            .justify_center()
            .child(mark);
        // A new event's mark grows in where the dot is going.
        let mark = if new {
            mark.with_animation(
                SharedString::from(format!("rail-mark-{order}")),
                Animation::new(Duration::from_millis(320)).with_easing(ease_out_quint()),
                |el, t| el.opacity(t),
            )
            .into_any_element()
        } else {
            mark.into_any_element()
        };
        let geom = self.rail.clone();
        div()
            .debug_selector(move || format!("timeline-row-{order}"))
            .relative()
            .flex()
            .items_start()
            .child(
                canvas(
                    move |bounds, _, _| geom.borrow_mut().rows.push((order, bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    // Its text starts just past its own mark, on whatever lane.
                    .w(px(x + MARK / 2. + TEXT_GAP))
                    .h(px(MARK))
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                rail.borrow_mut().points.push((
                                    order,
                                    RailPoint {
                                        x: bounds.origin.x + px(x),
                                        y: bounds.origin.y + px(MARK / 2.),
                                        spot,
                                    },
                                ));
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(mark),
            )
            .child(div().flex_1().min_w_0().child(content))
    }

    /// The note composer as the rail's next point: your avatar waiting, hollow, the composer
    /// beside it.
    fn composer_row(&self, order: usize, cx: &mut Context<Self>) -> Div {
        let mark = self.composer_mark();
        // Stuck at the panel's foot, it leaves its height here, so nothing below moves.
        let (stuck, h) = {
            let rail = self.rail.borrow();
            (
                rail.composer_stuck && COMPOSER == Stick::Bottom,
                rail.composer_h,
            )
        };
        let content = if stuck {
            div().h(h).into_any_element()
        } else {
            self.note_composer(cx).into_any_element()
        };
        self.rail_row(order, Spot::on(0, Part::Next), mark, false, content)
    }

    /// The composer's mark: your avatar waiting, hollow, lit while you write.
    fn composer_mark(&self) -> Div {
        div()
            .size(px(MARK))
            .rounded_full()
            .border_1()
            .border_color(color(if self.note_focused {
                ACCENT
            } else {
                BORDER_STRONG
            }))
            .bg(color(CONTENT))
            .flex()
            .items_center()
            .justify_center()
            .child(icon(
                Icon::You,
                12.,
                if self.note_focused { TEXT } else { TEXT_4 },
            ))
    }

    /// What an event says beside its mark: who, what, when; what was said in its card; a run
    /// that waits on the human, its answer.
    fn event_content(
        &self,
        d: &Detail,
        e: &Event,
        mark: Mark,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let said = is_said(mark);
        let what = line(e, &d.node.key);
        let head = div()
            .h(px(MARK))
            .flex()
            .items_center()
            .gap_1p5()
            .min_w_0()
            .child(
                div()
                    .flex_none()
                    .text_color(color(TEXT_2))
                    .font_weight(FontWeight::MEDIUM)
                    .child(who(&e.by).to_owned()),
            )
            .when(!said, |h| {
                h.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(color(TEXT_3))
                        .child(what.clone()),
                )
            })
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(format!("· {}", ago(&e.at))),
            );
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(head)
            .when(said, |c| {
                c.child(
                    div()
                        .p_3()
                        .rounded_lg()
                        .bg(color(CONTENT))
                        .border_1()
                        .border_color(color(BORDER))
                        .text_color(color(TEXT))
                        // Markdown, as the description: emphasis, lists, links, code by language.
                        .child(TextView::markdown(
                            SharedString::from(format!("note-{}", e.id)),
                            crate::model::note_markdown(&what),
                        )),
                )
            })
            .children(self.needs_you(d, e, cx))
            .into_any_element()
    }

    /// The rail through the points the rows laid out: solid through what happened, in the
    /// accent on to the note being written, dashed through what is ahead; the dot where the
    /// node is now, travelling there when it has just moved on.
    fn rail_canvas(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (start, end) = (self.rail.clone(), self.rail.clone());
        let reduce = cx.reduce_motion();
        let writing = self.note_focused;
        canvas(
            // Laid out first of the timeline's children: this frame's points start here.
            move |_, _, _| {
                let mut rail = start.borrow_mut();
                rail.points.clear();
                rail.rows.clear();
            },
            move |_, _, window, _| {
                let mut rail = end.borrow_mut();
                let mut points = rail.points.clone();
                points.sort_by_key(|(order, _)| *order);
                let points: Vec<RailPoint> = points.into_iter().map(|(_, p)| p).collect();
                let Some(last_past) = points.iter().rposition(|p| p.spot.part == Part::Past) else {
                    return;
                };
                // The rail: one line through every mark in order, stepping out to a session's
                // lane and back as its marks come, as the branching it shows.
                let past = path_through(&points[..=last_past]);
                stroke(window, &past, with_alpha(TEXT_4, 0.9));
                // On to the note being written, in the accent; then what is ahead, dashed.
                let next = points.iter().position(|p| p.spot.part == Part::Next);
                if let Some(next) = next {
                    // Down its lane and round onto the composer's, as every bend turns: never
                    // straight across what lies between.
                    let to_next = path_through(&points[last_past..=next]);
                    stroke(
                        window,
                        &to_next,
                        with_alpha(ACCENT, if writing { 0.9 } else { 0.4 }),
                    );
                    dashed(
                        window,
                        &path_through(&points[next..]),
                        with_alpha(TEXT_4, 0.6),
                    );
                }
                // The dot travels the rail to the last that happened.
                let reached = length(&past);
                // The dot: travelling from where it was to the end of what happened.
                let along = match (rail.from, rail.since) {
                    (Some(from), Some(since)) if !reduce => {
                        let t = (since.elapsed().as_secs_f32() / TRAVEL.as_secs_f32()).min(1.);
                        if t < 1. {
                            window.request_animation_frame();
                        } else {
                            rail.since = None;
                            rail.from = None;
                        }
                        let ease = 1. - (1. - t).powi(3);
                        from.min(reached) + (reached - from.min(reached)) * ease
                    }
                    _ => {
                        rail.since = None;
                        rail.from = None;
                        reached
                    }
                };
                rail.reached = reached;
                let at = point_along(&past, along);
                let glow = px(7.);
                window.paint_quad(quad(
                    Bounds::new(point(at.x - glow, at.y - glow), size(glow * 2., glow * 2.)),
                    glow,
                    with_alpha(ACCENT, 0.3),
                    px(0.),
                    color(ACCENT),
                    BorderStyle::default(),
                ));
                let dot = px(4.);
                window.paint_quad(quad(
                    Bounds::new(point(at.x - dot, at.y - dot), size(dot * 2., dot * 2.)),
                    dot,
                    color(TEXT),
                    px(0.),
                    color(TEXT),
                    BorderStyle::default(),
                ));
            },
        )
        .absolute()
        .inset_0()
    }
}

/// The rail's polyline through `points`: straight down a lane, a slanted step between lanes,
/// as a git graph draws a branch.
fn path_through(points: &[RailPoint]) -> Vec<Point<Pixels>> {
    let mut out: Vec<Point<Pixels>> = Vec::new();
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            let a = points[i - 1];
            if a.x != p.x {
                let turn = ((p.y - a.y) / 2.).min(px(12.));
                out.push(point(a.x, p.y - turn * 2.));
                out.push(point(p.x, p.y - turn));
            }
        }
        out.push(point(p.x, p.y));
    }
    out
}

/// The point `r` from `c` toward `to`.
fn toward(c: Point<Pixels>, to: Point<Pixels>, r: Pixels) -> Point<Pixels> {
    let d = distance(c, to);
    if d == px(0.) {
        return c;
    }
    let t = r / d;
    point(c.x + (to.x - c.x) * t, c.y + (to.y - c.y) * t)
}

fn length(path: &[Point<Pixels>]) -> Pixels {
    path.windows(2)
        .map(|w| distance(w[0], w[1]))
        .fold(px(0.), |a, b| a + b)
}

fn distance(a: Point<Pixels>, b: Point<Pixels>) -> Pixels {
    let (dx, dy) = (f32::from(b.x - a.x), f32::from(b.y - a.y));
    px((dx * dx + dy * dy).sqrt())
}

/// The point `along` the path from its start.
fn point_along(path: &[Point<Pixels>], along: Pixels) -> Point<Pixels> {
    let mut left = along;
    for w in path.windows(2) {
        let d = distance(w[0], w[1]);
        if left <= d && d > px(0.) {
            let t = left / d;
            return point(
                w[0].x + (w[1].x - w[0].x) * t,
                w[0].y + (w[1].y - w[0].y) * t,
            );
        }
        left -= d;
    }
    path.last().copied().unwrap_or_default()
}

/// `path` stroked, each bend rounded.
fn stroke(window: &mut Window, path: &[Point<Pixels>], colour: Hsla) {
    if path.len() < 2 {
        return;
    }
    let mut b = PathBuilder::stroke(px(1.5));
    b.move_to(path[0]);
    for i in 1..path.len() - 1 {
        // Into the bend and out of it along each leg, as far as half the shorter leg.
        let (a, c, z) = (path[i - 1], path[i], path[i + 1]);
        let r = px(BEND).min(distance(a, c) / 2.).min(distance(c, z) / 2.);
        b.line_to(toward(c, a, r));
        b.curve_to(toward(c, z, r), c);
    }
    b.line_to(path[path.len() - 1]);
    if let Ok(p) = b.build() {
        window.paint_path(p, colour);
    }
}

/// `path` in dashes.
fn dashed(window: &mut Window, path: &[Point<Pixels>], colour: Hsla) {
    let total = length(path);
    let mut b = PathBuilder::stroke(px(1.5));
    let mut at = px(0.);
    let mut any = false;
    while at < total {
        let end = (at + px(DASH)).min(total);
        b.move_to(point_along(path, at));
        b.line_to(point_along(path, end));
        any = true;
        at = end + px(GAP);
    }
    if any && let Ok(p) = b.build() {
        window.paint_path(p, colour);
    }
}
