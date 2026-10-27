//! The new-node form: one grammar for every kind it makes (docs/design.md, New-node form). Where
//! it goes, its title and body, a row of property chips ending in Add link, the links added,
//! then Create. Its properties are `Making`'s rows of `Prop`, held in the model's `Draft`; this
//! draws them from the design system's parts and turns the keys and clicks into picks.

use std::rc::Rc;

use jiff::civil::Date;

use super::form::toggle;
use super::parts::{Chip, ChoiceMenu, ChoiceRow, DatePicker};
use super::*;
use crate::model::dates::{self, Month, Quick, Step, WeekStart};
use crate::model::making::{Choice, Pick, Prop};

/// The form's fields: its title and body. Everything else it sets is the model's draft.
pub(super) struct NewForm {
    pub(super) title: Entity<InputState>,
    pub(super) body: Entity<TextareaState>,
}

/// Where the open date picker is: the month it shows, the day the keys are on, and the text typed
/// that could not be read as a date, with why: said while the field still holds it.
pub(super) struct DateState {
    pub(super) month: Month,
    pub(super) cursor: Date,
    pub(super) unread: Option<(String, String)>,
}

/// The day the week starts on, as the date picker lays out its grid.
const WEEK_START: WeekStart = WeekStart::Monday;

/// Today, where the human is.
fn today() -> Date {
    jiff::Zoned::now().date()
}

impl LivtkApp {
    /// Open the form making a node of kind `making`; one already open keeps its half-typed text
    /// and gets the keys back.
    pub(super) fn open_form(
        &mut self,
        making: Making,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(form) = &self.form {
            form.title.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        self.close_overlays(window, cx);
        let (title_hint, body_hint) = (making.spec().title_hint, making.spec().body_hint);
        let title = cx.new(|cx| InputState::new(window, cx).placeholder(title_hint));
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(body_hint)
                .auto_grow(3, 14)
        });
        let sub = cx.subscribe_in(&title, window, |this, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::PressEnter { .. } => this.submit_form(window, cx),
                // Create is lit once there is a title.
                InputEvent::Change => cx.notify(),
                _ => {}
            }
        });
        self._subscriptions.push(sub);
        // The body's textarea takes Ctrl+Enter itself, so the form's binding never sees it: it
        // reports it as a secondary Enter (after putting in the newline Enter does).
        let sub = cx.subscribe_in(&body, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter {
                secondary: true, ..
            } = event
            {
                this.submit_form(window, cx);
            }
        });
        self._subscriptions.push(sub);
        self.model.open_draft(making);
        title.read(cx).focus_handle(cx).focus(window, cx);
        self.form = Some(NewForm { title, body });
        cx.notify();
    }

    /// Make what the form says. With Create more, the form stays, emptied, for the next one.
    pub(super) fn submit_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let title = form.title.read(cx).value().to_string();
        // Without the newline Ctrl+Enter leaves at the end.
        let body = form.body.read(cx).value().trim_end().to_owned();
        if !self.model.create(&title, &body) {
            cx.notify();
            return;
        }
        if self.model.draft.is_some() {
            // Create more: the same form, empty, its chips kept.
            let (t, b) = (form.title.clone(), form.body.clone());
            t.update(cx, |s, cx| s.set_value("", window, cx));
            b.update(cx, |s, cx| s.set_value("", window, cx));
            t.read(cx).focus_handle(cx).focus(window, cx);
            self.sync_filter(window, cx);
        } else {
            self.form = None;
            self.sync_filter(window, cx);
            self.focus_main(window, cx);
            self.after_move(cx);
        }
        cx.notify();
    }

    /// Close the form, making nothing.
    pub(super) fn cancel_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        self.model.close_draft();
        self.menu = None;
        self.focus_main(window, cx);
        cx.notify();
    }

    // ---- the chips' menus -----------------------------------------------------------------

    /// Open (or close) a chip's menu, its field empty and the keys on the one chosen.
    fn toggle_form_menu(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        let opening = self.menu != Some(menu) && self.menu_closed != Some(menu);
        self.toggle_menu(menu, window, cx);
        if !opening || self.menu != Some(menu) {
            return;
        }
        self.menu_query
            .update(cx, |s, cx| s.set_value("", window, cx));
        if let Menu::FormProp(Prop::Agent) = menu {
            self.model.load_agents();
        }
        if let Menu::FormProp(Prop::Target) = menu {
            let at = self
                .model
                .draft
                .as_ref()
                .and_then(|d| d.target)
                .unwrap_or_else(today);
            self.date = DateState {
                month: Month::of(at),
                cursor: at,
                unread: None,
            };
            self.menu_query.update(cx, |s, cx| {
                s.set_placeholder("A date: Dec 15, next friday, in 2 weeks…", window, cx);
            });
        } else {
            self.menu_query
                .update(cx, |s, cx| s.set_placeholder("Find…", window, cx));
        }
        self.menu_hl = self.form_choices(cx).iter().position(|c| c.on).unwrap_or(0);
        self.menu_query.read(cx).focus_handle(cx).focus(window, cx);
    }

    /// The choices the open chip menu shows, as its field finds them.
    fn form_choices(&self, cx: &App) -> Vec<Choice> {
        let query = self.menu_query.read(cx).value().to_string();
        match self.menu {
            Some(Menu::FormProp(p)) => self.model.prop_choices(p, &query),
            Some(Menu::FormLink) => self.model.link_relations(&query),
            _ => Vec::new(),
        }
    }

    /// The open chip menu's choice `i`, picked: set on the draft, the menu closed; a relation
    /// goes on to the palette for the link's other end.
    fn form_pick(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(choice) = self.form_choices(cx).into_iter().nth(i) else {
            return;
        };
        self.close_menu(window, cx);
        if self.model.apply_pick(choice.pick) {
            self.reset_palette_input(window, cx);
        }
        cx.notify();
    }

    /// The target date picked, or cleared.
    fn form_date(&mut self, date: Option<Date>, window: &mut Window, cx: &mut Context<Self>) {
        self.model.apply_pick(Pick::Target(date));
        self.close_menu(window, cx);
    }

    /// Enter in an open chip menu: the highlighted choice; in the date picker, the date typed,
    /// or else the day the keys are on.
    fn form_menu_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu == Some(Menu::FormProp(Prop::Target)) {
            let typed = self.menu_query.read(cx).value().to_string();
            if typed.trim().is_empty() {
                let at = self.date.cursor;
                self.form_date(Some(at), window, cx);
                return;
            }
            match dates::parse(&typed, today(), WEEK_START) {
                Ok(d) if self.target_range.contains(d) => self.form_date(Some(d), window, cx),
                // A date it may not take is said back, as one it cannot read is.
                Ok(d) => {
                    self.date.unread =
                        Some((typed, format!("{d} is outside the dates it can take")));
                    cx.notify();
                }
                Err(e) => {
                    self.date.unread = Some((typed, e.0));
                    cx.notify();
                }
            }
            return;
        }
        let at = self.menu_hl;
        self.form_pick(at, window, cx);
    }

    /// Whether a chip's menu is open: then the arrows and Enter are its, not the field's.
    fn form_menu_open(&self) -> bool {
        matches!(self.menu, Some(Menu::FormProp(_) | Menu::FormLink))
    }

    /// The arrows in an open chip menu: up and down move the highlight (a week in the date
    /// picker); left, right, home and end move the date picker's day while its field is empty.
    fn form_menu_key(&mut self, key: MenuKey, cx: &mut Context<Self>) -> bool {
        if !self.form_menu_open() {
            return false;
        }
        if self.menu == Some(Menu::FormProp(Prop::Target)) {
            let empty = self.menu_query.read(cx).value().is_empty();
            let by = match key {
                MenuKey::Up => Step::Week(-1),
                MenuKey::Down => Step::Week(1),
                MenuKey::PageUp => Step::Month(-1),
                MenuKey::PageDown => Step::Month(1),
                MenuKey::Left if empty => Step::Day(-1),
                MenuKey::Right if empty => Step::Day(1),
                MenuKey::Home if empty => Step::WeekStart,
                MenuKey::End if empty => Step::WeekEnd,
                _ => return false,
            };
            self.date.cursor = dates::step(self.date.cursor, by, WEEK_START, self.target_range);
            self.date.month = Month::of(self.date.cursor);
            cx.notify();
            return true;
        }
        let n = self.form_choices(cx).len();
        if n == 0 {
            return false;
        }
        self.menu_hl = match key {
            MenuKey::Up => self.menu_hl.checked_sub(1).unwrap_or(n - 1),
            MenuKey::Down => (self.menu_hl + 1) % n,
            MenuKey::PageUp | MenuKey::Home => 0,
            MenuKey::PageDown | MenuKey::End => n - 1,
            _ => return false,
        };
        cx.notify();
        true
    }

    // ---- drawing ----------------------------------------------------------------------------

    pub(super) fn form_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.form.as_ref()?;
        let draft = self.model.draft.clone()?;
        let making = draft.making;
        let spec = making.spec();
        let (card_id, create_id) = (spec.card_id, spec.create_id);
        let titled = !form.title.read(cx).value().trim().is_empty();
        let (title, body) = (form.title.clone(), form.body.clone());
        // The chips: each property the kind sets, then Add link.
        let mut chips = div()
            .debug_selector(|| "form-props".into())
            .flex()
            .flex_wrap()
            .gap(px(6.));
        for &prop in spec.props {
            chips = chips.child(self.prop_chip(prop, cx));
        }
        chips = chips.child(self.add_link_chip(cx));
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
            .when(!titled, |b| b.opacity(0.45))
            .when(titled, |b| b.hover(|s| s.bg(color(ACCENT_HOVER))))
            .child(spec.create)
            .child(
                div()
                    .text_xs()
                    .text_color(with_alpha(TEXT, 0.7))
                    .child(keys::SUBMIT),
            )
            .on_click(cx.listener(|this, _, window, cx| this.submit_form(window, cx)));
        let more = draft.more;
        let card =
            div()
                .debug_selector(move || card_id.into())
                .w(px(640.))
                .flex()
                .flex_col()
                // While a chip's menu is open, the arrows and Enter are the menu's, not its field's.
                .capture_action(cx.listener(|this, _: &input::MoveUp, _, cx| {
                    this.menu_key_or_pass(MenuKey::Up, cx)
                }))
                .capture_action(cx.listener(|this, _: &input::MoveDown, _, cx| {
                    this.menu_key_or_pass(MenuKey::Down, cx)
                }))
                .capture_action(cx.listener(|this, _: &input::MoveLeft, _, cx| {
                    this.menu_key_or_pass(MenuKey::Left, cx)
                }))
                .capture_action(cx.listener(|this, _: &input::MoveRight, _, cx| {
                    this.menu_key_or_pass(MenuKey::Right, cx)
                }))
                .capture_action(cx.listener(|this, _: &input::MoveHome, _, cx| {
                    this.menu_key_or_pass(MenuKey::Home, cx)
                }))
                .capture_action(cx.listener(|this, _: &input::MoveEnd, _, cx| {
                    this.menu_key_or_pass(MenuKey::End, cx)
                }))
                .capture_action(cx.listener(|this, _: &input::MovePageUp, _, cx| {
                    this.menu_key_or_pass(MenuKey::PageUp, cx)
                }))
                .capture_action(cx.listener(|this, _: &input::MovePageDown, _, cx| {
                    this.menu_key_or_pass(MenuKey::PageDown, cx)
                }))
                .capture_action(cx.listener(|this, _: &input::Enter, window, cx| {
                    if this.form_menu_open() {
                        this.form_menu_enter(window, cx);
                    } else {
                        cx.propagate();
                    }
                }))
                // The head: where it goes, what it makes; its close.
                .child(
                    div()
                        .px_4()
                        .pt_3()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_xs()
                        .text_color(color(TEXT_3))
                        // Where it goes: the project, as a chip that is only said.
                        .child(
                            Chip::new(
                                "form-where",
                                self.model
                                    .project()
                                    .map(|p| p.name.clone())
                                    .unwrap_or_default(),
                            )
                            .lead(div().size(px(8.)).rounded(px(2.)).bg(color(ACCENT)))
                            .still(true)
                            .build(),
                        )
                        .child(div().text_color(color(TEXT_4)).child("›"))
                        .child(spec.title)
                        .child(div().flex_1())
                        .child(
                            div()
                                .id("form-close")
                                .debug_selector(|| "form-close".into())
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_lg()
                                .hover(|h| h.bg(color(HOVER)))
                                .child(icon(Icon::Close, 14., TEXT_4))
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.cancel_form(window, cx)),
                                ),
                        ),
                )
                // Its title and body, flush with the card's edge as the head is.
                .child(
                    div()
                        .px_4()
                        .pt_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(flush_input(Input::new(&title).text_lg(), 32.))
                        .child(indented(
                            div()
                                .debug_selector(|| "new-task-body".into())
                                .mx(-TEXTAREA_PX)
                                .child(Textarea::new(&body).appearance(false).xsmall().text_sm()),
                            &body,
                        )),
                )
                .child(div().px_4().pt_2().pb_3().child(chips))
                .children(self.form_links(&draft, cx))
                .child(
                    div()
                        .px_4()
                        .py_3()
                        .border_t_1()
                        .border_color(color(BORDER))
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .id("form-more")
                                .debug_selector(|| "form-more".into())
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_xs()
                                .text_color(color(TEXT_3))
                                .child(toggle("form-more-toggle".into(), more))
                                .child("Create more")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(d) = this.model.draft.as_mut() {
                                        d.more = !d.more;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(div().flex_1())
                        .child(
                            super::form::button("form-cancel", "Cancel", false).on_click(
                                cx.listener(|this, _, window, cx| this.cancel_form(window, cx)),
                            ),
                        )
                        .child(create),
                );
        Some(self.modal("new-task-modal", false, true, card, cx))
    }

    /// The key a menu took, or let through to the field it is in.
    fn menu_key_or_pass(&mut self, key: MenuKey, cx: &mut Context<Self>) {
        if !self.form_menu_key(key, cx) {
            cx.propagate();
        }
    }

    /// A property's chip: its value and mark when set, its name when not; a click opens its
    /// menu.
    fn prop_chip(&self, prop: Prop, cx: &mut Context<Self>) -> Div {
        let menu = Menu::FormProp(prop);
        let open = self.menu == Some(menu);
        let value = self.model.prop_value(prop, today());
        let d = self.model.draft.as_ref();
        let lead: AnyElement = match prop {
            Prop::State => state_mark(
                Kind::Task,
                if d.is_some_and(|d| d.planned) {
                    State::Planned
                } else {
                    State::Backlog
                },
            )
            .into_any_element(),
            Prop::Priority => priority_mark(d.map_or(3, |d| d.priority)).into_any_element(),
            Prop::Status => state_mark(
                Kind::Project,
                if d.is_none_or(|d| d.active) {
                    State::Active
                } else {
                    State::Proposed
                },
            )
            .into_any_element(),
            _ => icon(
                prop_icon(prop),
                12.,
                if value.is_some() { TEXT_3 } else { TEXT_4 },
            )
            .into_any_element(),
        };
        let chip = Chip::new(
            prop.id(),
            value.clone().unwrap_or_else(|| prop.name().into()),
        )
        .lead(lead)
        .unset(value.is_none())
        .open(open)
        .build()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.toggle_form_menu(menu, window, cx);
            }),
        );
        div().relative().child(chip).when(open, |el| {
            let panel = if prop == Prop::Target {
                self.date_menu(cx)
            } else {
                self.choice_menu(prop.id(), cx)
            };
            el.child(self.popover(menu, false, 30., panel, cx))
        })
    }

    /// The chip that adds a link: the relations, then the palette for the other end.
    fn add_link_chip(&self, cx: &mut Context<Self>) -> Div {
        let open = self.menu == Some(Menu::FormLink);
        let chip = Chip::new("form-add-link", "Add link")
            .lead(icon(Icon::Link, 12., TEXT_4))
            .unset(true)
            .open(open)
            .build()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_form_menu(Menu::FormLink, window, cx);
                }),
            );
        div().relative().child(chip).when(open, |el| {
            let panel = self.choice_menu("form-add-link", cx);
            el.child(self.popover(Menu::FormLink, false, 30., panel, cx))
        })
    }

    /// The open chip's menu: its field, its choices, the keys' highlight.
    fn choice_menu(&self, id: &'static str, cx: &mut Context<Self>) -> Div {
        let rows = choice_rows(self.form_choices(cx));
        let hl = (!rows.is_empty()).then(|| self.menu_hl.min(rows.len() - 1));
        let this = cx.entity().downgrade();
        let on_pick: super::parts::OnPick = Rc::new(move |i, window, cx| {
            let _ = this.update(cx, |this, cx| this.form_pick(i, window, cx));
        });
        let menu_id: &'static str = match id {
            "form-add-link" => "form-link-menu",
            _ => "form-menu",
        };
        let empty = match self.menu {
            Some(Menu::FormProp(Prop::Owner)) => "Type a name to make them the owner",
            Some(Menu::FormLink) => "No relation by that name",
            _ => "Nothing matches",
        };
        ChoiceMenu::new(menu_id, 260., on_pick)
            .empty(empty)
            .field(
                div()
                    .flex_1()
                    .child(Input::new(&self.menu_query).appearance(false).small()),
            )
            .rows(rows)
            .highlight(hl)
            .build()
    }

    /// The target date's picker.
    fn date_menu(&self, cx: &mut Context<Self>) -> Div {
        let chosen = self.model.draft.as_ref().and_then(|d| d.target);
        let (pick, month) = (cx.entity().downgrade(), cx.entity().downgrade());
        DatePicker::new(
            "form-date",
            today(),
            self.date.month,
            Rc::new(move |d, window, cx| {
                let _ = pick.update(cx, |this, cx| this.form_date(d, window, cx));
            }),
            Rc::new(move |m, _, cx| {
                let _ = month.update(cx, |this, cx| {
                    this.date.month = m;
                    cx.notify();
                });
            }),
        )
        .cursor(Some(self.date.cursor))
        .chosen(chosen)
        .range(self.target_range)
        .week_start(WEEK_START)
        .quick(&Quick::TARGET)
        .field(
            div()
                .flex_1()
                .child(Input::new(&self.menu_query).appearance(false).small()),
        )
        .unread(
            self.date
                .unread
                .as_ref()
                .filter(|(typed, _)| *self.menu_query.read(cx).value() == **typed)
                .map(|(_, why)| why.clone()),
        )
        .build()
    }

    /// The links the form will make, below its chips, grouped by relation as the task panel
    /// groups a node's: each node named as everywhere, its × taking the link away. Nothing until
    /// there is one.
    fn form_links(
        &self,
        draft: &crate::model::making::Draft,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if draft.links.is_empty() {
            return None;
        }
        // Grouped and headed as the panel groups a node's links, in the order first added.
        let mut groups: Vec<(&'static str, Vec<usize>)> = Vec::new();
        for (i, l) in draft.links.iter().enumerate() {
            let heading = crate::model::link_heading(l.choice.rel, l.choice.outgoing);
            match groups.iter_mut().find(|(h, _)| *h == heading) {
                Some((_, v)) => v.push(i),
                None => groups.push((heading, vec![i])),
            }
        }
        let mut list = super::parts::LinkList::new();
        for (heading, at) in groups {
            let slug = heading.to_lowercase().replace(' ', "-");
            let items = at
                .into_iter()
                .map(|i| {
                    let to = &draft.links[i].to;
                    let unlink = unlink_button(
                        format!("form-unlink-{slug}-{}", to.key),
                        move |this, _| {
                            if let Some(d) = this.model.draft.as_mut() {
                                d.remove_link(i);
                            }
                        },
                        cx,
                    );
                    self.link_item(
                        format!("form-link-{slug}-{}", to.key),
                        &to.key,
                        to.kind,
                        to.state,
                        &to.title,
                        OpenIn::View,
                        Some(unlink),
                        cx,
                    )
                })
                .collect();
            list = list.group(heading, items);
        }
        Some(
            div()
                .debug_selector(|| "form-links".into())
                .px_4()
                .pb_3()
                .child(list.build()),
        )
    }
}

/// A key an open chip menu answers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

/// The icon a property's chip leads with when no mark of its value does.
fn prop_icon(prop: Prop) -> Icon {
    match prop {
        Prop::Project | Prop::PartOf => Icon::Projects,
        Prop::Agent => Icon::Agents,
        Prop::Owner => Icon::You,
        Prop::Target => Icon::Calendar,
        Prop::State | Prop::Priority | Prop::Status => Icon::Tasks,
    }
}

/// The model's choices as a menu's rows: each led by its value's mark or its kind's icon. The
/// panel's Add link and every chip's menu draw theirs from it.
pub(super) fn choice_rows(choices: Vec<Choice>) -> Vec<ChoiceRow> {
    choices
        .into_iter()
        .map(|c| ChoiceRow {
            lead: pick_lead(&c.pick),
            label: c.label,
            sub: c.sub,
            on: c.on,
        })
        .collect()
}

/// What a choice leads with in its menu: its value's mark, or its kind's icon.
fn pick_lead(pick: &Pick) -> Option<AnyElement> {
    Some(match pick {
        Pick::Planned(p) => {
            state_mark(Kind::Task, if *p { State::Planned } else { State::Backlog })
                .into_any_element()
        }
        Pick::Priority(p) => priority_mark(*p).into_any_element(),
        Pick::Active(a) => state_mark(
            Kind::Project,
            if *a { State::Active } else { State::Proposed },
        )
        .into_any_element(),
        Pick::Parent(Some(_)) => icon(Icon::Projects, 13., TEXT_3).into_any_element(),
        Pick::Agent(_) => icon(Icon::Agents, 13., TEXT_3).into_any_element(),
        Pick::Owner(Some(_)) => icon(Icon::You, 13., TEXT_3).into_any_element(),
        Pick::Link(_) => icon(Icon::Link, 13., TEXT_3).into_any_element(),
        Pick::Parent(None) | Pick::Owner(None) | Pick::Target(_) => return None,
    })
}
