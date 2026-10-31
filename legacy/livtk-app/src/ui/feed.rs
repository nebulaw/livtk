//! The activity view: the feed of events, day by day.

use super::*;

impl LivtkApp {
    /// The activity feed: day headings and one line per event, newest first, virtualised as the
    /// lists are; "Show older" ends it while more pages wait.
    /// The Activity view: the grid above, the days picked in it, then the feed.
    pub(super) fn activity_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let chip = self.grid_pick_chip(cx);
        let feed = self.feed(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .children(self.model.flow.as_ref().and_then(flow_strip))
            .children(chip)
            .child(div().flex_1().min_h_0().child(feed))
            .into_any_element()
    }

    pub(super) fn feed(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.model.activity.is_empty() {
            return div()
                .size_full()
                .child(empty(
                    Icon::Activity,
                    if self.model.grid.pick.span.is_some() {
                        "Nothing on these days"
                    } else {
                        "No activity yet"
                    },
                    if self.model.grid.pick.span.is_some() {
                        "Pick another day in the grid, or clear the pick above."
                    } else {
                        "What Claude and you do in this project shows up here, newest first."
                    },
                ))
                .into_any_element();
        }
        let rows = self.model.feed();
        let more = self.model.activity_more;
        // The lines, "Show older" while pages wait, then the dock's room at the end.
        let count = rows.len() + usize::from(more) + DOCK_ROWS;
        let pane = self.model.focused();
        let list = uniform_list(
            "feed",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                // Laid out later than drawn: as its own pane, whichever is focused.
                let was = this.model.act_on(pane);
                let out = range
                    .map(|i| match rows.get(i) {
                        // Every line spans the feed, as the lists' rows do.
                        Some(FeedRow::Day(label)) => div()
                            .w_full()
                            .h(px(ROW))
                            .px_4()
                            .flex()
                            .items_end()
                            .pb_1()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(TEXT_4))
                            .child(label.clone())
                            .into_any_element(),
                        Some(FeedRow::Event(e)) => this.feed_line(*e, cx),
                        None if !(more && i == rows.len()) => dock_room(),
                        None => div()
                            .id("show-older")
                            .debug_selector(|| "show-older".into())
                            .w_full()
                            .h(px(ROW))
                            .group("show-older")
                            .px_4()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(color(TEXT_4))
                            .hover(|s| s.text_color(color(TEXT_2)))
                            .child(icon_hover(
                                Icon::CaretDown,
                                10.,
                                TEXT_4,
                                "show-older",
                                TEXT_2,
                            ))
                            .child("Show older")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.model.activity_older();
                                cx.notify();
                            }))
                            .into_any_element(),
                    })
                    .collect();
                this.model.act_on(was);
                out
            }),
        )
        .track_scroll(&self.list_scroll())
        .size_full();
        div()
            .debug_selector(|| "feed".into())
            .size_full()
            .child(list)
            .into_any_element()
    }

    /// One event: a dot if it is new on this visit, who, what, the node, and when.
    pub(super) fn feed_line(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(a) = self.model.activity.get(index) else {
            return div().into_any_element();
        };
        let on = index == self.model.activity_index;
        let new = self.model.is_new(a);
        let human = a.event.by == "human";
        let (key, title) = a.node.as_ref().map_or((String::new(), String::new()), |b| {
            (b.key.clone(), b.title.clone())
        });
        let (own, moving) = self.highlight(&format!("event-{index}"), on);
        let line = div()
            .id(SharedString::from(format!("event-{index}")))
            .debug_selector(move || format!("event-{index}"))
            .relative()
            .w_full()
            .h(px(ROW))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .when(own, |d| d.bg(color(SELECTED)))
            .when(!on, |d| d.hover(|s| s.bg(color(HOVER))))
            .children(moving.map(|(from, to, epoch)| glide_highlight(from, to, epoch)))
            .child(
                div()
                    .size(px(6.))
                    .flex_none()
                    .rounded_full()
                    .when(new, |d| d.bg(color(ACCENT))),
            )
            .child(avatar(human))
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(if new { TEXT } else { TEXT_2 }))
                    .child(who(&a.event.by).to_owned()),
            )
            .child(
                div()
                    .flex_none()
                    .max_w(px(320.))
                    .truncate()
                    .text_color(color(TEXT_3))
                    .child(describe(a)),
            )
            .child(div().flex_none().text_color(color(TEXT_4)).child(key))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(color(TEXT_2))
                    .child(title),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(ago(&a.event.at)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.activity_select(index);
                this.model.open_detail();
                cx.notify();
            }));
        let flash = self.model.flashed.get(&a.event.id).copied();
        flashing(line, &a.event.id.to_string(), flash)
    }
}
