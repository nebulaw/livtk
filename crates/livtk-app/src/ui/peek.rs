//! Where a node's detail opens, as Notion opens a page from a database (`DetailMode`): in the
//! panel at the right (side peek), in a large card over the dimmed window (center peek), or in
//! the list's place in the pane it was opened from (full page). The detail itself is one piece,
//! `detail_panel`, framed three ways; the button beside its close picks the way, and the choice
//! sticks.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::icons::{Icon, icon, icon_hover};
use super::theme::*;
use super::{Close, LivtkApp, Menu, check, menu_panel, option_row};
use crate::model::DetailMode;

/// The center peek's card: as wide as reading wants, and the room kept round it.
const PEEK_W: f32 = 760.;
const PEEK_MARGIN: f32 = 48.;

/// The key context of a detail shown as a full page: Esc closes it there.
pub(super) const DETAIL: &str = "Detail";

/// Each way's icon.
pub(super) fn mode_icon(mode: DetailMode) -> Icon {
    match mode {
        DetailMode::Side => Icon::PanelRight,
        DetailMode::Center => Icon::CenterPeek,
        DetailMode::Full => Icon::Fit,
    }
}

impl LivtkApp {
    /// The detail bar's button for the way it opens: its icon the way it is open now, a press
    /// opening the menu of the three, the current one ticked.
    pub(super) fn detail_mode_button(&self, cx: &mut Context<Self>) -> Div {
        let open = self.menu == Some(Menu::DetailMode);
        let mode = self.model.detail_mode;
        let button = div()
            .id("detail-mode")
            .debug_selector(|| "detail-mode".into())
            .group("detail-mode")
            .size(px(26.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_lg()
            .when(open, |d| d.bg(color(SELECTED)))
            .when(!open, |d| d.hover(|s| s.bg(color(HOVER))))
            .child(if open {
                icon(mode_icon(mode), 16., TEXT)
            } else {
                icon_hover(mode_icon(mode), 16., TEXT_4, "detail-mode", TEXT_2)
            })
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new("Open as…").build(window, cx)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_menu(Menu::DetailMode, window, cx)
                }),
            );
        div().relative().child(button).when(open, |d| {
            let mut panel = menu_panel(200.).debug_selector(|| "detail-mode-menu".into());
            for m in DetailMode::ALL {
                let id = format!("detail-mode-{}", m.name().to_lowercase().replace(' ', "-"));
                panel = panel.child(
                    option_row(SharedString::from(id.clone()))
                        .debug_selector(move || id.clone())
                        .child(icon(mode_icon(m), 14., TEXT_3))
                        .child(m.name())
                        .child(check(m == mode))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.set_detail_mode(m, window, cx);
                        })),
                );
            }
            d.child(self.popover(Menu::DetailMode, true, 30., panel, cx))
        })
    }

    /// The detail opens `mode`'s way from now on; one open moves there at once.
    pub(super) fn set_detail_mode(
        &mut self,
        mode: DetailMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.detail_mode = mode;
        // A full page shows in the pane the detail was opened from: the focused one, if none.
        if self.model.detail_pane.is_none() {
            self.model.detail_pane = Some(self.model.focused_id());
        }
        self.close_menu(window, cx);
        self.focus_main(window, cx);
        cx.notify();
    }

    /// The center peek: the open node's detail in a card over the dimmed window. A press
    /// outside closes it; the keys stay the list's behind it (J and K move on, the detail
    /// follows; Esc closes it).
    pub(super) fn center_peek(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.model.detail_mode != DetailMode::Center {
            return None;
        }
        let d = self.model.detail.clone()?;
        let size = window.viewport_size();
        let w = PEEK_W
            .min(f32::from(size.width) - 2. * PEEK_MARGIN)
            .max(320.);
        let h = (f32::from(size.height) - 2. * PEEK_MARGIN).max(240.);
        let card = self
            .detail_panel(&d, w, cx)
            .h(px(h))
            .rounded_xl()
            .overflow_hidden()
            .bg(color(SURFACE))
            .border_1()
            .border_color(color(BORDER_STRONG))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        Some(
            div()
                .id("center-peek")
                .debug_selector(|| "center-peek".into())
                .absolute()
                .inset_0()
                .occlude()
                .bg(with_alpha(0x000000, 0.6))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, e: &MouseDownEvent, window, cx| {
                        if !e.first_mouse {
                            this.model.close_detail();
                            this.focus_main(window, cx);
                            cx.notify();
                        }
                    }),
                )
                .child(card)
                .with_animation(
                    "center-peek-in",
                    Animation::new(std::time::Duration::from_millis(140))
                        .with_easing(super::ease_out_cubic),
                    |el, t| el.opacity(t),
                )
                .into_any_element(),
        )
    }

    /// The full page: in the pane acted on, when it is the one the detail was opened from, the
    /// detail in the list's place, `width` wide. Focused, it has the keys (Esc closes it);
    /// outside the list's, so its note field is typed in freely.
    pub(super) fn full_page(
        &mut self,
        width: f32,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let m = &self.model;
        if m.detail_mode != DetailMode::Full || m.detail_pane != Some(m.panes()[m.focused()].id) {
            return None;
        }
        let d = m.detail.clone()?;
        let page = self.detail_panel(&d, width, cx).w_full();
        Some(
            div()
                .debug_selector(|| "full-page".into())
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .when(focused, |el| {
                    el.key_context(DETAIL)
                        .track_focus(&self.nav)
                        .on_action(cx.listener(|this, _: &Close, _, cx| {
                            this.model.close_detail();
                            cx.notify();
                        }))
                })
                .child(page)
                .into_any_element(),
        )
    }
}
