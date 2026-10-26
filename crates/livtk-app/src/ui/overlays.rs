//! What opens over the window: the new-task form, the command palette, the help.

use super::form::{field, toggle};
use super::*;

impl LivtkApp {
    /// A modal: a dim backdrop that closes it when clicked, around a card sized to its content.
    /// `keys`: the modal itself has the keys (the shortcuts, About), rather than a field in it.
    /// `fades`: it fades in (the palette shows at once).
    pub(super) fn modal(
        &self,
        id: &'static str,
        keys: bool,
        fades: bool,
        card: Div,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .absolute()
            .inset_0()
            // Nothing behind it is hovered, scrolled or clicked while it is open.
            .occlude()
            .key_context(FORM)
            // So no key reaches the list, or a form, behind it.
            .when(keys, |d| d.track_focus(&self.modal_focus))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.escape(window, cx)))
            .on_action(cx.listener(|this, _: &input::Escape, window, cx| this.escape(window, cx)))
            .on_action(cx.listener(|this, _: &Submit, window, cx| this.submit_form(window, cx)))
            .bg(with_alpha(0x000000, 0.6))
            .flex()
            .flex_col()
            .items_center()
            // On a short window the gap above gives way first, then the card, whose content
            // scrolls inside it: a modal never runs past the window's bottom edge.
            .pb(px(16.))
            // A click outside closes it, but not the click that brings the window back from
            // another app: that one only activates it, so nothing half typed is lost.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    if !e.first_mouse {
                        this.close_top(window, cx);
                    }
                }),
            )
            .child(div().h(px(110.)).min_h(px(16.)).flex_shrink(20.))
            .child(
                card.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .min_h_0()
                    .flex_shrink(1.)
                    .overflow_hidden()
                    .rounded_xl()
                    .bg(color(SURFACE))
                    .border_1()
                    .border_color(color(BORDER_STRONG))
                    .shadow_lg(),
            )
            .with_animation(
                SharedString::from(format!("{id}-in")),
                Animation::new(Duration::from_millis(if fades { 140 } else { 0 }))
                    .with_easing(ease_out_quint()),
                |el, t| el.opacity(t),
            )
            .into_any_element()
    }

    pub(super) fn form_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.form.as_ref()?;
        let making = form.making;
        let (card_id, create_id) = (making.spec().card_id, making.spec().create_id);
        // What the kind adds: a task its priority beside the button; a project its fields
        // under the body.
        let (beside, fields): (Option<Div>, Option<Div>) =
            match &form.fields {
                Fields::Task { priority } => (
                    Some(
                        self.select(
                            Menu::Priority,
                            "priority-select",
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(priority_mark(*priority))
                                .child(PRIORITY_NAMES[usize::from((*priority).clamp(1, 4) - 1)]),
                            Self::priority_menu,
                            cx,
                        ),
                    ),
                    None,
                ),
                Fields::Project {
                    done_when,
                    target,
                    owner,
                    part_of,
                    commit,
                } => {
                    let label = |l: &'static str| {
                        div()
                            .w(px(84.))
                            .flex_none()
                            .text_xs()
                            .text_color(color(TEXT_4))
                            .child(l)
                    };
                    let line = |l: &'static str, id: &'static str, f: &Entity<InputState>| {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(label(l))
                            .child(div().flex_1().child(field(f, id)))
                    };
                    let parent = part_of
                        .as_ref()
                        .map_or_else(|| "None".to_owned(), |(k, t)| format!("{k}  {t}"));
                    let part = self.select(
                        Menu::PartOf,
                        "part-of-select",
                        div().child(parent),
                        Self::part_of_menu,
                        cx,
                    );
                    let on = *commit;
                    (
                        Some(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_xs()
                                .text_color(color(TEXT_3))
                                .child(toggle("new-project-commit".into(), on).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        if let Some(NewForm {
                                            fields: Fields::Project { commit, .. },
                                            ..
                                        }) = &mut this.form
                                        {
                                            *commit = !*commit;
                                        }
                                        cx.notify();
                                    }),
                                ))
                                .child("Commit to it now"),
                        ),
                        Some(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(line("Done when", "new-project-done", done_when))
                                .child(line("Target", "new-project-target", target))
                                .child(line("Owner", "new-project-owner", owner))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(label("Part of"))
                                        .child(part),
                                ),
                        ),
                    )
                }
            };
        let create = div()
            .id(create_id)
            .debug_selector(move || create_id.into())
            .flex_none()
            .h(px(28.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .rounded_lg()
            .bg(color(ACCENT))
            .text_color(color(TEXT))
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.bg(color(ACCENT_HOVER)))
            .child(making.spec().create)
            .child(
                div()
                    .text_xs()
                    .text_color(with_alpha(TEXT, 0.7))
                    .child(keys::SUBMIT),
            )
            .on_click(cx.listener(|this, _, window, cx| this.submit_form(window, cx)));
        let card = div()
            .debug_selector(move || card_id.into())
            .w(px(640.))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(color(TEXT_3))
                    .child(
                        self.model
                            .project()
                            .map(|p| p.name.clone())
                            .unwrap_or_default(),
                    )
                    .child("›")
                    .child(making.spec().title)
                    .child(
                        div()
                            .text_color(color(TEXT_4))
                            .child(format!("· {}", making.spec().starts)),
                    ),
            )
            // Flush with the card's edge, as the breadcrumb above them is.
            .child(flush_input(Input::new(&form.title).text_lg(), 32.))
            .child(indented(
                div()
                    .debug_selector(|| "new-task-body".into())
                    .mx(-TEXTAREA_PX)
                    .child(
                        Textarea::new(&form.body)
                            .appearance(false)
                            .xsmall()
                            .text_sm(),
                    ),
                &form.body,
            ))
            .children(fields)
            .child(div().h(px(1.)).bg(color(BORDER)))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_3()
                    .children(beside)
                    .child(div().flex_1())
                    .child(create),
            );
        Some(self.modal("new-task-modal", false, true, card, cx))
    }

    /// The command palette: a field, what it finds in sections, and the keys that drive it.
    /// The palette, shown at once and gone at once.
    pub(super) fn palette_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.model.palette.clone()?;
        let card = self.palette_card(&p, cx);
        Some(self.modal("palette-modal", false, false, card, cx))
    }

    /// The palette's card for `p`.
    pub(super) fn palette_card(&mut self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let mut results = div()
            .id("palette-results")
            .track_scroll(&self.palette_scroll)
            .max_h(px(400.))
            // On a short window the results give way and scroll, so the card fits.
            .min_h_0()
            .flex_shrink(1.)
            .overflow_y_scroll()
            .p_1()
            .flex()
            .flex_col();
        let mut section = None;
        for (i, item) in p.items.iter().enumerate() {
            let s = if p.link.is_some() || p.doc_pick.is_some() {
                "Link to"
            } else {
                section_of(item, &p.query)
            };
            if section != Some(s) {
                results = results.child(
                    div()
                        .px_3()
                        .pt_2()
                        .pb_1()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color(TEXT_4))
                        .child(s),
                );
                section = Some(s);
            }
            results = results.child(self.palette_row(i, item, i == p.selected, cx));
        }
        if p.items.is_empty() {
            results = results.child(
                div()
                    .h(px(64.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(color(TEXT_4))
                    .child("Nothing matches"),
            );
        }
        // What the actions apply to; when picking a link's other end, the link so far.
        let context = match (&p.link, &p.doc_pick) {
            (Some(pick), _) => Some(format!("{} · {}", pick.key, pick.choice.label)),
            (_, Some(DocPick::ForNode(key))) => Some(format!("{key} · Doc")),
            (_, Some(DocPick::ForDoc(path))) => Some(path.clone()),
            _ => self.model.acted_on().map(|r| r.key),
        };
        div()
            .debug_selector(|| "palette".into())
            .key_context(PALETTE)
            .w(px(PALETTE_W))
            .flex()
            .flex_col()
            .on_action(cx.listener(|this, _: &PaletteUp, _, cx| this.move_palette(-1, cx)))
            .on_action(cx.listener(|this, _: &PaletteDown, _, cx| this.move_palette(1, cx)))
            // One row, the field centred in it, with what the actions apply to at its end.
            .child(
                div()
                    .debug_selector(|| "palette-field".into())
                    .h(px(52.))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon(Icon::Search, 18., TEXT_4))
                    .child(
                        div()
                            .debug_selector(|| "palette-input".into())
                            .flex_1()
                            .min_w_0()
                            .child(flush_input(
                                Input::new(&self.palette_input).text_base(),
                                28.,
                            )),
                    )
                    .when_some(context, |d, key| {
                        d.child(
                            div()
                                .flex_none()
                                .h(px(20.))
                                .px_2()
                                .flex()
                                .items_center()
                                .rounded_full()
                                .border_1()
                                .border_color(color(BORDER_STRONG))
                                .text_xs()
                                .text_color(color(TEXT_3))
                                .child(key),
                        )
                    }),
            )
            .child(div().h(px(1.)).bg(color(BORDER)))
            .child(results)
            // How to move, run and close: a key hint, gone with the others.
            .when(hints_on(cx), |p| {
                p.child(
                    div()
                        .h(px(36.))
                        .px_4()
                        .flex()
                        .items_center()
                        .gap_2()
                        .border_t_1()
                        .border_color(color(BORDER))
                        .text_xs()
                        .text_color(color(TEXT_4))
                        .child(kbd("↑"))
                        .child(kbd("↓"))
                        .child("to move")
                        .child(div().w(px(8.)))
                        .child(kbd_icon(Icon::Enter))
                        .child("to run")
                        .child(div().w(px(8.)))
                        .child(kbd("Esc"))
                        .child("to close"),
                )
            })
    }

    /// An action's icon in the palette, so every row has one: a move shows the state it goes
    /// to, a priority its mark, the rest the icon of where that action lives in the app.
    pub(super) fn action_icon(&self, run: &Run) -> Div {
        let slot = div()
            .size(px(16.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center();
        let glyph = match run {
            Run::Choose(c) => {
                let kind = self.model.acted_on().map_or(Kind::Task, |r| r.kind);
                return slot.child(state_mark(kind, c.to));
            }
            Run::Priority(p) => return slot.child(priority_mark(*p)),
            Run::Go(view) => view_icon(*view),
            Run::Project(_) => Icon::Project,
            Run::ToggleLayout if self.model.layout() == Layout::List => Icon::Board,
            Run::ToggleLayout => Icon::List,
            Run::Group(_) => Icon::Grouping,
            Run::Order(_) => Icon::Ordering,
            Run::ToggleDismissed if self.model.display().show_dismissed => Icon::Hide,
            Run::ToggleDismissed => Icon::Show,
            Run::ToggleEmpty => Icon::ViewOptions,
            Run::FitGraph => Icon::Fit,
            Run::PickLink(_) | Run::Ask(Ask::PickLink) => Icon::Link,
            Run::Ask(Ask::New(m)) => making_icon(*m),
            Run::Ask(Ask::AddNote) => Icon::Notes,
            Run::Ask(Ask::EditTitle | Ask::EditBody) => Icon::Edit,
            Run::Ask(Ask::OpenFolder) => Icon::OpenProject,
            Run::Ask(Ask::Help) => Icon::Shortcuts,
            Run::Ask(Ask::SplitRight) => Icon::PanelRight,
            Run::Ask(Ask::SplitDown) => Icon::PanelBottom,
            Run::Ask(Ask::ClosePane) => Icon::Close,
            Run::Ask(Ask::NextPane) => Icon::ArrowRight,
            Run::Ask(Ask::NewTerminal) => Icon::Terminal,
        };
        slot.child(icon(glyph, 16., TEXT_3))
    }

    pub(super) fn palette_row(
        &self,
        i: usize,
        item: &PaletteItem,
        on: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let row = div()
            .id(SharedString::from(format!("palette-item-{i}")))
            .debug_selector(move || format!("palette-item-{i}"))
            .h(px(36.))
            .px_3()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .rounded_lg()
            .when(on, |r| r.bg(color(SELECTED)).text_color(color(TEXT)))
            .when(!on, |r| r.text_color(color(TEXT_2)))
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.model.palette.as_ref().is_some_and(|p| p.selected != i) {
                    this.model.palette_highlight(i);
                    cx.notify();
                }
            }))
            .on_click(
                cx.listener(move |this, _, window, cx| this.run_palette(Some(i), window, cx)),
            );
        match item {
            PaletteItem::Action(a) => {
                let keys = hint(a.shortcut.unwrap_or_default(), cx);
                row.child(self.action_icon(&a.run))
                    .child(div().truncate().child(a.label.clone()))
                    .when_some(a.context.clone(), |r, c| {
                        r.child(div().flex_none().text_color(color(TEXT_4)).child(c))
                    })
                    .child(div().flex_1())
                    .children(keys)
            }
            PaletteItem::Node(n) => row
                .child(state_mark(n.kind, n.state))
                .child(
                    div()
                        .w(px(60.))
                        .flex_none()
                        .text_color(color(TEXT_4))
                        .child(n.key.clone()),
                )
                .child(div().flex_1().min_w_0().truncate().child(n.title.clone()))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(color(TEXT_4))
                        .child(title_case(n.kind.as_str())),
                ),
            // Found by its text: the line it is in; else where it is.
            PaletteItem::Doc(d) => row
                .child(icon(Icon::Docs, 16., TEXT_3))
                .child(
                    div()
                        .flex_none()
                        .max_w(px(280.))
                        .truncate()
                        .child(d.title.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(color(TEXT_4))
                        .child(d.line.clone().unwrap_or_else(|| d.path.clone())),
                ),
        }
    }

    pub(super) fn help_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.help {
            return None;
        }
        let mut sections = div().flex().flex_col().gap_4();
        let mut current = "";
        let mut block = div().flex().flex_col();
        let shown = SHORTCUTS
            .iter()
            .filter(|(_, _, what)| cfg!(target_os = "macos") || *what != CLOSE_WINDOW);
        for (section, keys, what) in shown {
            if *section != current {
                if !current.is_empty() {
                    sections = sections.child(block);
                }
                block = div().flex().flex_col().child(
                    div()
                        .pb_1()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color(TEXT_4))
                        .child(*section),
                );
                current = section;
            }
            let mut combo = div().flex().items_center().gap_1();
            for k in *keys {
                combo = if matches!(*k, "then" | "or" | "+") {
                    combo.child(div().text_xs().text_color(color(TEXT_4)).child(*k))
                } else {
                    combo.child(kbd(k))
                };
            }
            block = block.child(
                div()
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(color(BORDER))
                    .child(div().flex_1().text_color(color(TEXT_2)).child(*what))
                    .child(combo),
            );
        }
        sections = sections.child(block);
        let card = div()
            .debug_selector(|| "shortcuts-card".into())
            .w(px(520.))
            .max_h(px(620.))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(46.))
                    .px_4()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(color(BORDER))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(TEXT))
                            .child("Keyboard shortcuts"),
                    )
                    .child(kbd("Esc")),
            )
            .child(
                div()
                    .id("shortcuts-scroll")
                    .debug_selector(|| "shortcuts-scroll".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .child(sections),
            );
        Some(self.modal("shortcuts-modal", true, true, card, cx))
    }
}

/// The child of the results list that shows item `index`: section headings come before it.
pub(super) fn palette_child(items: &[PaletteItem], index: usize) -> usize {
    let mut headings = 0;
    let mut last = None;
    for item in items.iter().take(index + 1) {
        let s = std::mem::discriminant(item);
        if last != Some(s) {
            headings += 1;
            last = Some(s);
        }
    }
    index + headings
}
