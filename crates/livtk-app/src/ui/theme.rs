//! Linear's dark palette, at its darkest: near-black surfaces, neutral hairlines, four text
//! levels, one indigo accent, and Linear's status colours. Colour carries meaning, nothing else.
//!
//! Surfaces, text and accent follow published breakdowns of Linear's design system (canvas
//! #010102, surfaces #0f1011 to #191a1b, hairlines #23252a and #34343a, ink #f7f8f8 to #62666d,
//! accent #5e6ad2 / #828fff). Status colours follow Linear's icons (backlog #bec2c8, todo
//! #e2e2e2, in progress #f2c94c, in review #4cb782, done #5e6ad2, canceled #95a2b3); review and
//! canceled are not confirmed by a public source. Urgent orange and rejected red are ours.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;
use livtk_core::{Kind, State};

/// The window and the sidebar: Linear's canvas.
pub const WINDOW: u32 = 0x010102;
/// The inset content panel.
pub const CONTENT: u32 = 0x08090a;
/// Cards, group bands, the detail panel, modals.
pub const SURFACE: u32 = 0x0f1011;
pub const HOVER: u32 = 0x141516;
pub const SELECTED: u32 = 0x18191a;
pub const LIFTED: u32 = 0x191a1b;
pub const BORDER: u32 = 0x23252a;
pub const BORDER_STRONG: u32 = 0x34343a;
pub const TEXT: u32 = 0xf7f8f8;
pub const TEXT_2: u32 = 0xd0d6e0;
pub const TEXT_3: u32 = 0x8a8f98;
pub const TEXT_4: u32 = 0x62666d;
pub const ACCENT: u32 = 0x5e6ad2;
pub const ACCENT_HOVER: u32 = 0x828fff;

pub const BACKLOG: u32 = 0xbec2c8;
pub const TODO: u32 = 0xe2e2e2;
pub const PROGRESS: u32 = 0xf2c94c;
pub const REVIEW: u32 = 0x4cb782;
pub const DONE: u32 = 0x5e6ad2;
pub const CANCELED: u32 = 0x95a2b3;
pub const URGENT: u32 = 0xf2994a;
pub const RED: u32 = 0xeb5757;

/// Row and header height, so virtualised lists stay uniform.
pub const ROW: f32 = 36.0;

pub fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub fn with_alpha(hex: u32, alpha: f32) -> Hsla {
    let mut c = color(hex);
    c.a = alpha;
    c
}

/// Dress GPUI Component's widgets (inputs, text areas, scroll bars) in the same palette.
pub fn apply(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    let theme = Theme::global_mut(cx);
    let c = &mut theme.colors;
    c.background = color(CONTENT);
    c.foreground = color(TEXT);
    c.border = color(BORDER);
    c.input = color(BORDER_STRONG);
    c.ring = color(ACCENT);
    c.caret = color(TEXT);
    c.selection = with_alpha(ACCENT, 0.35);
    c.muted = color(SURFACE);
    c.muted_foreground = color(TEXT_4);
    c.popover = color(SURFACE);
    c.popover_foreground = color(TEXT);
    c.primary = color(ACCENT);
    c.primary_hover = color(ACCENT_HOVER);
    c.primary_active = color(ACCENT);
    c.primary_foreground = color(TEXT);
    c.secondary = color(SURFACE);
    c.secondary_hover = color(HOVER);
    c.secondary_active = color(SELECTED);
    c.secondary_foreground = color(TEXT);
    c.accent = color(HOVER);
    c.accent_foreground = color(TEXT);
    c.list = color(CONTENT);
    c.list_hover = color(HOVER);
    c.list_active = color(SELECTED);
    c.list_active_border = color(ACCENT);
    c.scrollbar = with_alpha(0x000000, 0.0);
    c.scrollbar_thumb = color(BORDER_STRONG);
    c.scrollbar_thumb_hover = color(TEXT_4);
    c.overlay = with_alpha(0x000000, 0.6);
    c.window_border = color(BORDER);
    c.drag_border = color(ACCENT);
    c.drop_target = with_alpha(ACCENT, 0.15);
    // Edited in place, so pushed down by hand: the markdown views and the scrollbars paint
    // from the copy gpui-component keeps below.
    Theme::sync_base(cx);
    cx.refresh_windows();
}

/// The colour of a state, as Linear colours its status icons.
pub fn state_color(state: State) -> u32 {
    match state {
        State::Backlog => BACKLOG,
        State::Planned | State::Acked | State::Proposed => TODO,
        State::Doing => PROGRESS,
        State::Review => REVIEW,
        State::Done | State::Ratified | State::Accepted => DONE,
        State::Recorded => TEXT_3,
        State::Rejected => RED,
        State::Parked | State::Canceled | State::Retired | State::Superseded => CANCELED,
        // A declared state: settled when closed, else in hand.
        s if s.is_closed() => DONE,
        _ => PROGRESS,
    }
}

/// A status icon in Linear's manner: a faint ring for backlog, a ring for what is ready, half
/// filled in hand, filled once settled.
pub fn state_mark(kind: Kind, state: State) -> Div {
    let c = state_color(state);
    let ring = div()
        .size(px(14.))
        .flex_none()
        .rounded_full()
        .border_2()
        .border_color(color(c));
    match state {
        State::Backlog | State::Parked => ring.opacity(0.55),
        State::Acked => ring
            .flex()
            .items_center()
            .justify_center()
            .child(div().size(px(4.)).rounded_full().bg(color(c))),
        State::Doing | State::Review => ring
            .flex()
            .items_center()
            .justify_center()
            .child(div().size(px(6.)).rounded_full().bg(color(c))),
        State::Done | State::Ratified | State::Accepted | State::Recorded => {
            div().size(px(14.)).flex_none().rounded_full().bg(color(c))
        }
        State::Canceled | State::Rejected => div()
            .size(px(14.))
            .flex_none()
            .rounded_full()
            .bg(with_alpha(c, 0.7)),
        // A declared kind's: a ring where it starts, filled once closed, half filled between.
        s if s.is_declared() && s.is_closed() => {
            div().size(px(14.)).flex_none().rounded_full().bg(color(c))
        }
        s if s.is_declared() && s == kind.initial_state() => ring.border_color(color(TODO)),
        State::Retired | State::Superseded => ring.opacity(0.4),
        State::Planned | State::Proposed => ring,
        _ => ring
            .flex()
            .items_center()
            .justify_center()
            .child(div().size(px(6.)).rounded_full().bg(color(c))),
    }
}

/// Priority in Linear's manner: urgent is a filled orange square with an exclamation mark;
/// high, medium and low are three bars with three, two or one lit.
pub fn priority_mark(priority: u8) -> Div {
    if priority == 1 {
        return div()
            .size(px(14.))
            .flex_none()
            .rounded(px(3.))
            .bg(color(URGENT))
            .flex()
            .items_center()
            .justify_center()
            .text_color(color(WINDOW))
            .text_size(px(10.))
            .font_weight(FontWeight::BOLD)
            .child("!");
    }
    let lit = 5 - usize::from(priority).clamp(2, 4);
    let mut d = div()
        .flex()
        .flex_none()
        .items_end()
        .gap(px(1.5))
        .h(px(12.))
        .w(px(14.));
    for i in 0..3 {
        d = d.child(
            div()
                .w(px(3.))
                .h(px(4.0 + 3.5 * i as f32))
                .rounded(px(1.))
                .bg(color(if i < lit { TEXT_3 } else { BORDER_STRONG })),
        );
    }
    d
}

pub const PRIORITY_NAMES: [&str; 4] = ["Urgent", "High", "Medium", "Low"];
