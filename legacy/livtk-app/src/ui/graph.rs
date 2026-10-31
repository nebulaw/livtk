//! The graph view: cards, links, the camera.

use super::*;

impl LivtkApp {
    pub(super) fn zoom_graph(&mut self, factor: f32, cx: &mut Context<Self>) {
        if self.model.view() == View::Graph {
            self.model.graph_zoom(factor, None);
            cx.notify();
        }
    }

    /// A pointer position in the graph's own coordinates.
    pub(super) fn graph_point(&self, p: Point<Pixels>) -> (f32, f32) {
        let o = self.graph_bounds.get().origin;
        (f32::from(p.x - o.x), f32::from(p.y - o.y))
    }

    /// The graph: links on a canvas, cards over them, only what is in view. Drag or scroll to
    /// pan, Ctrl+scroll to zoom at the pointer, click a card to open it.
    pub(super) fn graph_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let drawn = self.graph_bounds.get().size;
        self.model
            .graph_viewport(f32::from(drawn.width), f32::from(drawn.height));
        if self.model.graph.nodes.is_empty() {
            return div()
                .size_full()
                .child(empty(
                    Icon::Graph,
                    "Nothing to draw yet",
                    "Tasks, lessons, decisions and findings show here with the links between them.",
                ))
                .into_any_element();
        }
        let g = &self.model.graph;
        let cam = g.camera;
        let z = cam.zoom;
        let (vw, vh) = g.viewport();
        // Before the first frame the size is unknown: draw everything once to learn it.
        let known = vw > 0. && vh > 0.;
        let focus = g.focus(self.model.detail.is_some());
        let near = focus.map(|i| g.neighbours(i)).unwrap_or_default();
        // As `Graph::lit`, with the neighbours worked out once for the frame.
        let lit = |i: usize| focus.is_none_or(|f| f == i || near.contains(&i));
        let in_view = |x: f32, y: f32, w: f32, h: f32| {
            !known || (x + w >= 0. && y + h >= 0. && x <= vw && y <= vh)
        };

        // Links as the stretches of their curves that show, gathered by how they are drawn,
        // and the arrowheads whose tips show. A long link costs only what is in view.
        let mut strokes: [Vec<Vec<(f32, f32)>>; LINK_STYLES] = Default::default();
        let mut heads: [Vec<Arrowhead>; LINK_STYLES] = Default::default();
        for e in &g.edges {
            let ((x0, y0), (x1, y1), across) = g.ends(e);
            let (a, b) = (cam.to_screen(x0, y0), cam.to_screen(x1, y1));
            let (lx, ly) = (a.0.min(b.0), a.1.min(b.1));
            if !in_view(lx, ly, (a.0 - b.0).abs(), (a.1 - b.1).abs()) {
                continue;
            }
            let runs = if known {
                visible_runs(&link_curve(a, b, across), vw, vh, 16.)
            } else {
                vec![link_curve(a, b, across)]
            };
            if runs.is_empty() {
                continue;
            }
            let touches = focus.is_some_and(|f| e.from == f || e.to == f);
            let dim = focus.is_some() && !touches;
            let style = match (touches, e.orders(), dim) {
                (true, _, _) => 0,
                (false, true, false) => 1,
                (false, true, true) => 2,
                (false, false, false) => 3,
                (false, false, true) => 4,
            };
            strokes[style].extend(runs);
            if e.orders() && in_view(b.0 - 10., b.1 - 10., 20., 20.) {
                let dir = if across {
                    ((b.0 - a.0).signum(), 0.)
                } else {
                    (0., (b.1 - a.1).signum())
                };
                heads[style].push((b, dir));
            }
        }
        let dots = z < DOTS_BELOW;
        let mut dot_quads = Vec::new();
        let mut cards = Vec::new();
        for (i, n) in g.nodes.iter().enumerate() {
            let (sx, sy) = cam.to_screen(n.x, n.y);
            let (w, h) = (CARD_W * z, CARD_H * z);
            if !in_view(sx, sy, w, h) {
                continue;
            }
            let on = g.selected == Some(i);
            if dots {
                dot_quads.push((sx, sy, w, h, state_color(n.kind, n.state), on, lit(i)));
                continue;
            }
            let key = n.key.clone();
            cards.push(
                div()
                    .debug_selector(move || format!("gnode-{key}"))
                    .absolute()
                    .left(px(sx))
                    .top(px(sy))
                    .w(px(w))
                    .h(px(h))
                    .px(px(10. * z))
                    .flex()
                    .items_center()
                    .gap(px(8. * z))
                    // As the board's cards, scaled with the zoom.
                    .rounded(px(8. * z))
                    .border_1()
                    .bg(color(if on { SELECTED } else { SURFACE }))
                    .border_color(color(if on { ACCENT } else { BORDER }))
                    .text_size(px(13. * z))
                    .line_height(px(18. * z))
                    .when(!lit(i), |d| d.opacity(0.35))
                    .child(state_dot(n.kind, n.state, 10. * z))
                    .child(
                        div()
                            .flex_none()
                            .text_color(color(TEXT_4))
                            .child(n.key.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(color(if on { TEXT } else { TEXT_2 }))
                            .child(n.title.clone()),
                    )
                    .into_any_element(),
            );
        }
        let unlinked = g.unlinked_top.filter(|_| !dots).map(|top| {
            let (sx, sy) = cam.to_screen(0., top - 26.);
            div()
                .absolute()
                .left(px(sx))
                .top(px(sy))
                .text_size(px(12. * z))
                .font_weight(FontWeight::MEDIUM)
                .text_color(color(TEXT_4))
                .child("Not linked")
        });
        let bounds = self.graph_bounds.clone();
        let edges = canvas(
            move |b, window, _| {
                // The size fits the camera next frame; draw again if it changed.
                if bounds.get() != b {
                    bounds.set(b);
                    window.refresh();
                }
            },
            move |b, (), window, _| {
                let o = b.origin;
                let at = |(x, y): (f32, f32)| point(o.x + px(x), o.y + px(y));
                // A faint dot grid that moves with the world, so panning is felt; its spacing
                // doubles as the zoom falls, so it never crowds.
                let mut step = GRID_STEP * z;
                while step < 14. {
                    step *= 2.;
                }
                let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
                let dot = with_alpha(BORDER_STRONG, 0.45);
                let mut y = cam.pan_y.rem_euclid(step);
                while y < h {
                    let mut x = cam.pan_x.rem_euclid(step);
                    while x < w {
                        window
                            .paint_quad(fill(Bounds::new(at((x, y)), size(px(1.5), px(1.5))), dot));
                        x += step;
                    }
                    y += step;
                }
                let width = px((1.5 * z).clamp(0.75, 2.));
                let colours = [
                    with_alpha(ACCENT_HOVER, 0.9),
                    with_alpha(TEXT_4, 0.85),
                    with_alpha(TEXT_4, 0.25),
                    with_alpha(BORDER_STRONG, 0.9),
                    with_alpha(BORDER_STRONG, 0.25),
                ];
                // One path per style: every stretch of every link drawn that way.
                for (style, runs) in strokes.iter().enumerate() {
                    if runs.is_empty() {
                        continue;
                    }
                    let mut line = PathBuilder::stroke(width);
                    for run in runs {
                        line.move_to(at(run[0]));
                        for &p in &run[1..] {
                            line.line_to(at(p));
                        }
                    }
                    if let Ok(path) = line.build() {
                        window.paint_path(path, colours[style]);
                    }
                }
                let s = (7. * z).clamp(3.5, 9.);
                for (style, tips) in heads.iter().enumerate() {
                    if tips.is_empty() {
                        continue;
                    }
                    let mut head = PathBuilder::fill();
                    for &(tip, (dx, dy)) in tips {
                        let base = (tip.0 - dx * s, tip.1 - dy * s);
                        let side = (-dy * s * 0.55, dx * s * 0.55);
                        head.move_to(at(tip));
                        head.line_to(at((base.0 + side.0, base.1 + side.1)));
                        head.line_to(at((base.0 - side.0, base.1 - side.1)));
                        head.close();
                    }
                    if let Ok(path) = head.build() {
                        window.paint_path(path, colours[style]);
                    }
                }
                for (x, y, w, h, state, on, lit) in dot_quads {
                    let r = Bounds::new(at((x, y)), size(px(w.max(2.)), px(h.max(2.))));
                    window.paint_quad(quad(
                        r,
                        px((3. * z).max(1.)),
                        with_alpha(state, if lit { 0.9 } else { 0.3 }),
                        px(if on { 1.5 } else { 0. }),
                        color(ACCENT_HOVER),
                        BorderStyle::default(),
                    ));
                }
            },
        )
        .absolute()
        .inset_0();
        // The cursor stays as it is, except while dragging the graph around.
        let dragging = matches!(self.graph_press, Some((_, true)));
        div()
            .id("graph")
            .debug_selector(|| "graph".into())
            .size_full()
            .relative()
            .overflow_hidden()
            .when(dragging, |d| d.cursor(CursorStyle::ClosedHand))
            .child(edges)
            .children(unlinked)
            .children(cards)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, _| {
                    this.graph_press = Some((e.position, false));
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                match (this.graph_press, e.pressed_button) {
                    (Some((last, _)), Some(MouseButton::Left)) => {
                        let d = e.position - last;
                        this.model.graph_pan(f32::from(d.x), f32::from(d.y));
                        this.graph_press = Some((e.position, true));
                        cx.notify();
                    }
                    // A release outside the graph ends a drag without a click. Nothing follows
                    // the pointer otherwise: cards have no hover look.
                    _ => this.graph_press = None,
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, cx| {
                    if let Some((_, false)) = this.graph_press.take() {
                        let (x, y) = this.graph_point(e.position);
                        this.model.graph_click(x, y);
                    }
                    cx.notify();
                }),
            )
            // A trackpad's pinch zooms at its centre, as Cmd+scroll (Ctrl+scroll) does.
            .on_pinch(cx.listener(|this, e: &PinchEvent, _, cx| {
                let (x, y) = this.graph_point(e.position);
                this.model.graph_zoom(1. + e.delta, Some((x, y)));
                cx.notify();
            }))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                let d = e.delta.pixel_delta(px(20.));
                if e.modifiers.control || e.modifiers.platform {
                    let (x, y) = this.graph_point(e.position);
                    this.model
                        .graph_zoom((f32::from(d.y) * 0.004).exp(), Some((x, y)));
                } else {
                    this.model.graph_pan(f32::from(d.x), f32::from(d.y));
                }
                cx.notify();
            }))
            .into_any_element()
    }

    // ---- rendering: lists ---------------------------------------------------------------------
}
