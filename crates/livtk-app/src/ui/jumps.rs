//! The jumps in a node's panel: to the latest event and back to the title, once that end is
//! more than a screen away. Something new while you read higher up never pulls you down: the
//! way down counts it instead.

use std::cell::RefCell;
use std::rc::Rc;

use super::parts::{JUMP, Jump};
use super::*;

/// How long a jump scrolls for, when motion is wanted.
const JUMP_MS: f32 = 240.;
/// Nearer an end than this is at it.
const AT_END: f32 = 8.;

/// Where the panel's scroll stands, as the probe last measured it, and a jump under way.
#[derive(Default)]
pub(super) struct JumpState {
    up: bool,
    down: bool,
    /// The node shown, and how many of its events were there when its end was last in view.
    seen: Option<(Uuid, usize)>,
    unseen: usize,
    /// A jump under way: on which node, from where, to where, since when; and where it last
    /// put the scroll, so a hand on the wheel meanwhile stops it.
    moving: Option<(Uuid, f32, f32, Instant)>,
    put: Option<f32>,
    /// Its end was in view on the last frame, how far it could scroll then, and how many events
    /// it had. Something arriving with its end in view pins it there while the content grows to
    /// hold it, as a chat follows; nothing else (an edit growing the body) moves it.
    at_end: bool,
    max: f32,
    events: usize,
    pinned: bool,
    /// The panel's own bounds, as last laid out, for the jumps to stay clear of the composer.
    view: Bounds<Pixels>,
}

/// Ease out: quick, then settling.
fn ease(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

impl LivtkApp {
    /// Measures the panel's scroll each frame: which jumps to show, what is new below; and moves
    /// a jump under way on. A change draws the panel again.
    /// While `editing` (a title or a block in place), nothing is followed: the edit's own event
    /// never moves the panel under the editor.
    pub(super) fn jump_probe(&self, node: Uuid, events: usize, editing: bool) -> impl IntoElement {
        let (state, scroll) = (self.jumps.clone(), self.detail_scroll.clone());
        canvas(
            move |_, window, _| {
                let mut s = state.borrow_mut();
                s.view = scroll.bounds();
                // A jump goes on, on its own node only, and stops at a hand on the wheel.
                if let Some((on, from, to, since)) = s.moving {
                    let moved = s
                        .put
                        .is_some_and(|y| (y + f32::from(scroll.offset().y)).abs() > 1.);
                    if on != node || moved {
                        s.moving = None;
                    } else {
                        let t = (since.elapsed().as_secs_f32() * 1000. / JUMP_MS).min(1.);
                        let y = from + (to - from) * ease(t);
                        scroll.set_offset(point(px(0.), px(-y)));
                        s.put = Some(y);
                        if t < 1. {
                            window.request_animation_frame();
                        } else {
                            s.moving = None;
                            // Down: wherever the end is now, as the content may have grown.
                            if to > from {
                                s.pinned = true;
                                s.max = f32::MIN;
                            }
                        }
                    }
                }
                let view = f32::from(scroll.bounds().size.height);
                let max = f32::from(scroll.max_offset().y);
                let same = s.seen.is_some_and(|(n, _)| n == node);
                // Only a panel long enough to scroll, read at its end: a short one, its end always
                // in view, never moves for an edit at its top.
                if same && !editing && s.at_end && s.max > AT_END && events > s.events {
                    s.pinned = true;
                }
                if s.pinned && s.moving.is_none() {
                    if max > s.max {
                        scroll.set_offset(point(px(0.), px(-max)));
                    } else {
                        // Grown to hold it: followed no more.
                        s.pinned = false;
                    }
                }
                s.max = max;
                s.events = events;
                let at = -f32::from(scroll.offset().y);
                let at_end = max - at < AT_END;
                s.at_end = at_end;
                // Seen: everything, once its end is in view; what came since, while it is not.
                let seen = match s.seen {
                    Some((n, count)) if n == node && !at_end => count.min(events),
                    _ => events,
                };
                s.seen = Some((node, seen));
                let unseen = events.saturating_sub(seen);
                let (up, down) = (at > view, max - at > view || unseen > 0);
                if (up, down, unseen) != (s.up, s.down, s.unseen) {
                    (s.up, s.down, s.unseen) = (up, down, unseen);
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }

    /// The jumps shown now, at the panel's foot, above the composer while it is stuck there.
    pub(super) fn jump_buttons(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (up, down, unseen) = {
            let s = self.jumps.borrow();
            (s.up, s.down, s.unseen)
        };
        if !up && !down {
            return None;
        }
        // Clear of the composer: above it while it is stuck at the foot, and above it in its
        // place while that is in view.
        let view = self.jumps.borrow().view;
        let in_place = self
            .composer_bounds()
            .filter(|c| c.top() < view.bottom() && c.bottom() > view.top())
            .map(|c| view.bottom() - c.top());
        let foot = self
            .stuck_foot()
            .or(in_place)
            .map_or(px(0.), |h| h + px(8.));
        Some(
            div()
                .absolute()
                .right(px(18.))
                .bottom(foot + px(14.))
                .flex()
                .flex_col()
                .gap_2()
                .when(up, |d| {
                    d.child(
                        Jump::new("jump-top", Icon::ChevronUp, "Back to the top  ⌘↑")
                            .build()
                            .on_click(cx.listener(|this, _, _, cx| this.jump(false, cx))),
                    )
                })
                .when(down, |d| {
                    d.child(
                        Jump::new("jump-latest", Icon::ChevronDown, "To the latest  ⌘↓")
                            .count(unseen)
                            .build()
                            .on_click(cx.listener(|this, _, _, cx| this.jump(true, cx))),
                    )
                })
                // Its width, for the stuck rows to leave room for.
                .w(px(JUMP)),
        )
    }

    /// Jump the open panel to its latest event (`down`) or back to its title: smoothly, unless
    /// less motion is asked for.
    pub(super) fn jump(&mut self, down: bool, cx: &mut Context<Self>) {
        if self.model.detail.is_none() {
            return;
        }
        let scroll = &self.detail_scroll;
        let from = -f32::from(scroll.offset().y);
        let to = if down {
            f32::from(scroll.max_offset().y)
        } else {
            0.
        };
        let node = self.model.detail.as_ref().map(|d| d.node.id);
        let mut s = self.jumps.borrow_mut();
        if cx.reduce_motion() {
            scroll.set_offset(point(px(0.), px(-to)));
            s.moving = None;
            // Down is to the end however long it grows, as what was just added is laid out.
            if down {
                s.pinned = true;
                s.max = f32::MIN;
            }
        } else if let Some(node) = node {
            s.moving = Some((node, from, to, Instant::now()));
            s.put = Some(from);
        }
        drop(s);
        cx.notify();
    }
}

impl LivtkApp {
    /// Everything on node `node` so far, `events` of it, seen: nothing of it counted as new.
    pub(super) fn saw(&self, node: Uuid, events: usize) {
        self.jumps.borrow_mut().seen = Some((node, events));
    }
}

/// A fresh jump state, shared with the probe.
pub(super) fn jumps() -> Rc<RefCell<JumpState>> {
    Rc::new(RefCell::new(JumpState::default()))
}
