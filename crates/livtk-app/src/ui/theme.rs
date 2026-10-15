//! Linear's calm density: a dark, quiet palette where colour carries meaning.

use gpui_kit::*;
use livtk_core::{Kind, State};

pub const BG: u32 = 0x0f1011;
pub const SIDEBAR: u32 = 0x0b0c0d;
pub const PANEL: u32 = 0x141517;
pub const RAISED: u32 = 0x1a1b1e;
pub const HOVER: u32 = 0x1c1d21;
pub const SELECTED: u32 = 0x22232a;
pub const BORDER: u32 = 0x232428;
pub const TEXT: u32 = 0xe4e5e9;
pub const TEXT_2: u32 = 0x9c9da4;
pub const TEXT_3: u32 = 0x65666d;
/// The accent: what waits for the human, the primary action, the focus.
pub const ACCENT: u32 = 0x5e6ad2;
pub const GREEN: u32 = 0x4cb782;
pub const YELLOW: u32 = 0xf2c94c;
pub const ORANGE: u32 = 0xf2994a;
pub const RED: u32 = 0xeb5757;
pub const BLUE: u32 = 0x4ea7fc;
pub const VIOLET: u32 = 0xa284e0;

/// Row and header heights, so virtualised lists stay uniform.
pub const ROW: f32 = 34.0;

pub fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub fn with_alpha(hex: u32, alpha: f32) -> Hsla {
    let mut c = color(hex);
    c.a = alpha;
    c
}

/// The colour of a state: grey for not yet, the accent for waiting on the human, yellow for in
/// hand, violet for review, green for settled, red for refused, dim for over.
pub fn state_color(kind: Kind, state: State) -> u32 {
    match (kind, state) {
        (_, State::Backlog | State::Parked) => TEXT_3,
        (_, State::Planned | State::Proposed) => ACCENT,
        (_, State::Acked) => TEXT_2,
        (_, State::Doing) => YELLOW,
        (_, State::Review) => VIOLET,
        (_, State::Done | State::Ratified | State::Accepted) => GREEN,
        (_, State::Recorded) => BLUE,
        (_, State::Rejected) => RED,
        (_, State::Canceled | State::Retired | State::Superseded) => TEXT_3,
    }
}

/// A small state mark: a ring while open, a filled dot once settled, half-filled in hand.
pub fn state_mark(kind: Kind, state: State) -> Div {
    let c = state_color(kind, state);
    let ring = div()
        .size(px(12.))
        .rounded_full()
        .border_2()
        .border_color(color(c));
    match state {
        State::Done | State::Ratified | State::Accepted | State::Recorded => {
            div().size(px(12.)).rounded_full().bg(color(c))
        }
        State::Doing | State::Review => {
            ring.child(div().size(px(4.)).m(px(2.)).rounded_full().bg(color(c)))
        }
        State::Canceled | State::Rejected | State::Retired | State::Superseded => ring.opacity(0.6),
        _ => ring,
    }
}

/// Priority as bars: four for urgent (in orange), down to one for low.
pub fn priority_mark(priority: u8) -> Div {
    let (bars, c) = match priority {
        1 => (4, ORANGE),
        2 => (3, TEXT_2),
        3 => (2, TEXT_2),
        _ => (1, TEXT_3),
    };
    let mut d = div().flex().items_end().gap(px(1.5)).h(px(12.)).w(px(14.));
    for i in 0..4 {
        let h = 4.0 + 2.5 * i as f32;
        let lit = i < bars;
        d = d.child(div().w(px(2.5)).h(px(h)).rounded(px(1.)).bg(if lit {
            color(c)
        } else {
            color(BORDER)
        }));
    }
    d
}
