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
    /// A jump under way: from where, to where, since when.
    moving: Option<(f32, f32, Instant)>,
}

/// Ease out: quick, then settling.
fn ease(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

impl LivtkApp {
    /// Measures the panel's scroll each frame: which jumps to show, what is new below; and moves
    /// a jump under way on. A change draws the panel again.
    pub(super) fn jump_probe(&self, node: Uuid, events: usize) -> impl IntoElement {
        let (state, scroll) = (self.jumps.clone(), self.detail_scroll.clone());
        canvas(
            move |_, window, _| {
                let mut s = state.borrow_mut();
                if let Some((from, to, since)) = s.moving {
                    let t = (since.elapsed().as_secs_f32() * 1000. / JUMP_MS).min(1.);
                    let y = from + (to - from) * ease(t);
                    scroll.set_offset(point(px(0.), px(-y)));
                    if t < 1. {
                        window.request_animation_frame();
                    } else {
                        s.moving = None;
                    }
                }
                let view = f32::from(scroll.bounds().size.height);
                let at = -f32::from(scroll.offset().y);
                let max = f32::from(scroll.max_offset().y);
                let at_end = max - at < AT_END;
                // Seen: everything, once its end is in view; what came since, while it is not.
                let seen = match s.seen {
                    Some((n, count)) if n == node && !at_end => count,
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
        let foot = self.stuck_foot().map_or(px(0.), |h| h + px(8.));
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
        let mut s = self.jumps.borrow_mut();
        if cx.reduce_motion() {
            scroll.set_offset(point(px(0.), px(-to)));
            s.moving = None;
        } else {
            s.moving = Some((from, to, Instant::now()));
        }
        drop(s);
        cx.notify();
    }
}

/// A fresh jump state, shared with the probe.
pub(super) fn jumps() -> Rc<RefCell<JumpState>> {
    Rc::new(RefCell::new(JumpState::default()))
}
