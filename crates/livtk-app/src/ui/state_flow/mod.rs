//! A node's states as a flow, drawn as light on the dark: the kind's main path left to right,
//! the states off it on a line below, joined by fine threads. Where the node is now is the
//! larger, ringed in its state's colour, with a soft glow of it around (the circle itself is
//! filled with the background). The threads that touch it, and those down to the line below,
//! are two strands twisting past each other; the rest of the path is a straight hairline. A
//! thread it can move along is lit (in the accent for the human's decision); the rest is faint.
//!
//! A click on a reachable state moves there: it grows smoothly into the current state, its glow
//! coming up, while the one it leaves settles back to the others' size. The twisting threads
//! breathe, so slightly it is felt more than seen; light drifts along the lit ones while the
//! pointer is over the flow; with the system's Reduce Motion on, nothing moves.
//!
//! A component of its own, with a fixed height, so the panel never moves as the state changes.
//! Where things go is `geometry`'s, plain arithmetic tested as such.

pub mod geometry;

use std::f32::consts::PI;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use livtk_core::{Kind, State};
use uuid::Uuid;

use self::geometry::{CURRENT, Glow, HEIGHT, Layout, NODE};
use super::theme::*;
use crate::model::{Arrow, Choice, Look, Step, state_short};

/// A state becoming the current one grows into it, and the one it leaves settles back, in
/// this time.
const GROW: Duration = Duration::from_millis(420);
/// The threads' slow life, in one cycle: the twisting strands breathe twice in it (barely: a
/// fraction of a pixel either way), and light drifts the length of a lit thread five times
/// while the pointer is over the flow.
const LIFE: Duration = Duration::from_millis(12_000);
const BREATHS: f32 = 2.;
const DRIFTS: f32 = 5.;
/// How far the strands breathe, as a share of their twist.
const BREATH: f32 = 0.22;
/// A label's room, centred under its state: wider than the label, which is centred in it.
const LABEL_ROOM: f32 = 80.;

type OnChoose = Rc<dyn Fn(&Choice, &mut Window, &mut App)>;
type OnStanding = Rc<dyn Fn(&mut Window, &mut App)>;

/// What the flow remembers between frames: the state it last showed (so a change transitions)
/// and whether the pointer is over it. The panel owns one, as a field owns its input state.
#[derive(Default)]
pub struct FlowState {
    shown: Option<(Uuid, State)>,
    change: Option<Change>,
    hovered: bool,
    /// The reachable state under the pointer, its highlight coming up, and the one the pointer
    /// just left, its highlight going: each with the number of its transition.
    lighting: Option<(State, usize)>,
    dimming: Option<(State, usize)>,
    epoch: usize,
}

/// The node moved from `was`: the transition numbered `epoch` plays.
#[derive(Clone, Copy, Debug)]
struct Change {
    was: State,
    epoch: usize,
}

impl FlowState {
    /// The pointer came over reachable state `state`, or left it.
    fn hover(&mut self, state: State, over: bool) {
        self.epoch += 1;
        if over {
            if let Some((was, _)) = self.lighting.filter(|(s, _)| *s != state) {
                self.dimming = Some((was, self.epoch));
                self.epoch += 1;
            }
            self.lighting = Some((state, self.epoch));
        } else if self.lighting.is_some_and(|(s, _)| s == state) {
            self.lighting = None;
            self.dimming = Some((state, self.epoch));
        }
    }

    /// The node is in `state` (or the panel shows another node): a change of state transitions,
    /// another node shows as it is.
    fn see(&mut self, node: Uuid, state: State) {
        match self.shown {
            Some((n, was)) if n == node && was != state => {
                self.epoch += 1;
                self.change = Some(Change {
                    was,
                    epoch: self.epoch,
                });
            }
            Some((n, _)) if n != node => {
                self.change = None;
                self.lighting = None;
                self.dimming = None;
            }
            _ => {}
        }
        self.shown = Some((node, state));
    }
}

/// The flow of `kind`'s states for node `id`, `width` wide: its steps and the main path's
/// arrows (as `steps_with` and `arrows_with` give them), what a move does, and for a lesson's
/// ratifying, the standing-rule menu.
#[derive(IntoElement)]
pub struct StateFlow {
    id: Uuid,
    kind: Kind,
    steps: Vec<Step>,
    arrows: Vec<Arrow>,
    width: f32,
    state: Entity<FlowState>,
    on_choose: OnChoose,
    on_standing: Option<OnStanding>,
    standing_menu: Option<AnyElement>,
}

impl StateFlow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Uuid,
        kind: Kind,
        steps: Vec<Step>,
        arrows: Vec<Arrow>,
        width: f32,
        state: Entity<FlowState>,
        on_choose: impl Fn(&Choice, &mut Window, &mut App) + 'static,
    ) -> Self {
        StateFlow {
            id,
            kind,
            steps,
            arrows,
            width,
            state,
            on_choose: Rc::new(on_choose),
            on_standing: None,
            standing_menu: None,
        }
    }

    /// What a move to a state that offers the standing rule does: open its menu.
    pub fn on_standing(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_standing = Some(Rc::new(f));
        self
    }

    /// The standing-rule menu, drawn under the state that offers it while it is open.
    pub fn standing_menu(mut self, menu: Option<AnyElement>) -> Self {
        self.standing_menu = menu;
        self
    }
}

/// What every handler of one frame shares: the steps, where they are, what a move does.
struct Flow {
    steps: Vec<Step>,
    layout: Layout,
    state: Entity<FlowState>,
    on_choose: OnChoose,
    on_standing: Option<OnStanding>,
}

impl Flow {
    /// Move to state `i`, or for a state that offers the standing rule, open its menu.
    fn request(&self, i: usize, window: &mut Window, cx: &mut App) {
        let s = &self.steps[i];
        if s.standing.is_some() {
            if let Some(open) = &self.on_standing {
                open(window, cx);
            }
            return;
        }
        if let Some(choice) = &s.to {
            (self.on_choose)(choice, window, cx);
        }
    }
}

impl RenderOnce for StateFlow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let StateFlow {
            id,
            kind,
            steps,
            arrows,
            width,
            state,
            on_choose,
            on_standing,
            mut standing_menu,
        } = self;
        if let Some(s) = steps.iter().find(|s| s.current) {
            let now = s.state;
            state.update(cx, |st, _| st.see(id, now));
        }
        let flow = Rc::new(Flow {
            layout: geometry::layout(&steps, &arrows, width),
            steps,
            state: state.clone(),
            on_choose,
            on_standing,
        });
        let st = state.read(cx);
        let change = st.change;
        let lighting = st.lighting;
        let dimming = st.dimming;
        let drifting = st.hovered && !cx.reduce_motion();

        let mut band = div()
            .id(SharedString::from(format!("state-flow-{id}")))
            .debug_selector(|| "state-flow".into())
            .relative()
            .w(px(width))
            .h(px(HEIGHT))
            .flex_none()
            .on_hover({
                let state = state.clone();
                move |hovered, _, cx| {
                    state.update(cx, |st, cx| {
                        st.hovered = *hovered;
                        cx.notify();
                    })
                }
            })
            .child(threads(&flow, !cx.reduce_motion(), drifting));
        for (i, n) in flow.layout.nodes.iter().enumerate() {
            let s = &flow.steps[i];
            band = band.child(label(s, n));
            let light = Light {
                change,
                lighting: lighting.filter(|(st, _)| *st == s.state).map(|(_, e)| e),
                dimming: dimming.filter(|(st, _)| *st == s.state).map(|(_, e)| e),
            };
            // The circle is drawn, and a still target over it takes the pointer: hover state
            // lives by an element's id, which an animation keyed by its run would change.
            band = band
                .child(node(kind, s, n, light))
                .child(target(i, s, n, &flow));
            if s.standing.is_some()
                && let Some(menu) = standing_menu.take()
            {
                band = band.child(anchor(n).child(menu));
            }
        }
        band
    }
}

/// The threads, lit as their moves are. While `alive` (Reduce Motion off) the twisting ones
/// breathe, so slightly it is felt more than seen; while `drifting` (the pointer over the flow)
/// light drifts along the lit ones too. A low frame rate is plenty for so slow a movement.
fn threads(flow: &Rc<Flow>, alive: bool, drifting: bool) -> AnyElement {
    let paint = {
        let flow = flow.clone();
        move |phase: Option<f32>| {
            let flow = flow.clone();
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    for (k, t) in flow.layout.threads.iter().enumerate() {
                        // Each thread breathes a little out of step with the next.
                        let sway = phase.map_or(1., |p| {
                            1. + BREATH * (2. * PI * (BREATHS * p + k as f32 * 0.17)).sin()
                        });
                        let drift = phase.filter(|_| drifting).map(|p| (DRIFTS * p).fract());
                        paint_thread(window, bounds.origin, &flow.layout, t, sway, drift);
                    }
                },
            )
            .absolute()
            .inset_0()
        }
    };
    if alive {
        div()
            .absolute()
            .inset_0()
            .when(drifting, |d| d.debug_selector(|| "flow-drift".into()))
            .with_animation(
                "flow-life",
                Animation::new(LIFE)
                    .repeat()
                    .with_max_fps(if drifting { 40. } else { 24. }),
                move |el, p| el.child(paint(Some(p))),
            )
            .into_any_element()
    } else {
        div()
            .absolute()
            .inset_0()
            .child(paint(None))
            .into_any_element()
    }
}

/// One thread: two strands twisting past each other (bent `sway` times their twist), or a
/// straight line along the path away from the current state; glowing when lit, and a mote of
/// light drifting along it at `drift` of the way.
fn paint_thread(
    window: &mut Window,
    origin: gpui_kit::Point<Pixels>,
    layout: &Layout,
    t: &geometry::Thread,
    sway: f32,
    drift: Option<f32>,
) {
    let at = |p: geometry::Point| origin + point(px(p.x), px(p.y));
    let hue = match t.glow {
        Glow::Decision => ACCENT_HOVER,
        Glow::Move | Glow::Idle => TEXT,
    };
    let lit = t.glow != Glow::Idle;
    let stroke = |window: &mut Window, c: &[geometry::Point; 4], width: f32, alpha: f32| {
        let mut b = PathBuilder::stroke(px(width));
        b.move_to(at(c[0]));
        b.cubic_bezier_to(at(c[3]), at(c[1]), at(c[2]));
        if let Ok(path) = b.build() {
            window.paint_path(path, with_alpha(hue, alpha.min(1.)));
        }
    };
    // The second strand breathes against the first.
    let a = geometry::curve(layout, t, sway);
    let b = t.twist.then(|| geometry::curve(layout, t, -(2. - sway)));
    if !lit {
        stroke(window, &a, 0.75, 0.16);
        if let Some(b) = &b {
            stroke(window, b, 0.6, 0.1);
        }
        return;
    }
    // A soft glow under the strands: GPUI blurs no path, so wider, fainter strokes stand in.
    stroke(window, &a, 5., 0.035);
    stroke(window, &a, 2.5, 0.08);
    stroke(window, &a, 1., 0.55);
    if let Some(b) = &b {
        stroke(window, b, 0.75, 0.35);
    }
    if let Some(d) = drift {
        let p = geometry::bezier(&a, d);
        // Fading in and out at the ends of its way.
        let fade = (d * PI).sin();
        for (r, alpha) in [(1.5, 0.9), (3.5, 0.22), (7., 0.07)] {
            let bounds = Bounds::new(
                at(geometry::Point::new(p.x - r, p.y - r)),
                size(px(2. * r), px(2. * r)),
            );
            window.paint_quad(fill(bounds, with_alpha(hue, alpha * fade)).corner_radii(px(r)));
        }
    }
}

/// A state's node: its mark in a circle. The current state's is the larger, filled faintly in
/// its colour with a soft glow of it; a reachable one is ringed (in the accent for the human's
/// decision) and moves there on a click; the rest are ghosts. When the node has just moved, the
/// new state grows into its place and the old one settles back.
fn node(kind: Kind, s: &Step, n: &geometry::Node, light: Light) -> AnyElement {
    let change = light.change;
    let name = s.state.as_str();
    let c = state_color(s.state);
    let at = n.at;
    // The node at `size` across, `lit` (0 to 1) of the way to being the current one.
    let shape = move |el: Stateful<Div>, size: f32, lit: f32| {
        el.left(px(at.x - size / 2.))
            .top(px(at.y - size / 2.))
            .size(px(size))
            .when(lit > 0., |el| {
                // Filled with the background, so its glow shows only around it.
                el.bg(color(WINDOW))
                    .border_color(with_alpha(c, 0.55 * lit))
                    .shadow(vec![BoxShadow {
                        color: with_alpha(c, 0.38 * lit),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(10.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
            })
    };
    let mut el = div()
        .id(SharedString::from(format!("flow-circle-{name}")))
        .absolute()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .child(state_mark(kind, s.state));
    let look = s.look();
    if look == Look::Out {
        el = el
            .border_color(with_alpha(TEXT_4, 0.35))
            .bg(color(WINDOW))
            .opacity(0.45);
    }
    if matches!(look, Look::Decision | Look::Move) {
        return reachable_node(el, look, shape, light);
    }
    let left = change.filter(|ch| ch.was == s.state && !s.current);
    match (s.current, change, left) {
        // Just became the current state: it grows into it, its glow coming up.
        (true, Some(ch), _) => el
            .with_animation(
                SharedString::from(format!("flow-grow-{}", ch.epoch)),
                Animation::new(GROW).with_easing(ease_out_quint()),
                move |el, t| shape(el, NODE + (CURRENT - NODE) * t, t),
            )
            .into_any_element(),
        // The state it just left settles back to the others' size, its glow going.
        (false, _, Some(ch)) => el
            .with_animation(
                SharedString::from(format!("flow-leave-{}", ch.epoch)),
                Animation::new(GROW).with_easing(ease_out_quint()),
                move |el, t| shape(el, CURRENT + (NODE - CURRENT) * t, 1. - t),
            )
            .into_any_element(),
        (true, None, _) => shape(el, CURRENT, 1.).into_any_element(),
        (false, ..) => shape(el, NODE, 0.).into_any_element(),
    }
}

/// What the pointer meets over a state's circle, still whatever the circle does: its tooltip,
/// a click that moves there, the hover that lights a reachable one.
fn target(i: usize, s: &Step, n: &geometry::Node, flow: &Rc<Flow>) -> impl IntoElement {
    let id = format!("flow-{}", s.state.as_str());
    let half = n.size / 2.;
    let tip: SharedString = match (&s.to, s.current) {
        (_, true) => "Where it is now".into(),
        (Some(c), _) => c.label.into(),
        (None, false) if s.standing.is_some() => "Ratify".into(),
        (None, false) => "Not a move from here".into(),
    };
    let mut el = div()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || id.clone())
        .absolute()
        .left(px(n.at.x - half))
        .top(px(n.at.y - half))
        .size(px(n.size))
        .rounded_full()
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        });
    if !s.current && (s.to.is_some() || s.standing.is_some()) {
        let state = flow.state.clone();
        let me = s.state;
        let on_click = flow.clone();
        el = el
            .cursor_pointer()
            .on_click(move |_, window, cx| on_click.request(i, window, cx))
            .on_hover(move |over, _, cx| {
                state.update(cx, |st, cx| {
                    st.hover(me, *over);
                    cx.notify();
                })
            });
    }
    el
}

/// How a reachable state's circle is lit, and what is changing it: a move just made (its ring
/// fades in), the pointer coming over it (its highlight comes up) or leaving it (it goes).
#[derive(Clone, Copy)]
struct Light {
    change: Option<Change>,
    lighting: Option<usize>,
    dimming: Option<usize>,
}

/// A highlight fades in and out in this time, and a new ring in in this.
const HIGHLIGHT: Duration = Duration::from_millis(160);
const RING_IN: Duration = Duration::from_millis(300);

/// A reachable state's circle: ringed (in the accent for the human's decision), filled with the
/// background, its highlight fading in under the pointer and out after it, and after a move its
/// ring fading in.
fn reachable_node(
    el: Stateful<Div>,
    look: Look,
    shape: impl Fn(Stateful<Div>, f32, f32) -> Stateful<Div> + 'static,
    light: Light,
) -> AnyElement {
    let decision = look == Look::Decision;
    // `ring` (0 to 1) of its ring, `lit` (0 to 1) of its highlight.
    let paint = move |el: Stateful<Div>, ring: f32, lit: f32| {
        let (rest, high, fill) = if decision {
            (
                with_alpha(ACCENT_HOVER, 0.75),
                with_alpha(ACCENT_HOVER, 1.),
                with_alpha(ACCENT, 0.18 * lit),
            )
        } else {
            (
                with_alpha(TEXT_3, 0.8),
                with_alpha(TEXT, 1.),
                with_alpha(HOVER, lit),
            )
        };
        let border = rest.blend(Hsla { a: lit, ..high });
        shape(el, NODE, 0.)
            .bg(color(WINDOW).blend(fill))
            .border_color(Hsla {
                a: border.a * ring,
                ..border
            })
    };
    match (light.lighting, light.dimming, light.change) {
        (Some(e), ..) => el
            .with_animation(
                SharedString::from(format!("flow-light-{e}")),
                Animation::new(HIGHLIGHT).with_easing(ease_in_out),
                move |el, t| paint(el, 1., t),
            )
            .into_any_element(),
        (None, Some(e), _) => el
            .with_animation(
                SharedString::from(format!("flow-dim-{e}")),
                Animation::new(HIGHLIGHT).with_easing(ease_in_out),
                move |el, t| paint(el, 1., 1. - t),
            )
            .into_any_element(),
        (None, None, Some(ch)) => el
            .with_animation(
                SharedString::from(format!("flow-ring-{}", ch.epoch)),
                Animation::new(RING_IN).with_easing(ease_out_quint()),
                move |el, t| paint(el, t, 0.),
            )
            .into_any_element(),
        (None, None, None) => paint(el, 1., 0.).into_any_element(),
    }
}

/// A state's name, centred under its node.
fn label(s: &Step, n: &geometry::Node) -> impl IntoElement {
    let id = format!("flow-label-{}", s.state.as_str());
    let (c, weight) = match s.look() {
        Look::Current => (TEXT, FontWeight::SEMIBOLD),
        Look::Decision => (ACCENT_HOVER, FontWeight::NORMAL),
        Look::Move => (TEXT_2, FontWeight::NORMAL),
        Look::Out => (TEXT_4, FontWeight::NORMAL),
    };
    div()
        .absolute()
        .left(px(n.at.x - LABEL_ROOM / 2.))
        .top(px(n.at.y + CURRENT / 2. + 4.))
        .w(px(LABEL_ROOM))
        .flex()
        .justify_center()
        .child(
            div()
                .debug_selector(move || id.clone())
                .text_xs()
                .whitespace_nowrap()
                .text_color(color(c))
                .font_weight(weight)
                .child(state_short(s.state)),
        )
}

/// A point to hang a menu from: the top left of node `n`'s circle, its size.
fn anchor(n: &geometry::Node) -> Div {
    let half = n.size / 2.;
    div()
        .absolute()
        .left(px(n.at.x - half))
        .top(px(n.at.y - half))
        .size(px(n.size))
}
