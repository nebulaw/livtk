//! Where the flow's states and threads go. Plain arithmetic, GPU-free and so tested as such:
//! the component draws exactly this.
//!
//! The main path runs left to right along one line, the states off it (parked, canceled,
//! rejected) on a line below. Neighbours on the path are joined by threads; the state the node is
//! in drops a thread to each state below (or, when it is below, rises to each state it can go
//! back to). A thread is lit when a move from here passes along it.

use livtk_core::State;

use crate::model::{Arrow, Look, Step};

/// The flow's height, whatever the state: the panel never moves under it.
pub const HEIGHT: f32 = 132.;
/// The main path's line, and the line of the states off it.
pub const MAIN_Y: f32 = 20.;
pub const BRANCH_Y: f32 = 96.;
/// Below the main path's labels, where a thread down to the line below leaves (or one rising
/// from it arrives), so it never crosses a label.
pub const UNDER_LABELS: f32 = 58.;
/// A state's node, and the current state's: the larger.
pub const NODE: f32 = 24.;
pub const CURRENT: f32 = 32.;
/// Room at each end of the path for the first and last labels, centred under their nodes.
pub const INSET: f32 = 26.;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Point = Point { x: 0., y: 0. };

    pub fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    fn minus(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }

    pub fn plus(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }

    fn times(self, k: f32) -> Point {
        Point::new(self.x * k, self.y * k)
    }

    fn dot(self, o: Point) -> f32 {
        self.x * o.x + self.y * o.y
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// The point `t` of the way from here to `to`.
    pub fn lerp(self, to: Point, t: f32) -> Point {
        self.plus(to.minus(self).times(t))
    }
}

/// A state's place in the flow.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub state: State,
    /// Its centre.
    pub at: Point,
    pub main: bool,
    /// Its diameter.
    pub size: f32,
}

/// How a thread is lit: idle, along a move anyone can make, or along the human's decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glow {
    Idle,
    Move,
    Decision,
}

impl Glow {
    fn of(look: Look) -> Glow {
        match look {
            Look::Decision => Glow::Decision,
            Look::Move => Glow::Move,
            Look::Current | Look::Out => Glow::Idle,
        }
    }
}

/// A thread between two states. Lit, it leads from `from` toward `to`, the way a move goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thread {
    pub from: usize,
    pub to: usize,
    pub glow: Glow,
    /// Along the main path, between neighbours; else down to (or up from) the line below.
    pub along: bool,
    /// Two strands twisting past each other: a thread that touches the current state, or goes
    /// down to (or up from) the line below. The rest of the path is a straight line.
    pub twist: bool,
}

/// The whole flow laid out in a band `width` wide and `HEIGHT` tall, its nodes in the order of
/// the steps it was made from.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub nodes: Vec<Node>,
    pub threads: Vec<Thread>,
    pub current: Option<usize>,
    pub width: f32,
}

/// Lay out `steps` (main path first, as `steps_with` gives them) with the main path's `arrows`.
pub fn layout(steps: &[Step], arrows: &[Arrow], width: f32) -> Layout {
    let main = steps.iter().filter(|s| s.main).count();
    let side = steps.len() - main;
    let span = (width - 2. * INSET).max(0.);
    // A fractional place `f` along the main path, from 0 at its first state.
    let x_at = |f: f32| {
        if main > 1 {
            INSET + f * span / (main - 1) as f32
        } else {
            width / 2.
        }
    };
    let mut nodes = Vec::with_capacity(steps.len());
    let (mut i, mut j) = (0usize, 0usize);
    for s in steps {
        let size = if s.current { CURRENT } else { NODE };
        let at = if s.main {
            i += 1;
            Point::new(x_at((i - 1) as f32), MAIN_Y)
        } else {
            j += 1;
            // Spread evenly under the path.
            let f = (main.saturating_sub(1)) as f32 * j as f32 / (side + 1) as f32;
            Point::new(x_at(f), BRANCH_Y)
        };
        nodes.push(Node {
            state: s.state,
            at,
            main: s.main,
            size,
        });
    }
    let current = steps.iter().position(|s| s.current);
    let mut threads = Vec::new();
    for g in 0..main.saturating_sub(1) {
        let (from, to, glow) = match arrows.get(g).copied().unwrap_or(Arrow::Idle) {
            Arrow::Idle => (g, g + 1, Glow::Idle),
            Arrow::Forward(look) => (g, g + 1, Glow::of(look)),
            Arrow::Back(look) => (g + 1, g, Glow::of(look)),
        };
        threads.push(Thread {
            from,
            to,
            glow,
            along: true,
            twist: current.is_some_and(|c| c == from || c == to),
        });
    }
    if let Some(c) = current {
        if steps[c].main {
            for (b, s) in steps.iter().enumerate().filter(|(_, s)| !s.main) {
                threads.push(Thread {
                    from: c,
                    to: b,
                    glow: Glow::of(s.look()),
                    along: false,
                    twist: true,
                });
            }
        } else {
            for (t, s) in steps.iter().enumerate().filter(|(_, s)| s.main) {
                let glow = Glow::of(s.look());
                if glow != Glow::Idle {
                    threads.push(Thread {
                        from: c,
                        to: t,
                        glow,
                        along: false,
                        twist: true,
                    });
                }
            }
        }
    }
    Layout {
        nodes,
        threads,
        current,
        width,
    }
}

/// A thread's curve, as a cubic Bézier's four points: from the edge of one node to the edge of
/// the other. A twisting thread's `strand` (about -1 or 1) bends it one way or the other, so its
/// two strands cross in the middle; a straight one ignores it.
pub fn curve(layout: &Layout, t: &Thread, strand: f32) -> [Point; 4] {
    let a = &layout.nodes[t.from];
    let b = &layout.nodes[t.to];
    if t.along {
        let dir = if b.at.x >= a.at.x { 1. } else { -1. };
        let start = Point::new(a.at.x + dir * (a.size / 2. + 3.), a.at.y);
        let end = Point::new(b.at.x - dir * (b.size / 2. + 3.), b.at.y);
        let third = (end.x - start.x) / 3.;
        let bow = if t.twist { 3. * strand } else { 0. };
        [
            start,
            Point::new(start.x + third, start.y - bow),
            Point::new(end.x - third, end.y + bow),
            end,
        ]
    } else {
        let (start, end) = if a.main {
            (
                Point::new(a.at.x, UNDER_LABELS),
                Point::new(b.at.x, b.at.y - b.size / 2. - 3.),
            )
        } else {
            (
                Point::new(a.at.x, a.at.y - a.size / 2. - 3.),
                Point::new(b.at.x, UNDER_LABELS),
            )
        };
        let bend = (end.y - start.y) * 0.6;
        [
            start,
            Point::new(start.x + 2. * strand, start.y + bend),
            Point::new(end.x - 2. * strand, end.y - bend),
            end,
        ]
    }
}

/// The point `t` of the way along a cubic Bézier.
pub fn bezier(p: &[Point; 4], t: f32) -> Point {
    let u = 1. - t;
    p[0].times(u * u * u)
        .plus(p[1].times(3. * u * u * t))
        .plus(p[2].times(3. * u * t * t))
        .plus(p[3].times(t * t * t))
}
