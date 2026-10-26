//! The dock floating over the bottom of a view, and its menus.

use super::*;

impl LivtkApp {
    /// The dock: one compact bar floating over the bottom of the content, centred on it, holding
    /// what the view can do: a new task, the filter, grouping, ordering, view options (list or
    /// board, empty groups, dismissed work; the graph's dismissed work alone) and the graph's
    /// zoom; none in a view with nothing for it. Labels show where there is room; else the icons
    /// stand alone, named by their tooltips. Menus open upwards from it.
    pub(super) fn dock(&self, window: &Window, cx: &mut Context<Self>) -> Option<Div> {
        // A placeholder has no view yet, nothing to filter or shape; a detail open as a full
        // page in its place has the list's dock no more.
        if self.model.placeholder() || self.full_page_here() {
            return None;
        }
        let view = self.model.view();
        let wide = self.content_width(window) >= DOCK_WIDE;
        let lists = matches!(
            view,
            View::Tasks
                | View::Projects
                | View::Lessons
                | View::Decisions
                | View::Findings
                | View::Of(_)
        );
        let mut groups: Vec<Vec<AnyElement>> = Vec::new();
        if view == View::Tasks {
            groups.push(vec![self.dock_new_task(wide, cx)]);
        }
        if view == View::Graph {
            groups.push(self.dock_zoom(cx));
        }
        let mut shaping = Vec::new();
        let doc_list = view == View::Docs && !self.model.docs.reading;
        if lists || view == View::Inbox || doc_list {
            shaping.push(self.dock_filter(wide, cx));
        }
        if lists {
            shaping.push(self.dock_choice(Menu::Grouping, wide, cx));
            shaping.push(self.dock_choice(Menu::Ordering, wide, cx));
        }
        let docs = &self.model.docs;
        if doc_list && docs.query.trim().is_empty() && !docs.tree.folders.is_empty() {
            shaping.push(self.dock_fold_docs(cx));
        }
        if lists {
            shaping.push(self.dock_view_options(wide, cx));
        } else if view == View::Graph {
            shaping.push(self.dock_dismissed(cx));
        }
        if !shaping.is_empty() {
            groups.push(shaping);
        }
        // A view with nothing to do here (the feed) has no dock at all, not an empty one.
        if groups.is_empty() {
            return None;
        }

        let mut dock = div()
            .debug_selector(|| "dock".into())
            .id("dock")
            .h(px(DOCK_HEIGHT))
            .p(px(4.))
            .flex()
            .items_center()
            .gap(px(2.))
            .rounded_xl()
            // Black, as the sidebar: the window's own colour; its border sets it apart.
            .bg(color(WINDOW))
            .border_1()
            .border_color(color(BORDER_STRONG))
            .shadow_lg()
            .text_sm()
            // Presses here are the dock's: nothing under it is clicked or dragged through it.
            .occlude();
        for (i, group) in groups.into_iter().enumerate() {
            if i > 0 {
                dock = dock.child(div().w(px(1.)).h(px(20.)).mx(px(4.)).bg(color(BORDER)));
            }
            dock = dock.children(group);
        }
        Some(div().child(dock))
    }

    /// A button of the dock: an icon, a label when `label` is given, a tooltip always.
    pub(super) fn dock_button(
        id: &'static str,
        glyph: Icon,
        label: Option<String>,
        tip: &'static str,
        on: bool,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .debug_selector(move || id.into())
            .group(id)
            .h(px(30.))
            .min_w(px(30.))
            .px(px(if label.is_some() { 10. } else { 7. }))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .rounded_lg()
            .when(on, |b| b.bg(color(SELECTED)).text_color(color(TEXT)))
            .when(!on, |b| {
                b.text_color(color(TEXT_3))
                    .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
            })
            .child(if on {
                icon(glyph, 16., TEXT)
            } else {
                icon_hover(glyph, 16., TEXT_3, id, TEXT)
            })
            .when_some(label, |b, l| b.child(div().whitespace_nowrap().child(l)))
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx)
            })
    }

    /// The dock's main button: make what the view lists (a project in Projects), else a task.
    pub(super) fn dock_new_task(&self, wide: bool, cx: &mut Context<Self>) -> AnyElement {
        let making = Making::for_view(self.model.view());
        div()
            .id("dock-new-task")
            .debug_selector(|| "dock-new-task".into())
            .h(px(30.))
            .min_w(px(30.))
            .px(px(if wide { 10. } else { 7. }))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .rounded_lg()
            .bg(color(ACCENT))
            .text_color(color(TEXT))
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.bg(color(ACCENT_HOVER)))
            .child(icon(making_icon(making), 16., TEXT))
            .when(wide, |b| {
                b.child(div().whitespace_nowrap().child(making.spec().title))
            })
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(format!(
                    "{} · {}",
                    making.spec().title,
                    making.spec().key
                ))
                .build(window, cx)
            })
            .on_click(cx.listener(move |this, _, window, cx| this.open_form(making, window, cx)))
            .into_any_element()
    }

    /// A dock button that opens `menu` upwards; the press is the button's alone.
    pub(super) fn dock_menu(
        &self,
        menu: Menu,
        button: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.menu == Some(menu);
        let button = button.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.toggle_menu(menu, window, cx);
            }),
        );
        div()
            .relative()
            .flex_none()
            .child(button)
            .when(open, |d| {
                let panel = match menu {
                    Menu::Filter => self.filter_menu(cx),
                    Menu::Grouping => self.grouping_menu(cx),
                    Menu::View => self.view_menu(cx),
                    _ => self.ordering_menu(cx),
                };
                d.child(self.popover_up(menu, false, panel, cx))
            })
            .into_any_element()
    }

    pub(super) fn dock_filter(&self, wide: bool, cx: &mut Context<Self>) -> AnyElement {
        let d = self.model.display();
        let active = if self.model.view() == View::Docs {
            usize::from(!self.model.docs.query.trim().is_empty())
        } else {
            usize::from(!d.filter.trim().is_empty())
                + usize::from(!d.priorities.is_empty())
                + usize::from(!d.states.is_empty())
        };
        let label = match (wide, active) {
            (false, _) => None,
            (true, 0) => Some("Filter".to_owned()),
            (true, n) => Some(format!("Filter · {n}")),
        };
        let open = self.menu == Some(Menu::Filter);
        let button = Self::dock_button(
            "filter-button",
            Icon::Filter,
            label,
            "Filter · /",
            open || active > 0,
        );
        self.dock_menu(Menu::Filter, button, cx)
    }

    /// Grouping or Ordering: the current choice when there is room, its menu on a press.
    pub(super) fn dock_choice(&self, menu: Menu, wide: bool, cx: &mut Context<Self>) -> AnyElement {
        let d = self.model.display();
        let (id, glyph, tip, current) = if menu == Menu::Grouping {
            let name = GROUPINGS
                .iter()
                .find(|g| g.2 == d.grouping)
                .map_or("State", |g| g.1);
            ("dock-grouping", Icon::Grouping, "Grouping", name)
        } else {
            let name = ORDERINGS
                .iter()
                .find(|o| o.2 == d.ordering)
                .map_or("Priority", |o| o.1);
            ("dock-ordering", Icon::Ordering, "Ordering", name)
        };
        let open = self.menu == Some(menu);
        let button = Self::dock_button(id, glyph, wide.then(|| current.to_owned()), tip, open);
        self.dock_menu(menu, button, cx)
    }

    /// How the list is shown, in a menu: lit while it is open or an option is on.
    pub(super) fn dock_view_options(&self, wide: bool, cx: &mut Context<Self>) -> AnyElement {
        let d = self.model.display();
        let open = self.menu == Some(Menu::View);
        let button = Self::dock_button(
            "dock-view",
            Icon::ViewOptions,
            wide.then(|| "View".to_owned()),
            "View options",
            open || d.show_empty || d.show_dismissed,
        );
        self.dock_menu(Menu::View, button, cx)
    }

    /// The docs' folders all shut with a press, or all open again: lit while all are shut.
    pub(super) fn dock_fold_docs(&self, cx: &mut Context<Self>) -> AnyElement {
        let shut = self.model.docs_all_shut();
        Self::dock_button(
            "dock-fold-docs",
            Icon::FoldAll,
            None,
            if shut { "Unfold all" } else { "Fold all" },
            shut,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.model.fold_docs(!shut);
            cx.notify();
        }))
        .into_any_element()
    }

    /// The graph's dismissed work: shown or hidden with a press, as it has nothing else to show.
    pub(super) fn dock_dismissed(&self, cx: &mut Context<Self>) -> AnyElement {
        let on = self.model.display().show_dismissed;
        Self::dock_button(
            "dock-dismissed",
            if on { Icon::Show } else { Icon::Hide },
            None,
            if on {
                "Hide dismissed work · H"
            } else {
                "Show dismissed work · H"
            },
            on,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.model.toggle_dismissed();
            cx.notify();
        }))
        .into_any_element()
    }

    pub(super) fn dock_zoom(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let z = self.model.graph.camera.zoom;
        vec![
            Self::dock_button("zoom-out", Icon::ZoomOut, None, "Zoom out · -", false)
                .on_click(cx.listener(|this, _, _, cx| this.zoom_graph(1. / ZOOM_STEP, cx)))
                .into_any_element(),
            Self::dock_button(
                "zoom-fit",
                Icon::Fit,
                Some(format!("{:.0}%", z * 100.)),
                "Zoom to fit · 0",
                false,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.model.graph_fit();
                cx.notify();
            }))
            .into_any_element(),
            Self::dock_button("zoom-in", Icon::ZoomIn, None, "Zoom in · +", false)
                .on_click(cx.listener(|this, _, _, cx| this.zoom_graph(ZOOM_STEP, cx)))
                .into_any_element(),
        ]
    }

    pub(super) fn grouping_menu(&self, cx: &mut Context<Self>) -> Div {
        let current = self.model.display().grouping;
        // A board needs columns: as in Linear, it is never ungrouped.
        let board = self.model.view().has_board() && self.model.layout() == Layout::Board;
        let mut panel = menu_panel(180.).debug_selector(|| "grouping-menu".into());
        // By project: tasks only.
        let tasks = self.model.view() == View::Tasks;
        for &(id, label, g) in GROUPINGS {
            if (board && g == Grouping::None) || (!tasks && g == Grouping::Project) {
                continue;
            }
            panel = panel.child(
                option_row(id.into())
                    .debug_selector(move || id.into())
                    .child(label)
                    .child(check(current == g))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.model.set_grouping(g);
                        this.close_menu(window, cx);
                    })),
            );
        }
        panel
    }

    /// How the list is shown, set in one visit, so no choice closes it: the tasks' list or board
    /// first, then a line, then options that each turn on or off: empty groups, dismissed work.
    pub(super) fn view_menu(&self, cx: &mut Context<Self>) -> Div {
        let d = self.model.display();
        let mut panel = menu_panel(232.).debug_selector(|| "view-menu".into());
        if self.model.view().has_board() {
            for (id, glyph, label, layout) in [
                ("view-layout-list", Icon::List, "List", Layout::List),
                ("view-layout-board", Icon::Board, "Board", Layout::Board),
            ] {
                let on = self.model.layout() == layout;
                panel = panel.child(
                    option_row(id.into())
                        .debug_selector(move || id.into())
                        .child(icon(glyph, 14., if on { TEXT } else { TEXT_3 }))
                        .child(label)
                        // B goes to the other one: its key is shown there.
                        .child(
                            div()
                                .ml_auto()
                                .flex()
                                .items_center()
                                .when(!on, |k| k.children(hint("B", cx)))
                                .child(check(on)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.model.set_layout(layout);
                            cx.notify();
                        })),
                );
            }
            panel = panel.child(menu_divider().debug_selector(|| "view-menu-divider".into()));
        }
        panel
            .child(view_option(
                "view-show-empty",
                "Show empty groups",
                Some(keys::EMPTY_GROUPS),
                d.show_empty,
                Model::toggle_empty,
                cx,
            ))
            .child(view_option(
                "view-show-dismissed",
                "Show dismissed work",
                Some(keys::DISMISSED),
                d.show_dismissed,
                Model::toggle_dismissed,
                cx,
            ))
            .when(self.model.view() == View::Tasks, |menu| {
                menu.child(view_option(
                    "view-nest",
                    "Nest sub-tasks",
                    None,
                    d.nest,
                    Model::toggle_nest,
                    cx,
                ))
            })
    }

    pub(super) fn ordering_menu(&self, cx: &mut Context<Self>) -> Div {
        let current = self.model.display().ordering;
        let mut panel = menu_panel(180.).debug_selector(|| "ordering-menu".into());
        for &(id, label, o) in ORDERINGS {
            panel = panel.child(
                option_row(id.into())
                    .debug_selector(move || id.into())
                    .child(label)
                    .child(check(current == o))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.model.set_ordering(o);
                        this.close_menu(window, cx);
                    })),
            );
        }
        panel
    }
}

/// One row of the room at the end of a virtualised list: empty, under the dock.
pub(super) fn dock_room() -> AnyElement {
    div().w_full().h(px(ROW)).into_any_element()
}
