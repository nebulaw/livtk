//! The sidebar, its collapsed rail, the top bar, and the edges that resize them.

use super::*;

impl LivtkApp {
    pub(super) fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Settings open: the sidebar lists their sections, and the way back.
        if self.settings.open.is_some() {
            return self.settings_sidebar(cx);
        }
        if self.rail(window) {
            return self.rail_bar(cx);
        }
        let mut side = div()
            .debug_selector(|| "sidebar".into())
            .w(px(self.sidebar_width))
            .flex_none()
            .h_full()
            .relative()
            .flex()
            .flex_col()
            .px_2()
            .pb_3()
            .pt_1()
            .gap(px(2.));

        let inbox = self.model.project().map(|p| p.inbox).unwrap_or(0);
        let unseen = self.model.project().map(|p| p.unseen).unwrap_or(0);
        // The split's views first, grouped, while there is one; then the inbox and the feed,
        // under no heading; then the views of the project's work; then the agents working on
        // it; then the tools to work on it with. A view in the split is in its group, not in
        // its usual place.
        let split = self.model.split_views();
        if let Some(group) = self.sidebar_split(&split, cx) {
            side = side.child(group).child(div().h(px(10.)));
        }
        for view in self.model.views() {
            if view == View::Tasks {
                side = side.child(div().h(px(14.))).child(section_label("Views"));
            }
            if view == View::Agents {
                side = side.child(div().h(px(14.))).child(section_label("Agents"));
            }
            if view == View::Graph {
                side = side.child(div().h(px(14.))).child(section_label("Tools"));
            }
            if split.iter().any(|(_, v)| *v == Some(view)) {
                continue;
            }
            let count = match view {
                View::Inbox => inbox,
                View::Activity => unseen,
                _ => 0,
            };
            let row = nav_item(
                SharedString::from(format!("view-{}", view.title())),
                view_icon(view),
                view.title(),
                view == self.model.view() && !self.model.placeholder(),
                count,
            )
            .on_click(cx.listener(move |this, _, window, cx| this.go(view, window, cx)));
            side = side.child(self.side_move(view.title(), row.into_any_element(), cx));
        }
        side.child(div().flex_1())
            .when_some(self.update.as_ref(), |side, staged| {
                side.child(
                    div()
                        .id("restart-to-update")
                        .debug_selector(|| "restart-to-update".into())
                        .px_2()
                        .h(px(NAV_ROW))
                        .text_size(px(NAV_TEXT))
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded_lg()
                        .text_color(color(ACCENT_HOVER))
                        .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT)))
                        .child(div().size(px(6.)).rounded_full().bg(color(ACCENT)))
                        .child(div().flex_1().child("Restart to Update"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(color(TEXT_4))
                                .child(format!("build {}", staged.manifest.build)),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.restart_to_update(cx))),
                )
            })
            .child(
                div()
                    .id("shortcuts")
                    .group("nav")
                    .px_2()
                    .h(px(NAV_ROW))
                    .text_size(px(NAV_TEXT))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_lg()
                    .text_color(color(TEXT_4))
                    .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_3)))
                    .child(icon_hover(Icon::Shortcuts, NAV_ICON, TEXT_4, "nav", TEXT_3))
                    .child(div().flex_1().child("Shortcuts"))
                    .child(kbd(keys::MOD))
                    .child(kbd("?"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.set_help(!this.help, window, cx)),
                    ),
            )
            .child(
                div()
                    .id("open-settings")
                    .debug_selector(|| "open-settings".into())
                    .group("nav")
                    .px_2()
                    .h(px(NAV_ROW))
                    .text_size(px(NAV_TEXT))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_lg()
                    .text_color(color(TEXT_4))
                    .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_3)))
                    .child(icon_hover(Icon::Settings, NAV_ICON, TEXT_4, "nav", TEXT_3))
                    .child(div().flex_1().child("Settings"))
                    .child(kbd(keys::MOD))
                    .child(kbd(","))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_settings(settings::Section::Appearance, window, cx)
                    })),
            )
            .child(self.edge(Edge::Sidebar, window, cx).right_0())
            .into_any_element()
    }

    /// A draggable edge: the sidebar's right, the panel's left, spanning the gutter beside the
    /// content box, so its middle is 4 px from the box on either side. Nothing shows at rest. On
    /// hover a glow follows the pointer along it, the handle at its middle (the cursor stays, by
    /// the developer's rule); dragging brightens it. A double-click puts the width back to its
    /// default. The caller places it: `right_0` beside the sidebar and the content, or across
    /// a floating panel's border.
    pub(super) fn edge(
        &self,
        edge: Edge,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id: SharedString = match edge {
            Edge::Sidebar => "sidebar-edge".into(),
            Edge::Panel => "panel-edge".into(),
            Edge::Bottom => "bottom-edge".into(),
            Edge::Divider { a, b, .. } => format!("divider-{a}-{b}").into(),
        };
        let dragging = self.resizing == Some(edge);
        // The side edges run the height under the top bar; the bottom one the content's width;
        // a divider its split's.
        let len = match edge {
            Edge::Bottom => self.window_content_width(window),
            Edge::Divider { .. } => self.divider_along(edge, None).0,
            _ => f32::from(window.viewport_size().height) - TOP_BAR,
        };
        let glow = match (self.edge_hover, self.edge_leaving) {
            (Some(h), _) if h.edge == edge => Some(edge_glow(h, dragging, len, false)),
            (_, Some(h)) if h.edge == edge => Some(edge_glow(h, false, len, true)),
            _ => None,
        };
        let selector = id.to_string();
        let el = div()
            .id(id)
            .debug_selector(move || selector.clone())
            .absolute();
        let el = if edge.across() {
            el.left_0().right_0().h(px(EDGE_W))
        } else {
            el.top_0().bottom_0().w(px(EDGE_W))
        };
        el.children(glow)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.edge_inside = Some(edge);
                    return;
                }
                if this.edge_inside == Some(edge) {
                    this.edge_inside = None;
                }
                // Dragging keeps it lit wherever the pointer goes.
                if this.resizing != Some(edge) && this.edge_hover.is_some_and(|h| h.edge == edge) {
                    this.let_go_edge(cx);
                }
            }))
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, window, cx| {
                let at = this.along(edge, e.position, window);
                this.hover_edge(edge, at);
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if e.click_count >= 2 {
                        match edge {
                            Edge::Sidebar => this.sidebar_width = SIDEBAR_WIDTH,
                            Edge::Panel => this.panel_width = PANEL_WIDTH,
                            Edge::Bottom => this.bottom_height = BOTTOM_HEIGHT,
                            Edge::Divider { .. } => this.even_divider(edge),
                        }
                        this.resizing = None;
                    } else {
                        this.resizing = Some(edge);
                    }
                    cx.notify();
                }),
            )
    }

    /// The glow lets go: it fades out where it was, then is gone.
    pub(super) fn let_go_edge(&mut self, cx: &mut Context<Self>) {
        let Some(h) = self.edge_hover.take() else {
            return;
        };
        self.edge_leaving = Some(h);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(GLOW_OUT).await;
            let _ = this.update(cx, |this, cx| {
                if this.edge_leaving.is_some_and(|l| l.epoch == h.epoch) {
                    this.edge_leaving = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// How far along `edge` the window's point `p` is: down a side edge, from under the top
    /// bar; across the bottom one, from the content's left.
    pub(super) fn along(&self, edge: Edge, p: Point<Pixels>, window: &Window) -> f32 {
        match edge {
            Edge::Bottom => f32::from(p.x) - self.sidebar_px(window),
            Edge::Divider { .. } => self.divider_along(edge, Some(p)).1,
            _ => f32::from(p.y) - TOP_BAR,
        }
    }

    /// The pointer over `edge`, `at` along it: the glow follows it, fading in when it arrives.
    pub(super) fn hover_edge(&mut self, edge: Edge, at: f32) {
        self.edge_leaving = None;
        let epoch = match self.edge_hover {
            Some(h) if h.edge == edge => h.epoch,
            _ => {
                self.edge_epoch += 1;
                self.edge_epoch
            }
        };
        self.edge_hover = Some(EdgeHover { edge, at, epoch });
    }

    /// The pointer at `p` while dragging `edge` in a window `size`. The sidebar's width is where
    /// the pointer is; dragged under the snap, it becomes the rail. The panel's width is what lies
    /// right of the pointer, the bottom bar's height what lies below it.
    pub(super) fn drag_edge(&mut self, edge: Edge, p: Point<Pixels>, size: Size<Pixels>) {
        let (x, y) = (f32::from(p.x), f32::from(p.y));
        let (window_w, window_h) = (f32::from(size.width), f32::from(size.height));
        match edge {
            Edge::Sidebar if x < SIDEBAR_SNAP => {
                if window_w < RAIL_BELOW {
                    self.sidebar_peek = false;
                } else {
                    self.sidebar_collapsed = true;
                }
                self.resizing = None;
            }
            Edge::Sidebar => self.sidebar_width = x.clamp(SIDEBAR_MIN, SIDEBAR_MAX),
            Edge::Panel => self.panel_width = (window_w - x).clamp(PANEL_MIN, PANEL_MAX),
            // Under the bar, the window's bottom inset.
            Edge::Bottom => {
                self.bottom_height = (window_h - y - BOTTOM_INSET).clamp(BOTTOM_MIN, BOTTOM_MAX)
            }
            Edge::Divider { .. } => self.drag_divider_edge(edge, p),
        }
    }

    /// ⌘B and the top bar's button. On a narrow window the sidebar is the rail anyway, so
    /// the toggle opens it there for now; on a wide one it is the human's choice, kept.
    pub(super) fn toggle_sidebar(&mut self, window: &Window, cx: &mut Context<Self>) {
        if Self::window_width(window) < RAIL_BELOW {
            self.sidebar_peek = !self.sidebar_peek;
        } else {
            self.sidebar_collapsed = !self.sidebar_collapsed;
        }
        cx.notify();
    }

    /// The bottom bar, under the content box between the sidebar and the panel: black, edged as
    /// the content box is, its top edge dragged for its height. It holds the activity grid. However tall
    /// it is kept, the content above it keeps its room. It slides in and away as the detail
    /// panel does, its height animating so the content above gives way smoothly.
    pub(super) fn bottom_bar(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let room = f32::from(window.viewport_size().height)
            - TOP_BAR
            - CONTENT_MIN_H
            - EDGE_W
            - BOTTOM_INSET;
        let h = self
            .bottom_height
            .min(room)
            .max(BOTTOM_MIN.min(room))
            .max(0.);
        let closing = !self.bottom_open;
        let epoch = self.bottom_epoch;
        let (id, duration) = match (closing, epoch) {
            (true, _) => (format!("bottom-out-{epoch}"), SLIDE_OUT),
            // Open from the start (brought back from the last run): no slide.
            (false, 0) => ("bottom-in-0".to_owned(), Duration::ZERO),
            (false, _) => (format!("bottom-in-{epoch}"), SLIDE),
        };
        let easing: fn(f32) -> f32 = if closing { ease_in_out } else { ease_out_cubic };
        let full = h + EDGE_W;
        div()
            .flex_none()
            .relative()
            .overflow_hidden()
            .pt(px(EDGE_W))
            .child(self.edge(Edge::Bottom, window, cx).top_0())
            .child(
                div()
                    .debug_selector(|| "bottom-bar".into())
                    .h(px(h))
                    .rounded_xl()
                    .bg(color(0x000000))
                    .border_1()
                    .border_color(color(BORDER))
                    .overflow_hidden()
                    .child(self.activity_grid(cx)),
            )
            .with_animation(
                SharedString::from(id),
                Animation::new(duration).with_easing(easing),
                move |el, t| {
                    let open = if closing { 1. - t } else { t };
                    el.h(px(full * open)).opacity(0.3 + 0.7 * open)
                },
            )
            .into_any_element()
    }

    /// ⌘J and the top bar's button: the bottom bar shown or hidden.
    /// It slides as the detail panel does: in, or away and then gone.
    pub(super) fn toggle_bottom(&mut self, cx: &mut Context<Self>) {
        self.bottom_open = !self.bottom_open;
        self.bottom_epoch += 1;
        self.bottom_leaving = !self.bottom_open;
        if self.bottom_leaving {
            let epoch = self.bottom_epoch;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SLIDE_OUT).await;
                let _ = this.update(cx, |this, cx| {
                    if this.bottom_epoch == epoch {
                        this.bottom_leaving = false;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// ⌥⌘B and the top bar's button: the detail panel closed, or opened on what is selected,
    /// as Enter opens it.
    pub(super) fn toggle_panel(&mut self, cx: &mut Context<Self>) {
        let reading_doc = self.model.view() == View::Docs && self.model.detail.is_none();
        let docs = &mut self.model.docs;
        if reading_doc && docs.reading {
            // Reading, the panel is the doc's.
            docs.hide_panel = !docs.hide_panel;
        } else if self.model.detail.is_some() {
            self.model.close_detail();
        } else {
            self.model.open_detail();
        }
        cx.notify();
    }

    /// The collapsed sidebar: a rail of icons. The project switcher, each view with a dot when
    /// it holds something new, and the shortcuts; names in tooltips.
    pub(super) fn rail_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let inbox = self.model.project().map_or(0, |p| p.inbox);
        let unseen = self.model.project().map_or(0, |p| p.unseen);
        let mut rail = div()
            .debug_selector(|| "rail".into())
            .w(px(RAIL_WIDTH))
            .flex_none()
            .h_full()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .pb_3()
            .pt_1()
            .gap(px(2.));
        // The split's views first, grouped, as on the sidebar; out of their usual places.
        let split = self.model.split_views();
        if let Some(group) = self.rail_split(&split, cx) {
            rail = rail.child(group).child(div().h(px(10.)));
        }
        for view in self.model.views() {
            // The inbox and the feed, the views of the work, the tools, as the sidebar groups
            // them.
            if matches!(view, View::Tasks | View::Graph) {
                rail = rail.child(div().h(px(14.)));
            }
            if split.iter().any(|(_, v)| *v == Some(view)) {
                continue;
            }
            let new = match view {
                View::Inbox => inbox > 0,
                View::Activity => unseen > 0,
                _ => false,
            };
            let button = rail_button(
                SharedString::from(format!("rail-{}", view.title())),
                view_icon(view),
                view == self.model.view() && !self.model.placeholder(),
                new,
                view.title().into(),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.go(view, window, cx)));
            rail = rail.child(self.side_move(view.title(), button.into_any_element(), cx));
        }
        rail.child(div().flex_1())
            .child(
                rail_button(
                    "rail-shortcuts".into(),
                    Icon::Shortcuts,
                    self.help,
                    false,
                    format!("Shortcuts · {}", keys::HELP).into(),
                )
                .on_click(cx.listener(|this, _, window, cx| this.set_help(!this.help, window, cx))),
            )
            .child(
                rail_button(
                    "rail-settings".into(),
                    Icon::Settings,
                    false,
                    false,
                    format!("Settings · {} ,", keys::MOD).into(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_settings(settings::Section::Appearance, window, cx)
                })),
            )
            .into_any_element()
    }

    /// The projects, from the rail: pick one, or open another.
    pub(super) fn projects_menu(&self, cx: &mut Context<Self>) -> Div {
        let mut panel = menu_panel(260.).debug_selector(|| "projects-menu".into());
        for i in self.model.recent_projects() {
            let p = &self.model.projects[i];
            let current = i == self.model.current;
            panel = panel.child(
                option_row(SharedString::from(format!("menu-project-{i}")))
                    .debug_selector(move || format!("menu-project-{i}"))
                    .child(icon(Icon::Project, 14., TEXT_3))
                    .child(div().flex_1().truncate().child(capitalized(&p.name)))
                    .when(p.catching_up, |d| d.child(catching_up(i)))
                    .when(p.inbox > 0, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(color(TEXT_4))
                                .child(p.inbox.to_string()),
                        )
                    })
                    .child(check(current))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.model.select_project(i);
                        this.sync_filter(window, cx);
                        this.close_menu(window, cx);
                    })),
            );
        }
        panel.child(menu_divider()).child(
            option_row("menu-open-project".into())
                .debug_selector(|| "menu-open-project".into())
                .child(icon(Icon::OpenProject, 14., TEXT_3))
                .child("Open another project…")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_menu(window, cx);
                    this.open_folder(cx);
                })),
        )
    }

    /// The bar across the window's top: on macOS the traffic lights at its left, and a press on
    /// it drags the window (a double-click zooms it, as the system's titlebar does). In its
    /// middle, the palette's field at rest: what it searches; a click, or its
    /// keys, opens the palette. At its right end, the panels' toggles.
    pub(super) fn top_bar(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let field = div()
            .id("top-field")
            .debug_selector(|| "top-field".into())
            .w(px(TOP_FIELD_W))
            .max_w_full()
            .min_w_0()
            .h(px(30.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            // Edged as the content box is: its own black, its hairline.
            .rounded_lg()
            .bg(color(CONTENT))
            .border_1()
            .border_color(color(BORDER))
            .text_sm()
            .text_color(color(TEXT_3))
            .relative()
            // Its hover comes up and goes in a breath: a lighter layer over the field (under its
            // text), fading, so the field itself (what the pointer is over) never changes.
            .child({
                let (over, epoch) = (self.top_field_hover, self.top_field_epoch);
                div()
                    .absolute()
                    .top(px(-1.))
                    .left(px(-1.))
                    .right(px(-1.))
                    .bottom(px(-1.))
                    .rounded_lg()
                    .bg(color(HOVER))
                    .border_1()
                    .border_color(color(BORDER))
                    .opacity(if over { 1. } else { 0. })
                    .with_animation(
                        SharedString::from(format!("top-field-{epoch}")),
                        Animation::new(if epoch == 0 {
                            Duration::ZERO
                        } else {
                            HOVER_FADE
                        })
                        .with_easing(ease_out_cubic),
                        move |el, t| el.opacity(if over { t } else { 1. - t }),
                    )
            })
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if this.top_field_hover != *over {
                    this.top_field_hover = *over;
                    this.top_field_epoch += 1;
                    cx.notify();
                }
            }))
            .child(icon(Icon::Search, 15., TEXT_4))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child("Search tasks and commands"),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(color(TEXT_4))
                    .child(keys::PALETTE),
            )
            // Its press is its own: it never drags the window.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.toggle_palette(window, cx)));
        let titlebar = titlebar_height(window) > 0.;
        let open = self.menu == Some(Menu::Projects);
        let project = self.model.project();
        let catching = project.is_some_and(|p| p.catching_up);
        let name = project.map_or("No project".to_owned(), |p| capitalized(&p.name));
        let project_button = div()
            .id("top-project")
            .debug_selector(|| "top-project".into())
            .h(px(30.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_lg()
            .text_base()
            .font_weight(FontWeight::MEDIUM)
            .text_color(color(TEXT_2))
            .when(open, |d| d.bg(color(SELECTED)))
            .when(!open, |d| d.hover(|h| h.bg(color(HOVER))))
            .child(div().max_w(px(220.)).truncate().child(name))
            .when(catching, |d| d.child(catching_up(self.model.current)))
            // Its press is its own: it never drags the window.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_menu(Menu::Projects, window, cx);
                }),
            );
        div()
            .id("top-bar")
            .debug_selector(|| "top-bar".into())
            .flex_none()
            .w_full()
            .h(px(TOP_BAR))
            .relative()
            .when(titlebar, |d| {
                d.on_mouse_down(MouseButton::Left, |e: &MouseDownEvent, window, _| {
                    if e.click_count == 2 {
                        window.titlebar_double_click();
                    } else {
                        window.start_window_move();
                    }
                })
            })
            // At its left, past the traffic lights: the open project, and the recent ones under it.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(if titlebar { TRAFFIC_ROOM } else { 12. }))
                    .flex()
                    .items_center()
                    .child(div().relative().child(project_button).when(open, |d| {
                        let panel = self.projects_menu(cx);
                        d.child(self.popover(Menu::Projects, false, 34., panel, cx))
                    })),
            )
            // Centred on the window, whatever sits at either end.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .px(px(120.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(field),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .pr_3()
                    .flex()
                    .items_center()
                    .child(panel_toggle(
                        "toggle-left",
                        Icon::PanelLeft,
                        !self.rail(window),
                        format!("Sidebar · {}", keys::SIDEBAR),
                        cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)),
                    ))
                    .child(panel_toggle(
                        "toggle-bottom",
                        Icon::PanelBottom,
                        self.bottom_open,
                        format!("Bottom bar · {}", keys::BOTTOM),
                        cx.listener(|this, _, _, cx| this.toggle_bottom(cx)),
                    ))
                    .child(panel_toggle(
                        "toggle-right",
                        Icon::PanelRight,
                        self.panel_open(),
                        format!("Detail panel · {}", keys::PANEL),
                        cx.listener(|this, _, _, cx| this.toggle_panel(cx)),
                    )),
            )
    }
}

/// The glow on an edge at the pointer: a lens of light, accent at its middle and thickest
/// there, thinning and fading to nothing at both ends, and the handle on it. Three pills of
/// light make the lens, each shorter and wider than the last; where they overlap the light is
/// strongest. Drawn last (deferred), so it lies over the panes on either side; it takes no
/// pointer events. While dragging it is brighter.
pub(super) fn edge_glow(h: EdgeHover, dragging: bool, len: f32, leaving: bool) -> AnyElement {
    let half = GLOW_LEN / 2.;
    let at = h.at.clamp(half, (len - half).max(half));
    let strength = if dragging { 1. } else { 0.75 };
    // Down a side edge, or across the bottom one: the same glow turned.
    let across = h.edge.across();
    let pill = |w: f32, len: f32, alpha: f32| {
        let lit = with_alpha(ACCENT, alpha * strength);
        let clear = with_alpha(ACCENT, 0.);
        let (into, out_of) = (
            linear_gradient(
                if across { 90. } else { 180. },
                linear_color_stop(clear, 0.),
                linear_color_stop(lit, 1.),
            ),
            linear_gradient(
                if across { 90. } else { 180. },
                linear_color_stop(lit, 0.),
                linear_color_stop(clear, 1.),
            ),
        );
        let d = div().absolute().flex();
        if across {
            d.left(px((GLOW_LEN - len) / 2.))
                .top(px((HANDLE - w) / 2.))
                .w(px(len))
                .h(px(w))
                .child(div().flex_1().rounded_l_full().bg(into))
                .child(div().flex_1().rounded_r_full().bg(out_of))
        } else {
            d.top(px((GLOW_LEN - len) / 2.))
                .left(px((HANDLE - w) / 2.))
                .w(px(w))
                .h(px(len))
                .flex_col()
                .child(div().flex_1().rounded_t_full().bg(into))
                .child(div().flex_1().rounded_b_full().bg(out_of))
        }
    };
    let glow = div().debug_selector(|| "edge-glow".into()).absolute();
    let glow = if across {
        glow.left(px(at - half))
            .top(px((EDGE_W - HANDLE) / 2.))
            .w(px(GLOW_LEN))
            .h(px(HANDLE))
    } else {
        glow.top(px(at - half))
            .left(px((EDGE_W - HANDLE) / 2.))
            .w(px(HANDLE))
            .h(px(GLOW_LEN))
    };
    let resize = icon(Icon::Resize, 12., TEXT);
    let resize = if across {
        resize.with_transformation(Transformation::rotate(radians(std::f32::consts::FRAC_PI_2)))
    } else {
        resize
    };
    let handle = div().debug_selector(|| "edge-handle".into()).absolute();
    let handle = if across {
        handle.left(px((GLOW_LEN - HANDLE) / 2.)).top_0()
    } else {
        handle.top(px((GLOW_LEN - HANDLE) / 2.)).left_0()
    };
    deferred(
        glow.child(pill(1., GLOW_LEN, 0.9))
            .child(pill(2., GLOW_LEN * 0.66, 0.7))
            .child(pill(4., GLOW_LEN * 0.4, 0.6))
            .child(
                handle
                    .size(px(HANDLE))
                    .rounded_full()
                    // A ring of the window's own black sets it off the content box's border.
                    .border_2()
                    .border_color(color(WINDOW))
                    .bg(color(if dragging { ACCENT_HOVER } else { ACCENT }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(resize),
            )
            .with_animation(
                SharedString::from(format!(
                    "edge-glow-{}-{}",
                    h.epoch,
                    if leaving { "out" } else { "in" }
                )),
                Animation::new(if leaving { GLOW_OUT } else { GLOW_IN })
                    .with_easing(ease_out_cubic),
                move |el, t| el.opacity(if leaving { 1. - t } else { t }),
            ),
    )
    .into_any_element()
}

/// A button on the collapsed sidebar's rail: an icon, a dot when its view has work waiting, and
/// its name as a tooltip.
pub(super) fn rail_button(
    id: SharedString,
    glyph: Icon,
    on: bool,
    badge: bool,
    tip: SharedString,
) -> Stateful<Div> {
    let name = id.clone();
    div()
        .id(id)
        .debug_selector(move || name.to_string())
        .relative()
        .size(px(36.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_lg()
        .when(on, |d| d.bg(color(SELECTED)))
        .when(!on, |d| d.hover(|s| s.bg(color(HOVER))))
        .child(icon(glyph, 18., if on { TEXT } else { TEXT_3 }))
        .when(badge, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(6.))
                    .right(px(6.))
                    .size(px(7.))
                    .rounded_full()
                    .bg(color(ACCENT))
                    .debug_selector(|| "rail-badge".into()),
            )
        })
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
}

/// Beside project `i`'s name while its store catches up with its log off the window's thread.
pub(super) fn catching_up(i: usize) -> Div {
    div()
        .debug_selector(move || format!("catching-up-{i}"))
        .flex_none()
        .whitespace_nowrap()
        .text_xs()
        .text_color(color(TEXT_4))
        .child("Catching up…")
}

pub(super) fn nav_item(
    id: SharedString,
    glyph: Icon,
    label: &str,
    active: bool,
    count: usize,
) -> Stateful<Div> {
    let name = id.clone();
    div()
        .id(id)
        .debug_selector(move || name.to_string())
        .group("nav")
        .px_2()
        .h(px(NAV_ROW))
        .text_size(px(NAV_TEXT))
        .rounded_lg()
        .flex()
        .items_center()
        .gap_2()
        .when(active, |d| d.bg(color(SELECTED)).text_color(color(TEXT)))
        .when(!active, |d| {
            d.text_color(color(TEXT_3))
                .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_2)))
        })
        .child(if active {
            icon(glyph, NAV_ICON, TEXT)
        } else {
            icon_hover(glyph, NAV_ICON, TEXT_4, "nav", TEXT_3)
        })
        .child(div().flex_1().truncate().child(label.to_owned()))
        .when(count > 0, |d| {
            d.child(
                div()
                    .min_w(px(18.))
                    .h(px(18.))
                    .px(px(5.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(color(ACCENT))
                    .text_color(color(TEXT))
                    .text_xs()
                    // The unseen count stops at its cap; past 99 a badge says so.
                    .child(if count > 99 {
                        "99+".to_owned()
                    } else {
                        format!("{count}")
                    }),
            )
        })
}
