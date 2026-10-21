//! The notice in the top bar and the toasts.

use super::*;

impl LivtkApp {
    /// The notice, at the bottom right above the dock's level, with its ×. The pointer over it
    /// holds it; when its time is up it fades out. Its id stays the same from one notice to the
    /// next, so a pointer over it stays over it; its animations are each notice's own, so a new
    /// one comes in afresh.
    /// The notifications, bottom right above the dock: newest first, a few shown and the rest
    /// counted.
    pub(super) fn notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (shown, more) = self.model.toasts().shown();
        if shown.is_empty() {
            return None;
        }
        let mut stack = div()
            .absolute()
            // Above the dock's level, so the two never meet.
            .bottom(px(DOCK_SPACE + 8.))
            .right(px(16.))
            .w(px(TOAST_WIDTH))
            .flex()
            .flex_col_reverse()
            .gap_2();
        for (i, t) in shown.into_iter().enumerate() {
            stack = stack.child(self.toast(t, i == 0, cx));
        }
        if more > 0 {
            stack = stack.child(
                div()
                    .debug_selector(|| "toasts-more".into())
                    .self_end()
                    .px_2()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(format!("+{more} more")),
            );
        }
        Some(stack.into_any_element())
    }

    /// One notification, drawn as the new task modal is: a breadcrumb (its icon in its level's
    /// colour, where it comes from and what it is, how many times it came, and a close), its
    /// title and markdown text (four lines, then Show more), and its buttons in a footer under a
    /// hairline. The pointer over it holds it. The newest answers to `notice` in tests.
    pub(super) fn toast(&self, t: &Toast, newest: bool, cx: &mut Context<Self>) -> AnyElement {
        let id = t.id;
        let hue = match t.level {
            Level::Info => ACCENT,
            Level::Success => REVIEW,
            Level::Warning => PROGRESS,
            Level::Error => RED,
        };
        let glyph = match t.mark {
            Mark::Bug => Icon::Bug,
            Mark::Bell => Icon::Bell,
            Mark::Inbox => Icon::Inbox,
            Mark::Meteor => Icon::Meteor,
            Mark::Message => Icon::Message,
            // The sparkles, as Claude's own mark is drawn.
            Mark::Sparkles => Icon::Claude,
        };
        let name = |what: &str| -> SharedString {
            if newest {
                format!("notice{what}").into()
            } else {
                format!("toast-{id}{what}").into()
            }
        };
        let close_name = name("-close");
        let close = div()
            .id(close_name.clone())
            .debug_selector(move || close_name.to_string())
            .group("toast-close")
            .size(px(20.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_lg()
            .hover(|s| s.bg(color(HOVER)))
            .child(icon_hover(Icon::Close, 14., TEXT_4, "toast-close", TEXT))
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new("Close").build(window, cx)
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.dismiss(id);
                cx.notify();
            }));
        let (from, what) = t.crumbs();
        let crumbs = div()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(color(TEXT_3))
            .child(icon(glyph, 14., hue))
            .child(from.to_owned())
            .child("›")
            .child(what.to_owned())
            .child(div().flex_1())
            .when(t.count > 1, |d| {
                d.child(
                    div()
                        .text_color(color(TEXT_4))
                        .child(format!("×{}", t.count)),
                )
            })
            .child(close);
        let expanded = self.expanded.contains(&id);
        let long = t.text.len() > TOAST_LONG || t.text.lines().count() > 4;
        let text = div()
            .id(SharedString::from(format!("toast-text-{id}")))
            .text_color(color(TEXT_2))
            .when(long && !expanded, |d| {
                d.max_h(px(TOAST_CLIPPED)).overflow_hidden()
            })
            .when(expanded, |d| d.max_h(px(TOAST_OPEN)).overflow_y_scroll())
            .child(TextView::markdown(
                SharedString::from(format!("toast-md-{id}")),
                t.text.clone(),
            ));
        let mut body = div()
            .flex()
            .flex_col()
            .gap_1()
            .when_some(t.title.clone(), |d, title| {
                d.child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color(TEXT))
                        .child(title),
                )
            })
            .child(text);
        if long {
            body = body.child(
                div()
                    .id(SharedString::from(format!("toast-more-{id}")))
                    .debug_selector(move || format!("toast-more-{id}"))
                    .text_xs()
                    .text_color(color(TEXT_3))
                    .hover(|s| s.text_color(color(TEXT)))
                    .child(if expanded { "Show less" } else { "Show more" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.expanded.remove(&id) {
                            this.expanded.insert(id);
                        }
                        cx.notify();
                    })),
            );
        }
        // As the modal's footer: the first button its Create task, the rest quiet.
        let footer = (!t.actions.is_empty()).then(|| {
            let mut row = div().flex().justify_end().gap_2();
            for (i, b) in t.actions.iter().enumerate() {
                let act = b.act.clone();
                let act_name = name(&format!("-act-{i}"));
                let first = i == 0;
                row = row.child(
                    div()
                        .id(act_name.clone())
                        .debug_selector(move || act_name.to_string())
                        .h(px(28.))
                        .px_3()
                        .flex()
                        .items_center()
                        .rounded_lg()
                        .font_weight(FontWeight::MEDIUM)
                        .when(first, |d| {
                            d.bg(color(ACCENT))
                                .text_color(color(TEXT))
                                .hover(|s| s.bg(color(ACCENT_HOVER)))
                        })
                        .when(!first, |d| {
                            d.border_1()
                                .border_color(color(BORDER_STRONG))
                                .text_color(color(TEXT_2))
                                .hover(|s| s.bg(color(HOVER)))
                        })
                        .child(b.label.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.model.dismiss(id);
                            this.act(&act, cx);
                        })),
                );
            }
            row
        });
        let card_name = name("");
        let card = div()
            .id(card_name.clone())
            .debug_selector(move || card_name.to_string())
            .w_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_xl()
            .bg(color(SURFACE))
            .border_1()
            .border_color(color(BORDER_STRONG))
            .shadow_lg()
            .text_color(color(TEXT))
            .text_sm()
            // Presses here are the notification's: nothing under it is clicked through it.
            .occlude()
            // Only the pointer lets it go: a key pressed with the pointer resting on it, which
            // GPUI by default takes as leaving, does not.
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.model.hold(id, *hovered, Instant::now());
                cx.notify();
            }))
            .child(crumbs)
            .child(body)
            .when_some(footer, |d, footer| {
                d.child(div().h(px(1.)).bg(color(BORDER))).child(footer)
            });
        let card = if t.leaving.is_some() {
            card.with_animation(
                SharedString::from(format!("toast-out-{id}")),
                Animation::new(NOTICE_FADE).with_easing(ease_in_out),
                |el, t| el.opacity(1.0 - t),
            )
            .into_any_element()
        } else {
            card.into_any_element()
        };
        // The coming in is drawn whether or not the card is leaving, so it plays once per
        // notification: one caught while fading comes back at full strength.
        div()
            .child(card)
            .with_animation(
                SharedString::from(format!("toast-in-{id}")),
                Animation::new(NOTICE_IN).with_easing(ease_out_quint()),
                |el, t| el.opacity(t),
            )
            .into_any_element()
    }
}
