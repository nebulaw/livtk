//! The list in the middle: its groups and rows, the board, the inbox.

use super::*;

impl LivtkApp {
    /// The inbox: a short list, so rows can fade and collapse when the human acts on them.
    pub(super) fn inbox(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.model.rows().is_empty() && self.model.leaving().is_empty() {
            return div()
                .size_full()
                .child(empty(
                    Icon::Inbox,
                    "Nothing waits for you",
                    "Plans to acknowledge, lessons to ratify and decisions to accept appear here.",
                ))
                .into_any_element();
        }
        let mut shown: Vec<(Summary, Option<Instant>)> = self
            .model
            .rows()
            .iter()
            .map(|r| (r.clone(), None))
            .collect();
        for (r, at, when) in self.model.leaving() {
            let i = (*at).min(shown.len());
            shown.insert(i, (r.clone(), Some(*when)));
        }
        let selected = self.model.selected().map(|r| r.id);
        let mut list = div()
            .id("inbox")
            .size_full()
            .overflow_y_scroll()
            .pt_1()
            .flex()
            .flex_col();
        for (r, leaving) in shown {
            list = list.child(self.row(
                &r,
                Some(r.id) == selected && leaving.is_none(),
                leaving,
                true,
                cx,
            ));
        }
        // Room at the end of what scrolls, not round it: rows pass under the dock, and the
        // last one can rise clear of it.
        list.child(div().h(px(DOCK_SPACE)).flex_none())
            .into_any_element()
    }

    /// An empty list's way on: the filter cleared, when one hides what is there; else a new
    /// task, where tasks are.
    pub(super) fn empty_list_action(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if !self.model.display().filter.is_empty() {
            return Some(empty_action(
                "empty-clear-filter",
                "Clear the filter",
                cx.listener(|this, _, window, cx| {
                    this.model.set_filter("");
                    this.sync_filter(window, cx);
                    cx.notify();
                }),
            ));
        }
        (self.model.view() == View::Tasks).then(|| {
            empty_action(
                "empty-new-task",
                "New task",
                cx.listener(|this, _, window, cx| this.open_form(window, cx)),
            )
        })
    }

    /// Tasks, lessons, decisions or findings: a virtualised list in solid group bands.
    pub(super) fn list(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if self.model.rows().is_empty() && self.model.groups().is_empty() {
            let what = self.model.view().title().to_lowercase();
            return div()
                .size_full()
                .child(
                    empty(
                        view_icon(self.model.view()),
                        &format!("No {what} here"),
                        "Clear the filter, press S D to show dismissed work, or N for a new task.",
                    )
                    .children(self.empty_list_action(cx)),
                )
                .into_any_element();
        }
        // The rows, then the dock's room at the end of what scrolls.
        let count = self.items().len() + DOCK_ROWS;
        let pane = self.model.focused();
        let list = uniform_list(
            "rows",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                // Laid out later than drawn: its own pane's rows, whichever is focused.
                let was = this.model.act_on(pane);
                let items = this.items();
                let selected = this.model.selected().map(|r| r.id);
                let mut out = Vec::with_capacity(range.len());
                for i in range {
                    match items.get(i) {
                        Some(Item::Band(gi)) => out.push(this.band(*gi, cx)),
                        Some(Item::Row(ri)) => {
                            let r = this.model.rows()[*ri].clone();
                            let sel = Some(r.id) == selected;
                            out.push(this.row(&r, sel, None, false, cx));
                        }
                        None => out.push(dock_room()),
                    }
                }
                this.model.act_on(was);
                out
            }),
        )
        .track_scroll(&self.list_scroll())
        .size_full();
        // More projects active than is wise: said plainly above them, a nudge and not a lock.
        match self.model.too_many_active() {
            Some(n) => div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .debug_selector(|| "too-many-active".into())
                        .h(px(ROW))
                        .flex_none()
                        .px_4()
                        .flex()
                        .items_center()
                        .gap_2()
                        .border_b_1()
                        .border_color(color(BORDER))
                        .text_xs()
                        .text_color(color(TEXT_3))
                        .child(icon(Icon::Projects, 12., TEXT_4))
                        .child(format!(
                            "{n} projects are active at once; fewer at a time finish sooner."
                        )),
                )
                .child(div().flex_1().min_h_0().child(list))
                .into_any_element(),
            None => list.into_any_element(),
        }
    }

    /// A group's solid header band: its mark, name and count. Click to collapse; drop a row on
    /// it to move the row into the group.
    pub(super) fn band(&self, gi: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(g) = self.model.groups().get(gi).cloned() else {
            return div().into_any_element();
        };
        let (mark, label) = group_label(
            g.key,
            g.kind.unwrap_or(Kind::Task),
            self.model.project_names(),
        );
        let key = g.key;
        div()
            .id(SharedString::from(format!("band-{gi}")))
            .w_full()
            .h(px(ROW))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .bg(color(if self.model.band() == Some(key) {
                SELECTED
            } else {
                SURFACE
            }))
            .border_b_1()
            .border_color(color(BORDER))
            .hover(|s| s.bg(color(HOVER)))
            .child(icon(
                if g.collapsed {
                    Icon::CaretRight
                } else {
                    Icon::CaretDown
                },
                10.,
                TEXT_4,
            ))
            .child(mark)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(TEXT))
                    .child(label),
            )
            .child(
                div()
                    .text_color(color(TEXT_4))
                    .child(format!("{}", g.count)),
            )
            .drag_over::<Dragged>(move |style, d, _, _| {
                if d.fits(key) {
                    style
                        .bg(with_alpha(ACCENT, 0.18))
                        .border_color(color(ACCENT))
                } else {
                    style
                }
            })
            .on_drop(cx.listener(move |this, d: &Dragged, _, cx| {
                this.model.drop_on(d.id, key);
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.toggle_collapsed(key);
                cx.notify();
            }))
            .into_any_element()
    }

    /// A parent's sub-tasks: how many are done, of how many.
    pub(super) fn sub_task_count(&self, id: Uuid) -> Option<Div> {
        let &(done, total) = self.model.progress().get(&id)?;
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(color(TEXT_4))
                .child(icon(Icon::SubTask, 12., TEXT_4))
                .child(format!("{done}/{total}")),
        )
    }

    /// A project's row, after its title: how far it has come (a bar and done of all), how it
    /// stands, its target, and what waits on the human.
    pub(super) fn project_parts(&self, r: &Summary) -> Option<Div> {
        let g = self.model.project_progress(r.id)?;
        let key = r.key.clone();
        let share = if g.total == 0 {
            0.
        } else {
            g.done as f32 / g.total as f32
        };
        let bar = div()
            .w(px(48.))
            .h(px(4.))
            .flex_none()
            .rounded_full()
            .bg(color(BORDER_STRONG))
            .child(
                div()
                    .h_full()
                    .w(px(48. * share))
                    .rounded_full()
                    .bg(color(DONE)),
            );
        let k = key.clone();
        let mut parts = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .text_xs()
            .text_color(color(TEXT_4))
            .child(
                div()
                    .debug_selector(move || format!("project-progress-{k}"))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(bar)
                    .child(format!("{}/{}", g.done, g.total)),
            );
        let (health, c) = if g.update_missing {
            ("Update missing", TEXT_3)
        } else {
            match g.update.as_ref().map(|u| u.health) {
                Some(livtk_core::Health::OnTrack) => ("On track", REVIEW),
                Some(livtk_core::Health::AtRisk) => ("At risk", URGENT),
                Some(livtk_core::Health::OffTrack) => ("Off track", RED),
                None => ("", TEXT_4),
            }
        };
        if !health.is_empty() {
            let k = key.clone();
            parts =
                parts.child(tag(health, c).debug_selector(move || format!("project-health-{k}")));
        }
        if let Some(t) = &g.target {
            let k = key.clone();
            parts = parts.child(
                div()
                    .debug_selector(move || format!("project-target-{k}"))
                    .child(t.clone()),
            );
        }
        let waiting: usize = g.waiting.iter().map(|w| w.count).sum();
        if waiting > 0 {
            let k = key.clone();
            parts = parts.child(
                icon_tag(Icon::You, &format!("{waiting} waiting on you"), ACCENT)
                    .debug_selector(move || format!("project-waiting-{k}")),
            );
        }
        Some(parts)
    }

    pub(super) fn row(
        &self,
        r: &Summary,
        selected: bool,
        leaving: Option<Instant>,
        inbox: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = r.id;
        let flash = self.model.flashed.get(&id).copied();
        let blocked = r.blocked && !r.kind.is_closed(r.state);
        let (own, moving) = self.highlight(&r.key, selected);
        let mut row = div()
            .id(SharedString::from(format!("row-{}", r.key)))
            .relative()
            .w_full()
            .h(px(ROW))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(color(CONTENT))
            .when(own, |d| d.bg(color(SELECTED)))
            .when(!selected, |d| d.hover(|s| s.bg(color(HOVER))))
            .text_color(color(if blocked { TEXT_3 } else { TEXT_2 }))
            .children(moving.map(|(from, to, epoch)| glide_highlight(from, to, epoch)));
        // Every row leads with the same slot, so keys, states and titles line up whatever the
        // kind: a task's priority, else the kind's icon.
        row = if r.kind == Kind::Task {
            row.child(self.priority_button(Menu::RowPriority(id), id, r.priority, false, cx))
        } else {
            row.child(kind_slot(r.kind))
        };
        let key = r.key.clone();
        row = row
            .child(
                div()
                    .debug_selector(move || format!("key-{key}"))
                    .w(px(62.))
                    .flex_none()
                    .text_color(color(TEXT_4))
                    .child(r.key.clone()),
            )
            .child(self.state_button(Menu::RowState(id), id, r.kind, r.state, false, cx))
            .children(self.model.depth().get(&id).map(|&d| {
                // Under its parent: indented by its depth, led by the turn.
                div().flex_none().pl(px((d - 1) as f32 * 16.)).child(icon(
                    Icon::SubTask,
                    14.,
                    TEXT_4,
                ))
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(color(if blocked { TEXT_3 } else { TEXT }))
                    .child(r.title.clone()),
            )
            .children(self.model.parent_key(r).map(parent_chip))
            .children(self.sub_task_count(id))
            .children(self.project_parts(r));
        if blocked {
            row = row.child(icon_tag(Icon::Blocked, "Blocked", TEXT_3));
        }
        if r.standing {
            row = row.child(icon_tag(Icon::Standing, "Standing rule", DONE));
        }
        if inbox {
            let label = match r.kind {
                Kind::Task => "Acknowledge",
                Kind::Lesson => "Ratify",
                Kind::Decision => "Accept",
                Kind::Project => "Commit",
                _ => "",
            };
            row = row.child(tag(label, ACCENT));
            if selected {
                row = row.child(kbd("A"));
            }
        }
        row = row.child(
            div()
                .w(px(60.))
                .flex_none()
                .text_xs()
                .text_color(color(TEXT_4))
                .flex()
                .justify_end()
                .child(ago(&r.updated)),
        );
        let dragged = Dragged::of(r);
        let row = row
            .on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                this.model.select(id);
                if e.click_count() >= 2 {
                    this.model.open_detail();
                }
                cx.notify();
            }));
        match leaving {
            Some(at) => {
                let started = at.elapsed().as_secs_f32() / FADE.as_secs_f32();
                row.with_animation(
                    SharedString::from(format!("leave-{}", r.key)),
                    Animation::new(FADE).with_easing(ease_in_out),
                    move |el, t| {
                        let t = (started + t).min(1.0);
                        el.opacity(1.0 - t).h(px(ROW * (1.0 - t)))
                    },
                )
                .into_any_element()
            }
            None if selected => row
                .with_animation(
                    SharedString::from(format!("select-{}", r.key)),
                    Animation::new(Duration::from_millis(120)).with_easing(ease_out_quint()),
                    |el, t| el.bg(with_alpha(SELECTED, t)),
                )
                .into_any_element(),
            None => flashing(row, &r.key, flash),
        }
    }

    pub(super) fn board(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.model.selected().map(|r| r.id);
        // The wheel scrolls a column down; across, only a sideways swipe or Shift+wheel.
        let mut cols = div()
            .id("board")
            .debug_selector(|| "board".into())
            .size_full()
            .overflow_x_scroll()
            .restrict_scroll_to_axis()
            .track_scroll(&self.board_scroll)
            .flex()
            .gap(px(6.))
            .p_2();
        for (key, rows) in self.model.columns() {
            let (mark, label) = group_label(key, Kind::Task, self.model.project_names());
            let mut col = div()
                .id(SharedString::from(format!("col-{key:?}")))
                .debug_selector(move || match key {
                    GroupKey::State(s) => format!("column-{}", s.as_str()),
                    GroupKey::Priority(p) => format!("column-priority-{p}"),
                    GroupKey::Project(p) => format!(
                        "column-project-{}",
                        p.map_or_else(|| "none".to_owned(), |id| id.to_string())
                    ),
                    GroupKey::All => "column-all".to_owned(),
                })
                .w(px(BOARD_COLUMN))
                .flex_none()
                .h_full()
                .flex()
                .flex_col()
                .gap(px(6.))
                .p(px(6.))
                .rounded_xl()
                .bg(color(SURFACE))
                .border_1()
                .border_color(color(BORDER))
                .overflow_y_scroll()
                .restrict_scroll_to_axis()
                .track_scroll(&self.column_scroll(key))
                .drag_over::<Dragged>(move |style, d, _, _| {
                    if d.fits(key) {
                        style
                            .bg(with_alpha(ACCENT, 0.12))
                            .border_color(color(ACCENT))
                    } else {
                        style
                    }
                })
                .on_drop(cx.listener(move |this, d: &Dragged, _, cx| {
                    this.model.drop_on(d.id, key);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap_2()
                        .px_1()
                        .h(px(24.))
                        .child(mark)
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(color(TEXT))
                                .child(label),
                        )
                        .child(
                            div()
                                .text_color(color(TEXT_4))
                                .child(format!("{}", rows.len())),
                        ),
                );
            for r in rows {
                let id = r.id;
                let sel = Some(id) == selected;
                let blocked = r.blocked && !r.kind.is_closed(r.state);
                let flash = self.model.flashed.get(&id).copied();
                let card = div()
                    .id(SharedString::from(format!("card-{}", r.key)))
                    .debug_selector({
                        let key = r.key.clone();
                        move || format!("card-{key}")
                    })
                    // Cards keep their height, so a full column overflows and scrolls. Two rows:
                    // its icons (with their menus) leading its key, then its title.
                    .flex_none()
                    .p_2()
                    .rounded_lg()
                    .bg(color(if sel { SELECTED } else { CONTENT }))
                    .border_1()
                    .border_color(color(if sel { BORDER_STRONG } else { BORDER }))
                    .hover(|s| s.border_color(color(BORDER_STRONG)))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(color(TEXT_4))
                            .child(self.priority_button(
                                Menu::RowPriority(id),
                                id,
                                r.priority,
                                false,
                                cx,
                            ))
                            .child(self.state_button(
                                Menu::RowState(id),
                                id,
                                Kind::Task,
                                r.state,
                                false,
                                cx,
                            ))
                            .child(div().pl_1().child(r.key.clone()))
                            .children(self.model.parent_of(r).map(parent_chip))
                            .child(div().flex_1())
                            .children(self.sub_task_count(id))
                            .when(blocked, |d| {
                                d.child(icon(Icon::Blocked, 12., TEXT_4)).child("Blocked")
                            }),
                    )
                    .child(
                        div()
                            .px_1()
                            .line_clamp(2)
                            .text_color(color(if blocked { TEXT_3 } else { TEXT }))
                            .child(r.title.clone()),
                    )
                    .on_drag(Dragged::of(r), |d, _, _, cx| cx.new(|_| d.clone()))
                    .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                        this.model.select(id);
                        if e.click_count() >= 2 {
                            this.model.open_detail();
                        }
                        cx.notify();
                    }));
                col = col.child(flashing(card, &r.key, flash));
            }
            // Full height behind the floating dock: cards pass under it, and the room at the
            // end lets the last one rise clear of it.
            cols = cols.child(col.child(div().h(px(DOCK_SPACE)).flex_none()));
        }
        // Shift+wheel scrolls the board across, before any column sees it: a listener in the
        // capture phase over the board's area, which then stops the event.
        let handle = self.board_scroll.clone();
        let shift_wheel = canvas(
            |_, _, _| (),
            move |area, (), window, _| {
                window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture
                        || !e.modifiers.shift
                        || !area.contains(&e.position)
                    {
                        return;
                    }
                    let d = e.delta.pixel_delta(px(20.));
                    let along = if f32::from(d.x) != 0. { d.x } else { d.y };
                    let max = f32::from(handle.max_offset().x);
                    let mut offset = handle.offset();
                    offset.x = px((f32::from(offset.x) + f32::from(along)).clamp(-max, 0.));
                    handle.set_offset(offset);
                    cx.stop_propagation();
                    window.refresh();
                });
            },
        )
        // Pinned to the corner: an absolute element without offsets sits where the flow would
        // have put it, here below the board.
        .absolute()
        .inset_0();
        div().relative().size_full().child(cols).child(shift_wheel)
    }

    // ---- rendering: the detail panel ----------------------------------------------------------
}

/// A non-task row's leading slot: its kind's icon, as wide as a task's priority button.
pub(super) fn kind_slot(kind: Kind) -> Div {
    let glyph = kind_icon(kind);
    div()
        .w(px(LEAD))
        .h(px(22.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .child(icon(glyph, 14., TEXT_4).debug_selector(|| "kind-icon".into()))
}

/// A group's mark and name, for list bands and board columns alike; a project's by its key and
/// title, from `names`.
pub(super) fn group_label(
    key: GroupKey,
    kind: Kind,
    names: &[(Uuid, livtk_core::Brief)],
) -> (Div, String) {
    match key {
        GroupKey::State(s) => (state_mark(kind, s), title_case(s.as_str())),
        GroupKey::Priority(p) => (
            priority_mark(p),
            PRIORITY_NAMES[usize::from(p.clamp(1, 4) - 1)].to_owned(),
        ),
        GroupKey::Project(Some(id)) => (
            kind_slot(Kind::Project),
            names.iter().find(|(p, _)| *p == id).map_or_else(
                || "A project".to_owned(),
                |(_, b)| format!("{}  {}", b.key, b.title),
            ),
        ),
        GroupKey::Project(None) => (div(), "No project".to_owned()),
        GroupKey::All => (div(), "All".to_owned()),
    }
}

/// A sub-task shown away from its parent: the parent's key, quietly.
pub(super) fn parent_chip(key: &str) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .text_color(color(TEXT_4))
        .child(icon(Icon::SubTask, 12., TEXT_4))
        .child(key.to_owned())
}
