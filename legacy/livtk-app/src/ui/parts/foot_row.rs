//! A row at the sidebar's foot (updates, Shortcuts, Settings): quieter than the views above it,
//! a sidebar row's size. It leads with an icon (or the accent's dot), then its label, cut when
//! the sidebar is narrow, then its keys or a few words of its own.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::super::icons::{Icon, icon_hover};
use super::super::theme::*;
use super::super::{NAV_ICON, NAV_ROW, NAV_TEXT, hint};

/// What a foot row leads with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Lead {
    Icon(Icon),
    /// A small dot in the accent: something waits on the human.
    Dot,
}

/// How loud a foot row is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) enum Tone {
    /// As Shortcuts and Settings: quiet, brighter on hover.
    #[default]
    Muted,
    /// The one row asking for the human (a build to restart to).
    Accent,
}

/// A row at the sidebar's foot. `build` gives the row to put a click on; a disabled row has no
/// hover, and its caller puts no click on it.
pub(in crate::ui) struct FootRow {
    id: SharedString,
    lead: Lead,
    label: SharedString,
    keys: Option<String>,
    trailing: Option<SharedString>,
    tone: Tone,
    disabled: bool,
    tip: Option<SharedString>,
}

impl FootRow {
    /// The row `id` (its debug selector too), leading with `lead`, saying `label`.
    pub(in crate::ui) fn new(
        id: impl Into<SharedString>,
        lead: Lead,
        label: impl Into<SharedString>,
    ) -> Self {
        FootRow {
            id: id.into(),
            lead,
            label: label.into(),
            keys: None,
            trailing: None,
            tone: Tone::Muted,
            disabled: false,
            tip: None,
        }
    }

    /// The keys that do what the row does, shown as a hint at its end (unless hints are off).
    pub(in crate::ui) fn keys(mut self, keys: impl Into<String>) -> Self {
        self.keys = Some(keys.into());
        self
    }

    /// A few quiet words at the row's end, kept whole while the label is cut.
    pub(in crate::ui) fn trailing(mut self, text: Option<impl Into<SharedString>>) -> Self {
        self.trailing = text.map(Into::into);
        self
    }

    pub(in crate::ui) fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// Shown, but nothing to do now (a check already under way).
    pub(in crate::ui) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// What the row's tooltip says, for when its label is cut.
    pub(in crate::ui) fn tip(mut self, tip: impl Into<SharedString>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    pub(in crate::ui) fn build(self, cx: &App) -> Stateful<Div> {
        let FootRow {
            id,
            lead,
            label,
            keys,
            trailing,
            tone,
            disabled,
            tip,
        } = self;
        let name = id.clone();
        let (rest, over) = match tone {
            Tone::Muted => (TEXT_4, TEXT_3),
            Tone::Accent => (ACCENT_HOVER, TEXT),
        };
        let over = if disabled { rest } else { over };
        div()
            .id(id)
            .debug_selector(move || name.to_string())
            .group("nav")
            .px_2()
            .h(px(NAV_ROW))
            .text_size(px(NAV_TEXT))
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .rounded_lg()
            .text_color(color(rest))
            .when(!disabled, |d| {
                d.hover(move |s| s.bg(color(HOVER)).text_color(color(over)))
            })
            .child(match lead {
                Lead::Icon(glyph) => {
                    icon_hover(glyph, NAV_ICON, rest, "nav", over).into_any_element()
                }
                Lead::Dot => div()
                    .flex_none()
                    .size(px(6.))
                    .rounded_full()
                    .bg(color(ACCENT))
                    .into_any_element(),
            })
            .child(div().flex_1().min_w_0().truncate().child(label))
            .children(trailing.map(|text| {
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(text)
            }))
            .children(keys.and_then(|k| hint(&k, cx)))
            .when_some(tip, |d, tip| {
                d.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
            })
    }
}
