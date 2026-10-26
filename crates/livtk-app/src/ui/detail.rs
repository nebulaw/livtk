//! The detail panel: a node's title, body, state, links and docs, and a project's own section.

use super::text::{Blocks, InlineEdit, TextRole};
use super::*;

impl LivtkApp {
    /// The Claude Code sessions that worked on a node, each a link: a click reads it in Agents,
    /// from the node's first event in it.
    pub(super) fn task_sessions(
        &self,
        uses: &[livtk_core::SessionUse],
        cx: &mut Context<Self>,
    ) -> Div {
        // One line a session: its actors together, from the first event of any of them.
        let mut by_session: Vec<(String, Vec<String>, String)> = Vec::new();
        for u in uses {
            match by_session.iter_mut().find(|(s, _, _)| *s == u.session) {
                Some((_, by, first)) => {
                    by.push(who(&u.by).to_owned());
                    if u.first < *first {
                        first.clone_from(&u.first);
                    }
                }
                None => by_session.push((
                    u.session.clone(),
                    vec![who(&u.by).to_owned()],
                    u.first.clone(),
                )),
            }
        }
        let names = self.model.session_rows();
        let mut list = div().flex().flex_col().gap_1();
        for (session, by, first) in by_session {
            let name = names.iter().find(|r| r.id == session).map_or_else(
                || livtk_core::format::session(&session).to_owned(),
                |r| r.name.clone(),
            );
            list = list.child(
                link_row(format!("task-session-{session}"))
                    .hover(|h| h.bg(color(HOVER)))
                    .child(icon(Icon::Sessions, 13., TEXT_4))
                    .child(link_title(name))
                    .child(link_aside(format!("{} · {}", by.join(", "), ago(&first))))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.go(View::Sessions, window, cx);
                        this.model.read_session(&session, Some(first.clone()));
                        cx.notify();
                    })),
            );
        }
        list
    }

    /// The open node's title: read, or edited in place.
    pub(super) fn detail_title(&self, n: &livtk_core::Node, cx: &mut Context<Self>) -> AnyElement {
        let edit = self.part_edit(&Part::Title);
        if let Some(Editing { field, .. }) = self.editing_of(n)
            && self.editing.as_ref().is_some_and(|e| e.part == Part::Title)
        {
            return edit.editing(field, &self.read_heights, cx);
        }
        edit.reading_text(
            n.title.clone(),
            |this, at, w, cx| this.start_edit(Part::Title, at, w, cx),
            &self.read_heights,
            cx,
        )
    }

    /// The open node's body, block by block: each read as markdown, one edited in place.
    pub(super) fn detail_body(&self, n: &livtk_core::Node, cx: &mut Context<Self>) -> AnyElement {
        let editing = self.editing_of(n).and_then(|e| match &e.part {
            Part::Block(at) => Some((at, &e.field)),
            Part::Title => None,
        });
        div()
            .debug_selector(|| "detail-body".into())
            .child(self.body_blocks().render(
                &n.body,
                editing,
                |this, at, picked, w, cx| this.start_edit(Part::Block(at), picked, w, cx),
                &self.block_views,
                &self.read_heights,
                cx,
            ))
            .into_any_element()
    }

    /// The edit under way on `n`, if any.
    fn editing_of(&self, n: &livtk_core::Node) -> Option<&Editing> {
        self.editing.as_ref().filter(|e| e.node == n.id)
    }

    /// How a body's blocks read and open: a double-click, as a word is picked, so the editor
    /// opens with the word picked in the source.
    fn body_blocks(&self) -> Blocks {
        Blocks {
            prefix: "body",
            placeholder: BODY_PLACEHOLDER,
            width: BODY_DIAGRAM_W,
        }
    }

    /// How `part` of a node's panel is edited in place.
    pub(super) fn part_edit(&self, part: &Part) -> InlineEdit {
        match part {
            Part::Title => InlineEdit::new(TextRole::Title, "detail-title", "title-editor")
                .placeholder("Title"),
            Part::Block(at) => self.body_blocks().edit(at),
        }
    }

    /// A node's state mark, with its `name` in the panel, that opens the states of its kind, as
    /// Linear's status menu: the moves it can make enabled, the human's own decisions in the
    /// accent, the rest greyed so the whole flow shows. A press here neither selects the row nor
    /// starts a drag.
    pub(super) fn state_button(
        &self,
        menu: Menu,
        id: Uuid,
        kind: Kind,
        state: State,
        name: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        if !has_flow(kind) {
            return div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .child(state_mark(kind, state))
                .when(name, |d| d.child(state_name(state)));
        }
        let open = self.menu == Some(menu);
        let selector = match menu {
            Menu::DetailState => "detail-state".to_owned(),
            _ => format!("state-of-{id}"),
        };
        let button = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .h(px(22.))
            .flex()
            .flex_none()
            .items_center()
            .rounded_lg()
            .when(name, |b| b.px(px(4.)).gap_2())
            .when(!name, |b| b.w(px(22.)).justify_center())
            .when(open, |b| b.bg(color(SELECTED)))
            .when(!open, |b| b.hover(|h| h.bg(color(HOVER))))
            .child(state_mark(kind, state))
            .when(name, |b| b.child(state_name(state)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_menu(menu, window, cx);
                }),
            );
        div().relative().flex_none().child(button).when(open, |d| {
            let mut panel = menu_panel(220.).debug_selector(|| "set-state-menu".into());
            let all = steps_with(kind, state, self.model.gates());
            let mut side = false;
            for s in &all {
                if !s.main && !side {
                    panel = panel.child(menu_divider());
                    side = true;
                }
                panel = panel.child(self.state_option(
                    id,
                    kind,
                    s,
                    s.to.clone(),
                    state_name(s.state),
                    cx,
                ));
                if let Some(standing) = &s.standing {
                    panel = panel.child(self.state_option(
                        id,
                        kind,
                        s,
                        Some(standing.clone()),
                        "Ratified, as a standing rule",
                        cx,
                    ));
                }
            }
            d.child(self.popover(menu, false, 26., panel, cx))
        })
    }

    /// A state in the state menu: enabled when `to` is a move the node can make.
    pub(super) fn state_option(
        &self,
        id: Uuid,
        kind: Kind,
        s: &Step,
        to: Option<Choice>,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let standing = to.as_ref().is_some_and(|c| c.always);
        let row_id = format!(
            "set-state-{}{}",
            s.state.as_str(),
            if standing { "-standing" } else { "" }
        );
        let decides = to.as_ref().is_some_and(|c| c.decides);
        let row = div()
            .id(SharedString::from(row_id.clone()))
            .debug_selector(move || row_id.clone())
            .h(px(30.))
            .px(px(6.))
            .flex()
            .items_center()
            .gap_2()
            .rounded_lg()
            .child(flow_mark(kind, s))
            .child(label)
            .child(check(s.current && !standing));
        match to {
            Some(choice) => row
                .text_color(color(if decides { ACCENT_HOVER } else { TEXT_2 }))
                .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.model.choose_on(id, &choice);
                    this.close_menu(window, cx);
                })),
            // Where it is now: highlighted as well as checked.
            None if s.current => row
                .bg(color(SELECTED))
                .text_color(color(TEXT))
                .font_weight(FontWeight::MEDIUM),
            // Not a move from here: shown, so the flow reads whole, but greyed.
            None => row.text_color(color(TEXT_4)).opacity(0.55),
        }
    }

    /// The node's states as a flow (see `state_flow`): a click makes the move, and ratifying a
    /// lesson opens the standing-rule menu first.
    pub(super) fn state_flow(
        &self,
        id: Uuid,
        kind: Kind,
        state: State,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let steps = steps_with(kind, state, self.model.gates());
        let arrows = arrows_with(kind, state, self.model.gates());
        let standing = (self.menu == Some(Menu::Standing))
            .then(|| steps.iter().find(|s| s.standing.is_some()))
            .flatten()
            .map(|s| {
                let panel = self.standing_menu(id, s, cx);
                self.popover(Menu::Standing, false, 30., panel, cx)
                    .into_any_element()
            });
        let this = cx.entity().downgrade();
        let choose = move |choice: &Choice, _: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |app, cx| {
                app.model.choose_on(id, choice);
                cx.notify();
            });
        };
        let this = cx.entity().downgrade();
        let standing_open = move |window: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |app, cx| app.toggle_menu(Menu::Standing, window, cx));
        };
        state_flow::StateFlow::new(id, kind, steps, arrows, width, self.flow.clone(), choose)
            .on_standing(standing_open)
            .standing_menu(standing)
            .into_any_element()
    }

    /// Ratify, or ratify as a standing rule shown at every session start.
    pub(super) fn standing_menu(&self, id: Uuid, s: &Step, cx: &mut Context<Self>) -> Div {
        let mut panel = menu_panel(240.).debug_selector(|| "standing-menu".into());
        for (row_id, label, choice) in [
            ("ratify-plain", "Ratify", s.to.clone()),
            (
                "ratify-standing",
                "Ratify as a standing rule",
                s.standing.clone(),
            ),
        ] {
            let Some(choice) = choice else { continue };
            panel = panel.child(
                option_row(row_id.into())
                    .debug_selector(move || row_id.into())
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.model.choose_on(id, &choice);
                        this.close_menu(window, cx);
                    })),
            );
        }
        panel
    }

    /// "+ Add link": the relations the open node can gain, each opening the palette to pick
    /// the other end.
    pub(super) fn add_link_button(&self, kind: Kind, cx: &mut Context<Self>) -> Div {
        let open = self.menu == Some(Menu::AddLink);
        let button = div()
            .id("add-link")
            .debug_selector(|| "add-link".into())
            .group("add-link")
            .h(px(24.))
            .px_2()
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_lg()
            .text_xs()
            .text_color(color(if open { TEXT } else { TEXT_3 }))
            .when(open, |b| b.bg(color(SELECTED)))
            .when(!open, |b| {
                b.hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
            })
            .child(if open {
                icon(Icon::Link, 14., TEXT)
            } else {
                icon_hover(Icon::Link, 14., TEXT_3, "add-link", TEXT)
            })
            .child("Add link")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.toggle_menu(Menu::AddLink, window, cx)),
            );
        div().relative().child(button).when(open, |d| {
            let mut panel = menu_panel(200.).debug_selector(|| "add-link-menu".into());
            for (i, choice) in link_choices(kind, &self.model.declared_rels())
                .into_iter()
                .enumerate()
            {
                let row_id = format!("link-choice-{i}");
                panel = panel.child(
                    option_row(SharedString::from(row_id.clone()))
                        .debug_selector(move || row_id.clone())
                        .child(choice.label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pick_link(choice, window, cx)
                        })),
                );
            }
            let panel = panel.child(menu_divider()).child(
                option_row("link-choice-doc".into())
                    .debug_selector(|| "link-choice-doc".into())
                    .child(icon(Icon::Docs, 14., TEXT_3))
                    .child("A doc…")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_menu(window, cx);
                        this.help = false;
                        this.model.open_doc_picker();
                        if this.model.palette.is_some() {
                            this.reset_palette_input(window, cx);
                        }
                        cx.notify();
                    })),
            );
            d.child(self.popover(Menu::AddLink, true, 28., panel, cx))
        })
    }

    /// One group of the open node's links: a click on one goes to it, its × removes the link.
    pub(super) fn link_group(&self, g: &LinkGroup, cx: &mut Context<Self>) -> Div {
        let mut out = div().flex().flex_col().gap_1().child(group_label(g.label));
        let (rel, outgoing) = (g.rel, g.outgoing);
        let group = g.label.to_lowercase().replace(' ', "-");
        for b in &g.briefs {
            let key = b.key.clone();
            let unlink = unlink_button(
                format!("unlink-{group}-{key}"),
                move |this, _| this.model.unlink(rel, outgoing, &key),
                cx,
            );
            out = out.child(self.node_ref(
                format!("link-{group}-{}", b.key),
                &b.key,
                b.kind,
                b.state,
                &b.title,
                OpenIn::View,
                Some(unlink),
                cx,
            ));
        }
        out
    }

    /// The node's docs, as a link group: each by its title (its path struck when it is gone), a
    /// click opening it in the Docs view, its × taking the link away.
    pub(super) fn doc_group(
        &self,
        key: &str,
        docs: &[(String, Option<String>)],
        cx: &mut Context<Self>,
    ) -> Div {
        let mut out = div()
            .debug_selector(|| "detail-docs".into())
            .flex()
            .flex_col()
            .gap_1()
            .child(group_label("Docs"));
        for (path, title) in docs {
            let gone = title.is_none();
            let (open, unlink_path, node) = (path.clone(), path.clone(), key.to_owned());
            let unlink = format!("unlink-doc-{path}");
            let row = format!("detail-doc-{path}");
            out = out.child(
                link_row(row)
                    .when(!gone, |d| d.hover(|h| h.bg(color(HOVER))))
                    .child(icon(Icon::Docs, 13., TEXT_4))
                    .child(
                        link_title(title.clone().unwrap_or_else(|| path.clone()))
                            .when(gone, |d| d.text_color(color(TEXT_4)).line_through()),
                    )
                    .child(link_aside(if gone {
                        "missing".to_owned()
                    } else {
                        path.clone()
                    }))
                    .child(unlink_button(
                        unlink,
                        move |this, _| this.model.link_doc(&node, &unlink_path, false),
                        cx,
                    ))
                    .when(!gone, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            this.model.open_doc(&open);
                            cx.notify();
                        }))
                    }),
            );
        }
        out
    }

    /// The panel shows another node (or none): put the note field's draft by the node it was
    /// written for, and bring back the new one's own.
    pub(super) fn track_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = self
            .model
            .detail
            .as_ref()
            .zip(self.model.project())
            .map(|(d, p)| (p.dir.clone(), d.node.id));
        if open == self.note_for {
            return;
        }
        let draft = self.note.read(cx).value().to_string();
        if let Some(was) = self.note_for.take() {
            if draft.is_empty() {
                self.note_drafts.remove(&was);
            } else {
                self.note_drafts.insert(was, draft);
            }
        }
        let back = open
            .as_ref()
            .and_then(|key| self.note_drafts.remove(key))
            .unwrap_or_default();
        self.note.update(cx, |s, cx| s.set_value(back, window, cx));
        self.note_for = open;
    }

    pub(super) fn track_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match (&self.model.detail, self.last_detail.is_some()) {
            (Some(d), was_open) => {
                if !was_open {
                    self.detail_epoch += 1;
                    self.detail_leaving = None;
                }
                self.last_detail = Some(d.clone());
            }
            (None, true) => {
                self.detail_epoch += 1;
                self.detail_leaving = self.last_detail.take();
                let epoch = self.detail_epoch;
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(SLIDE_OUT).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.detail_epoch == epoch {
                            this.detail_leaving = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            (None, false) => {}
        }
    }

    /// The detail panel, sliding in when it opens and out when it closes. Its width animates,
    /// so the list beside it makes room smoothly instead of jumping.
    pub(super) fn detail(&mut self, width: f32, cx: &mut Context<Self>) -> Option<AnyElement> {
        // The Agents or Sessions panel: a session, or an agent's page.
        if matches!(self.model.view(), View::Agents | View::Sessions) {
            return self.agents_panel(width, cx);
        }
        // A node's detail opened another way is not the panel's: the panel keeps only a doc's.
        let side = self.model.detail_mode == crate::model::DetailMode::Side;
        let (d, closing) = match (&self.model.detail, &self.detail_leaving) {
            (Some(_), _) | (None, Some(_)) if !side => {
                return self.doc_panel_open().then(|| {
                    div()
                        .debug_selector(|| "detail-panel".into())
                        .flex_none()
                        .h_full()
                        .overflow_hidden()
                        .child(self.doc_panel(width, cx))
                        .into_any_element()
                });
            }
            (Some(d), _) => (d.clone(), false),
            (None, Some(d)) => (d.clone(), true),
            // A doc being read: its details, when nothing else is open in the panel.
            (None, None) if self.doc_panel_open() => {
                return Some(
                    div()
                        .debug_selector(|| "detail-panel".into())
                        .flex_none()
                        .h_full()
                        .overflow_hidden()
                        .child(self.doc_panel(width, cx))
                        .into_any_element(),
                );
            }
            (None, None) => return None,
        };
        let panel = self.detail_panel(&d, width, cx);
        let (id, duration) = if closing {
            (format!("detail-out-{}", self.detail_epoch), SLIDE_OUT)
        } else {
            (format!("detail-in-{}", self.detail_epoch), SLIDE)
        };
        let easing: fn(f32) -> f32 = if closing { ease_in_out } else { ease_out_cubic };
        Some(
            div()
                .debug_selector(|| "detail-panel".into())
                .flex_none()
                .h_full()
                .overflow_hidden()
                .child(panel)
                .with_animation(
                    SharedString::from(id),
                    Animation::new(duration).with_easing(easing),
                    move |el, t| {
                        let open = if closing { 1.0 - t } else { t };
                        el.w(px(width * open)).opacity(0.3 + 0.7 * open)
                    },
                )
                .into_any_element(),
        )
    }

    /// The Links section: each group of links, the docs, then the sessions that worked on it.
    fn links_section(&mut self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let n = &d.node;
        let mut links = div()
            .debug_selector(|| "detail-links".into())
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(TEXT_4))
                            .child("Links"),
                    )
                    .child(div().flex_1())
                    .child(self.add_link_button(n.kind, cx)),
            );
        for g in &d.link_groups {
            links = links.child(self.link_group(g, cx));
        }
        if !d.docs.is_empty() {
            links = links.child(self.doc_group(&n.key, &d.docs, cx));
        }
        // The sessions that worked on it, after its other links.
        if !d.sessions.is_empty() {
            self.load_transcripts(cx);
            links = links.child(
                div()
                    .debug_selector(|| "detail-sessions".into())
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(group_label("Sessions"))
                    .child(self.task_sessions(&d.sessions, cx)),
            );
        }
        links
    }

    pub(super) fn detail_panel(
        &mut self,
        d: &Detail,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let n = &d.node;
        let props = self.props(d, cx);
        let links = self.links_section(d, cx);
        // The panel's content: its width less its padding either side.
        let flow = self.state_flow(n.id, n.kind, n.state, width - 40., cx);

        let title = self.detail_title(n, cx);
        let body = self.detail_body(n, cx);
        let timeline = self.timeline_view(d, cx);
        div()
            .id(SharedString::from(format!("detail-{}", n.key)))
            .debug_selector(|| "detail-body-panel".into())
            .w(px(width))
            .flex_none()
            .h_full()
            // The window's black shows through, as behind the sidebar.
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(46.))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(color(BORDER))
                    .child(div().text_color(color(TEXT_3)).child(n.key.clone()))
                    // Its run, right after its key: where it stands, and Start or Stop.
                    .children(self.run_row(d, cx))
                    .child(div().flex_1())
                    .child(self.detail_mode_button(cx))
                    .child(
                        div()
                            .id("close-detail")
                            .debug_selector(|| "close-detail".into())
                            .group("close-detail")
                            .size(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .hover(|s| s.bg(color(HOVER)))
                            .child(icon_hover(
                                if self.model.detail_mode == crate::model::DetailMode::Side {
                                    Icon::DockRight
                                } else {
                                    Icon::Close
                                },
                                16.,
                                TEXT_4,
                                "close-detail",
                                TEXT_2,
                            ))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Close · Esc")
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.model.close_detail();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("detail-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(title)
                            .child(meta_line(d, cx)),
                    )
                    .when(has_flow(n.kind) && !self.model.prefs.hide_state_flow, |d| {
                        d.child(flow)
                    })
                    .child(props)
                    .children(project_section(d))
                    // What it waits for and what it blocks come before what it says.
                    .child(links)
                    .child(body)
                    .child(section_label("Activity"))
                    .child(timeline),
            )
    }

    /// The note bar's one field, as Linear's comment box: its icon, the text and a send button
    /// in one rounded card. The send button lights once there is something to send and sends as
    /// Enter does; while the field has the keys, a hint under the text says how.
    pub(super) fn note_composer(&self, cx: &mut Context<Self>) -> Div {
        let focused = self.note_focused;
        let field = self.note.read(cx);
        let ready = !field.value().trim().is_empty();
        let note = self.note.clone();
        // Its fill eases in as there comes to be something to send, and out as there is not; a
        // press shrinks it a little in its slot, and it springs back.
        let (from, to) = if ready {
            (SURFACE, ACCENT)
        } else {
            (ACCENT, SURFACE)
        };
        let button = div()
            .id("note-send")
            .debug_selector(|| "note-send".into())
            .size(px(SEND))
            .flex()
            .items_center()
            .justify_center()
            .rounded_lg()
            .when(ready, |d| {
                d.hover(|h| h.bg(color(ACCENT_HOVER)))
                    .active(|a| a.size(px(SEND_PRESSED)))
                    .child(icon(Icon::Send, 14., TEXT))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.send_note(&note, window, cx);
                    }))
            })
            .when(!ready, |d| d.child(icon(Icon::Send, 14., TEXT_4)))
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new("Send · Enter").build(window, cx)
            })
            .with_animation(
                SharedString::from(format!("note-send-{ready}")),
                Animation::new(Duration::from_millis(SEND_EASE)).with_easing(ease_out_quint()),
                move |el, t| el.bg(blend(from, to, t)),
            );
        let send = div()
            .flex_none()
            .self_end()
            .size(px(SEND))
            .flex()
            .items_center()
            .justify_center()
            .child(button);
        div()
            .debug_selector(|| "note-composer".into())
            .flex()
            .items_start()
            .gap_2()
            .pl(px(6.))
            .pr(px(6.))
            .py(px(6.))
            .rounded_xl()
            .bg(color(SURFACE))
            .border_1()
            .border_color(color(if focused { ACCENT } else { BORDER }))
            .when(!focused, |d| {
                d.hover(|h| h.border_color(color(BORDER_STRONG)))
            })
            .child(
                div().flex_1().min_w_0().child(indented(
                    div().child(
                        Textarea::new(&self.note)
                            .appearance(false)
                            .xsmall()
                            .text_sm(),
                    ),
                    &self.note,
                )),
            )
            .child(send)
    }

    // ---- rendering: overlays ------------------------------------------------------------------
}

/// A project's own section in its panel: how far it has come, what waits on the human, and
/// how it stands, from its updates, newest first.
pub(super) fn project_section(d: &Detail) -> Option<Div> {
    let (g, updates, flow) = d.project.as_ref()?;
    let share = if g.total == 0 {
        0.
    } else {
        g.done as f32 / g.total as f32
    };
    let mut out = div()
        .debug_selector(|| "project-section".into())
        .flex()
        .flex_col()
        .gap_3()
        .child(section_label("Progress"))
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .text_sm()
                .text_color(color(TEXT_2))
                .child(
                    div()
                        .flex_1()
                        .h(px(6.))
                        .rounded_full()
                        .bg(color(BORDER_STRONG))
                        .child(
                            div()
                                .h_full()
                                .w(relative(share))
                                .rounded_full()
                                .bg(color(DONE)),
                        ),
                )
                .child(
                    div()
                        .debug_selector(|| "project-done".into())
                        .flex_none()
                        .child(format!("{} of {} tasks done", g.done, g.total)),
                ),
        );
    for w in &g.waiting {
        let state = w.state;
        out = out.child(
            div()
                .debug_selector(move || format!("project-waits-{}", state.as_str()))
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(color(TEXT_2))
                .child(icon(Icon::You, 14., ACCENT))
                .child(format!(
                    "{} {} waiting on you; {} the longest, since {}",
                    w.count,
                    state_name(state).to_lowercase(),
                    w.oldest,
                    ago(&w.since)
                )),
        );
    }
    if g.update_missing {
        out = out.child(
            div()
                .debug_selector(|| "project-update-missing".into())
                .text_sm()
                .text_color(color(TEXT_3))
                .child("Update missing: none for a week while active."),
        );
    }
    out = out.children(flow_section(flow));
    if !updates.is_empty() {
        out = out.child(section_label("Updates"));
        for (i, u) in updates.iter().enumerate() {
            let (label, c) = match u.health {
                livtk_core::Health::OnTrack => ("On track", REVIEW),
                livtk_core::Health::AtRisk => ("At risk", URGENT),
                livtk_core::Health::OffTrack => ("Off track", RED),
            };
            out = out.child(
                div()
                    .debug_selector(move || format!("project-update-{i}"))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(tag(label, c))
                            .child(div().text_xs().text_color(color(TEXT_4)).child(ago(&u.at))),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(color(TEXT_2))
                            .child(u.text.clone()),
                    ),
            );
        }
    }
    Some(out)
}

/// A project's flow in its panel: its lead time, and the median time its tasks spend in each
/// state, as one quiet bar per state (one series, one hue; the numbers in text).
pub(super) fn flow_section(f: &livtk_core::Flow) -> Option<Div> {
    if f.states.is_empty() && f.lead.is_none() {
        return None;
    }
    let longest = f
        .states
        .iter()
        .map(|t| t.median_secs)
        .max()
        .unwrap_or(0)
        .max(1);
    let mut out = div()
        .debug_selector(|| "project-flow".into())
        .flex()
        .flex_col()
        .gap_2()
        .child(section_label("Flow"));
    if let Some(l) = &f.lead {
        out = out.child(
            div()
                .debug_selector(|| "project-lead".into())
                .text_sm()
                .text_color(color(TEXT_2))
                .child(format!(
                    "Lead time {}{}",
                    duration(l.secs),
                    if l.finished.is_some() { "" } else { " so far" }
                )),
        );
    }
    for t in &f.states {
        let state = t.state;
        let share = t.median_secs as f32 / longest as f32;
        out = out.child(
            div()
                .debug_selector(move || format!("flow-time-{}", state.as_str()))
                .h(px(20.))
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .child(
                    div()
                        .w(px(80.))
                        .flex_none()
                        .text_color(color(TEXT_3))
                        .child(state_name(state)),
                )
                .child(
                    div().flex_1().h(px(6.)).child(
                        div()
                            .h_full()
                            .w(relative(share.max(0.02)))
                            .rounded_r(px(4.))
                            .bg(color(ACCENT)),
                    ),
                )
                .child(
                    div()
                        .w(px(64.))
                        .flex_none()
                        .flex()
                        .justify_end()
                        .text_color(color(TEXT_2))
                        .child(duration(t.median_secs)),
                ),
        );
    }
    Some(out)
}

pub(super) fn prop_row(label: &str, value: Div) -> Div {
    // Aligned to the top: a value of several lines keeps its name beside its first line; one
    // line sits level with its name.
    div()
        .flex()
        .items_start()
        .child(
            div()
                .w(px(96.))
                .h(px(PROP_LINE))
                .flex_none()
                .flex()
                .items_center()
                .text_color(color(TEXT_4))
                .child(label.to_owned()),
        )
        .child(
            div()
                .min_h(px(PROP_LINE))
                .flex()
                .items_center()
                .child(value.text_color(color(TEXT_2))),
        )
}

/// A property's first line: its name's height, and a one-line value's.
const PROP_LINE: f32 = 26.;

/// The note's send button, at rest and pressed; how long its fill takes to ease in or out.
const SEND: f32 = 24.;
const SEND_PRESSED: f32 = 21.;
const SEND_EASE: u64 = 160;

/// A group's name in a node's links: its relation, its docs, its sessions.
fn group_label(label: &str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color(TEXT_4))
        .child(label.to_owned())
}

/// The quiet line under a node's title: when it was made and by whom, and when it last changed
/// and by whom (its last change of any kind: a move, a link, a note).
fn meta_line(d: &Detail, cx: &App) -> Div {
    let n = &d.node;
    let made = format!("created {} by {}", ago(&n.created), who(&n.created_by));
    let line = match d.history.last().filter(|e| e.at != n.created) {
        Some(e) => format!("Last update {} by {} · {made}", ago(&e.at), who(&e.by)),
        None => upper_first(&made),
    };
    TextRole::Meta
        .style()
        .frame(div(), cx)
        .debug_selector(|| "detail-meta".into())
        .child(line)
}

/// `s` with its first letter upper case.
fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// What a node's body says while it has none.
const BODY_PLACEHOLDER: &str = "Add a description…";
/// How wide a diagram in a body may draw: the panel's text, at its narrowest.
const BODY_DIAGRAM_W: f32 = 360.;
