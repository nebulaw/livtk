//! The parts forms are made of, one look wherever they are used: Settings, an agent's page.

use gpui_component::Sizable;
use gpui_component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::icons::{Icon, icon};
use super::theme::*;

/// Choices side by side in one tray; each is a `choice`.
pub(super) fn segmented() -> Div {
    div()
        .flex()
        .p(px(2.))
        .gap(px(2.))
        .rounded_lg()
        .bg(color(SURFACE))
        .border_1()
        .border_color(color(BORDER))
}

/// One of a `segmented` tray's choices, lit while chosen.
pub(super) fn choice(id: String, label: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || id.clone())
        .h(px(26.))
        .px_3()
        .flex()
        .items_center()
        .rounded_md()
        .text_sm()
        .when(on, |d| d.bg(color(SELECTED)).text_color(color(TEXT)))
        .when(!on, |d| {
            d.text_color(color(TEXT_3))
                .hover(|h| h.text_color(color(TEXT_2)))
        })
        .child(label)
}

/// A switch: lit and to the right when on. Its selector says which (`<id>-on`, `<id>-off`).
pub(super) fn toggle(id: String, on: bool) -> Stateful<Div> {
    let name = id.clone();
    div()
        .id(SharedString::from(id))
        .debug_selector(move || format!("{name}-{}", if on { "on" } else { "off" }))
        .w(px(34.))
        .h(px(20.))
        .p(px(2.))
        .flex()
        .when(on, |d| d.justify_end())
        .rounded_full()
        .bg(color(if on { ACCENT } else { BORDER_STRONG }))
        .child(div().size(px(16.)).rounded_full().bg(color(TEXT)))
}

/// One of many to pick several from (an agent's tools), lit while picked. Its selector says
/// which (`<id>-on`, `<id>-off`).
pub(super) fn chip(id: &str, label: &'static str, on: bool) -> Stateful<Div> {
    let sel = format!("{id}-{}", if on { "on" } else { "off" });
    div()
        .id(SharedString::from(id.to_owned()))
        .debug_selector(move || sel.clone())
        .h(px(22.))
        .px_2()
        .flex()
        .items_center()
        .rounded_full()
        .text_xs()
        .border_1()
        .when(on, |d| {
            d.bg(color(SELECTED))
                .border_color(color(ACCENT))
                .text_color(color(TEXT))
        })
        .when(!on, |d| {
            d.border_color(color(BORDER_STRONG))
                .text_color(color(TEXT_3))
                .hover(|h| h.text_color(color(TEXT_2)))
        })
        .child(label)
}

/// A button; `main` is the one the form is for, in the accent.
pub(super) fn button(id: &'static str, label: &str, main: bool) -> Stateful<Div> {
    labelled(id, label, main, None)
}

/// A button whose label `glyph` leads, as a run's Start and Stop.
pub(super) fn icon_button(id: &'static str, glyph: Icon, label: &str, main: bool) -> Stateful<Div> {
    labelled(id, label, main, Some(glyph))
}

fn labelled(id: &'static str, label: &str, main: bool, glyph: Option<Icon>) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .h(px(28.))
        .px_3()
        .flex()
        .items_center()
        .rounded_lg()
        .text_sm()
        .when(main, |d| {
            d.bg(color(ACCENT))
                .text_color(color(TEXT))
                .hover(|h| h.bg(color(ACCENT_HOVER)))
        })
        .when(!main, |d| {
            d.border_1()
                .border_color(color(BORDER_STRONG))
                .text_color(color(TEXT_2))
                .hover(|h| h.bg(color(HOVER)))
        })
        .when_some(glyph, |d, g| {
            d.gap_1p5()
                .child(icon(g, 14., if main { TEXT } else { TEXT_3 }))
        })
        .child(label.to_owned())
}

/// A one-line text field, as wide as its container: give it a width where it should have one.
pub(super) fn field(state: &Entity<InputState>, id: &'static str) -> Div {
    div()
        .debug_selector(move || id.into())
        .h(px(28.))
        .px_2()
        .flex()
        .items_center()
        .rounded_lg()
        .bg(color(CONTENT))
        .border_1()
        .border_color(color(BORDER_STRONG))
        .child(
            div()
                .flex_1()
                .child(Input::new(state).appearance(false).small()),
        )
}
