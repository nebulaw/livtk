//! The panes: the middle of the window as a split of panes (docs/design.md), each a view in its
//! own content box, a divider between them to drag. The focused pane is drawn as the app always
//! drew its one view (the list's keys, the terminal, the doc editor, the dock); the others are
//! drawn as they stand, and a press in one focuses it. A placeholder pane offers the views.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::icons::{Icon, icon, icon_hover};
use super::theme::*;
use super::{Edge, LivtkApp, View, empty_action, view_icon};
use std::collections::HashMap;

use crate::model::{Axis, Split};

/// The gutter between two panes, where the divider is dragged.
const GUTTER: f32 = 8.;
/// The least share of its split a pane is dragged down to.
const MIN_SHARE: f32 = 0.12;

impl LivtkApp {
    /// The panes as laid out, `focused` (the focused pane's content, keys and all) in its place.
    pub(super) fn pane_tree(
        &mut self,
        focused: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // A pane closing: the layout it left, a moment longer, while it shrinks away.
        let split = match &self.pane_gone {
            Some((was, _)) => was.clone(),
            None => self.model.shown_split(),
        };
        let mut focused = Some(focused);
        self.split_element(&split, &mut Vec::new(), &mut focused, window, cx)
    }

    fn split_element(
        &mut self,
        split: &Split,
        path: &mut Vec<usize>,
        focused: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (axis, parts) = match split {
            Split::Pane(id) => return self.pane_box(*id, focused, window, cx),
            Split::Split { axis, parts } => (*axis, parts),
        };
        let row = axis == Axis::Row;
        let key = path.clone();
        let measured = key.clone();
        let mut out = div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .relative()
            .flex()
            .when(!row, |d| d.flex_col())
            // Where the split is drawn, for a divider dragged across it.
            .child({
                let this = cx.entity().downgrade();
                canvas(
                    |_, _, _| {},
                    move |bounds, _, _, cx| {
                        let _ = this.update(cx, |app, _| {
                            app.split_bounds
                                .borrow_mut()
                                .insert(measured.clone(), bounds);
                        });
                    },
                )
                .absolute()
                .size_full()
            });
        for (i, (part, share)) in parts.iter().enumerate() {
            if i > 0 {
                out = out.child(self.divider(key.clone(), i - 1, axis, window, cx));
            }
            path.push(i);
            let inner = self.split_element(part, path, focused, window, cx);
            path.pop();
            let mut cell = div().min_w_0().min_h_0().flex().child(inner);
            // Shares as growth from nothing, so the gutters take their room first.
            cell.style().flex_grow = Some(*share);
            cell.style().flex_shrink = Some(1.);
            cell.style().flex_basis = Some(px(0.).into());
            out = out.child(self.pane_motion(part, *share, cell, cx));
        }
        out.into_any_element()
    }

    /// A pane split off grows from nothing to its share as its neighbours give way, fading in;
    /// a pane closed shrinks from its share to nothing, fading out. Once each; with less motion
    /// asked for, neither.
    fn pane_motion(
        &self,
        part: &Split,
        share: f32,
        cell: Div,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Split::Pane(id) = part else {
            return cell.into_any_element();
        };
        let id = *id;
        let born = self.pane_born == Some(id);
        let gone = self.pane_gone.as_ref().is_some_and(|(_, g)| *g == id);
        if cx.reduce_motion() || !(born || gone) {
            return cell.into_any_element();
        }
        let (name, duration) = if born {
            (format!("pane-in-{id}-{}", self.pane_epoch), super::SLIDE)
        } else {
            (
                format!("pane-out-{id}-{}", self.pane_epoch),
                super::SLIDE_OUT,
            )
        };
        cell.with_animation(
            SharedString::from(name),
            Animation::new(duration).with_easing(if born {
                super::ease_out_cubic
            } else {
                super::ease_in_out
            }),
            move |mut el, t| {
                let open = if born { t } else { 1. - t };
                el.style().flex_grow = Some(share * open);
                el.opacity(0.2 + 0.8 * open)
            },
        )
        .into_any_element()
    }

    /// After `duration`, the pane animation of `epoch` is over (unless another began).
    fn end_pane_motion(&self, epoch: usize, duration: std::time::Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |app, cx| {
                if app.pane_epoch == epoch {
                    app.pane_born = None;
                    app.pane_gone = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Pane `id` about to close from layout `was`: it shrinks away there before it is gone.
    fn pane_closing(&mut self, was: Split, id: u64, cx: &mut Context<Self>) {
        if was.ids().len() < 2 || !was.ids().contains(&id) {
            return;
        }
        self.pane_epoch += 1;
        self.pane_born = None;
        self.pane_gone = Some((was, id));
        self.end_pane_motion(self.pane_epoch, super::SLIDE_OUT, cx);
    }

    /// The gutter between part `before` and the next of the split at `path`: an edge as the
    /// sidebar's is (a glow following the pointer, the handle on it, a double-click evening
    /// the two out), dragged to move the room between them.
    fn divider(
        &self,
        path: Vec<usize>,
        before: usize,
        axis: Axis,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = axis == Axis::Row;
        let Some(edge) = self.divider_edge(&path, before, axis) else {
            return div().into_any_element();
        };
        div()
            .flex_none()
            .relative()
            .when(row, |d| d.w(px(GUTTER)).h_full())
            .when(!row, |d| d.h(px(GUTTER)).w_full())
            .child(self.edge(edge, window, cx).left_0().top_0())
            .into_any_element()
    }

    /// The edge of the divider after part `before` of the split at `path`, named by the first
    /// pane on either side of it, which stays the same while the split is dragged.
    fn divider_edge(&self, path: &[usize], before: usize, axis: Axis) -> Option<Edge> {
        let mut split = self.model.split.clone();
        let Split::Split { parts, .. } = split.at_mut(path)? else {
            return None;
        };
        let first = |s: &Split| s.ids().first().copied();
        Some(Edge::Divider {
            a: first(&parts.get(before)?.0)?,
            b: first(&parts.get(before + 1)?.0)?,
            across: axis == Axis::Column,
        })
    }

    /// Where the divider between the parts led by panes `a` and `b` is: its split's path, the
    /// part before it, and that split's bounds as drawn last.
    fn divider_at(&self, a: u64, b: u64) -> Option<(Vec<usize>, usize, Bounds<Pixels>)> {
        fn find(s: &Split, a: u64, b: u64, path: &mut Vec<usize>) -> Option<(Vec<usize>, usize)> {
            let Split::Split { parts, .. } = s else {
                return None;
            };
            let first = |s: &Split| s.ids().first().copied();
            for i in 0..parts.len().saturating_sub(1) {
                if first(&parts[i].0) == Some(a) && first(&parts[i + 1].0) == Some(b) {
                    return Some((path.clone(), i));
                }
            }
            for (i, (p, _)) in parts.iter().enumerate() {
                path.push(i);
                if let Some(found) = find(p, a, b, path) {
                    return Some(found);
                }
                path.pop();
            }
            None
        }
        let (path, before) = find(&self.model.split, a, b, &mut Vec::new())?;
        let bounds = self.split_bounds.borrow().get(&path).copied()?;
        Some((path, before, bounds))
    }

    /// How long the divider is (its split's height across panes side by side, its width
    /// across stacked ones), and how far along it the window's point `p` is.
    pub(super) fn divider_along(&self, edge: Edge, p: Option<Point<Pixels>>) -> (f32, f32) {
        let Edge::Divider { a, b, across } = edge else {
            return (0., 0.);
        };
        let Some((_, _, bounds)) = self.divider_at(a, b) else {
            return (0., 0.);
        };
        let (len, at) = if across {
            (
                f32::from(bounds.size.width),
                p.map_or(0., |p| f32::from(p.x - bounds.origin.x)),
            )
        } else {
            (
                f32::from(bounds.size.height),
                p.map_or(0., |p| f32::from(p.y - bounds.origin.y)),
            )
        };
        (len, at)
    }

    /// The divider `edge` dragged to the window's point `at`.
    pub(super) fn drag_divider_edge(&mut self, edge: Edge, at: Point<Pixels>) {
        if let Edge::Divider { a, b, .. } = edge
            && let Some((path, before, bounds)) = self.divider_at(a, b)
        {
            self.drag_divider(&path, before, bounds, at);
        }
    }

    /// The two parts either side of divider `edge` given even room again.
    pub(super) fn even_divider(&mut self, edge: Edge) {
        let Edge::Divider { a, b, .. } = edge else {
            return;
        };
        let Some((path, before, _)) = self.divider_at(a, b) else {
            return;
        };
        if let Some(Split::Split { parts, .. }) = self.model.split.at_mut(&path)
            && before + 1 < parts.len()
        {
            let pair = parts[before].1 + parts[before + 1].1;
            parts[before].1 = pair / 2.;
            parts[before + 1].1 = pair / 2.;
        }
    }

    /// The divider after part `before` of the split at `path` dragged to `at`.
    fn drag_divider(
        &mut self,
        path: &[usize],
        before: usize,
        bounds: Bounds<Pixels>,
        at: Point<Pixels>,
    ) {
        let Some(Split::Split { axis, parts }) = self.model.split.at_mut(path) else {
            return;
        };
        if before + 1 >= parts.len() {
            return;
        }
        let (start, size, pos) = match axis {
            Axis::Row => (bounds.origin.x, bounds.size.width, at.x),
            Axis::Column => (bounds.origin.y, bounds.size.height, at.y),
        };
        let size = f32::from(size).max(1.);
        let along = (f32::from(pos - start) / size).clamp(0., 1.);
        let first: f32 = parts[..before].iter().map(|(_, f)| f).sum();
        let pair = parts[before].1 + parts[before + 1].1;
        let mine = (along - first).clamp(MIN_SHARE.min(pair / 2.), pair - MIN_SHARE.min(pair / 2.));
        parts[before].1 = mine;
        parts[before + 1].1 = pair - mine;
    }

    /// Pane `id` in its content box: the focused one's own content, else the view it shows,
    /// drawn as it stands. A press in it focuses it.
    fn pane_box(
        &mut self,
        id: u64,
        focused: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(i) = self.model.pane_index(id) else {
            // The pane closing, drawn empty as it shrinks away.
            return div()
                .size_full()
                .rounded_xl()
                .border_1()
                .border_color(color(BORDER))
                .bg(color(CONTENT))
                .into_any_element();
        };
        let is_focused = i == self.model.focused();
        let several = self.model.shown_split().ids().len() > 1;
        let inner = if is_focused {
            focused.take().unwrap_or_else(|| div().into_any_element())
        } else {
            let was = self.model.act_on(i);
            let el = self.unfocused_pane(window, cx);
            self.model.act_on(was);
            el
        };
        let this = cx.entity().downgrade();
        div()
            .id(SharedString::from(format!("pane-{id}")))
            .debug_selector(move || {
                if is_focused {
                    "content-box".into()
                } else {
                    format!("pane-{id}")
                }
            })
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .rounded_xl()
            .border_1()
            .border_color(color(if is_focused && several {
                BORDER_STRONG
            } else {
                BORDER
            }))
            .bg(color(CONTENT))
            .overflow_hidden()
            .child(inner)
            // Where it is drawn, for its view's room (the dock, the reader).
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, _, cx| {
                        let _ = this.update(cx, |app, _| {
                            app.pane_bounds.borrow_mut().insert(id, bounds);
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .when(!is_focused, |d| {
                d.capture_any_mouse_down(cx.listener(move |this, _, window, cx| {
                    this.focus_pane(i, window, cx);
                }))
            })
            .into_any_element()
    }

    /// Give pane `i` the keys, the sidebar and the dock.
    pub(super) fn focus_pane(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.model.focus_pane(i);
        self.glide = None;
        self.sync_filter(window, cx);
        self.focus_main(window, cx);
        cx.notify();
    }

    /// A pane not focused: its view as it stands (the terminal's screen, the doc's editor, the
    /// list), its keys left to the focused one.
    fn unfocused_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.model.view() == View::Docs
            && let Some(editor) = self.doc_editor(cx)
        {
            return editor;
        }
        let content = match self.terminal_view(cx) {
            Some(t) => self.terminal_element(t, cx),
            None => self.content(window, cx),
        };
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(content)
            .into_any_element()
    }

    /// A pane waiting for its view: each view to put in it (one shown elsewhere goes to its
    /// pane), and a way to close it.
    pub(super) fn placeholder(&self, cx: &mut Context<Self>) -> AnyElement {
        let shown: Vec<View> = self
            .model
            .panes()
            .iter()
            .filter(|p| !p.placeholder)
            .map(|p| p.view)
            .collect();
        let mut grid = div()
            .flex()
            .flex_wrap()
            .justify_center()
            .gap_2()
            .max_w(px(420.));
        for view in View::ALL {
            let id = format!("place-{}", view.title().to_lowercase());
            let elsewhere = shown.contains(&view);
            grid = grid.child(
                div()
                    .id(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
                    .group("place")
                    .w(px(124.))
                    .h(px(64.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(color(BORDER))
                    .text_xs()
                    .text_color(color(if elsewhere { TEXT_4 } else { TEXT_2 }))
                    .hover(|h| h.bg(color(HOVER)).border_color(color(BORDER_STRONG)))
                    .child(icon_hover(view_icon(view), 18., TEXT_3, "place", TEXT))
                    .child(view.title())
                    .when(elsewhere, |d| {
                        d.tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(
                                "Shown in another pane: goes there",
                            )
                            .build(window, cx)
                        })
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.go(view, window, cx);
                    })),
            );
        }
        div()
            .debug_selector(|| "placeholder".into())
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .child(icon(Icon::Grouping, 22., TEXT_4))
            .child(
                div()
                    .text_color(color(TEXT_2))
                    .child("Choose what this pane shows"),
            )
            .child(grid)
            .child(empty_action(
                "placeholder-close",
                "Close pane",
                cx.listener(|this, _, window, cx| this.close_pane(window, cx)),
            ))
            .into_any_element()
    }

    pub(super) fn split_pane(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.model.split.ids();
        self.model.split_pane(axis);
        // The pane that joined the split grows in.
        if let Some(&new) = self
            .model
            .split
            .ids()
            .iter()
            .find(|id| !before.contains(id))
        {
            self.pane_epoch += 1;
            self.pane_gone = None;
            self.pane_born = Some(new);
            self.end_pane_motion(self.pane_epoch, super::SLIDE, cx);
        }
        self.glide = None;
        self.sync_filter(window, cx);
        cx.notify();
    }

    pub(super) fn close_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let was = self.model.shown_split();
        let id = self.model.focused_id();
        let solo = self.model.solo();
        self.model.close_pane();
        if !solo {
            self.pane_closing(was, id, cx);
        }
        self.glide = None;
        self.sync_filter(window, cx);
        self.focus_main(window, cx);
        cx.notify();
    }

    pub(super) fn next_pane(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.model.focus_next_pane(delta);
        self.glide = None;
        self.sync_filter(window, cx);
        self.focus_main(window, cx);
        cx.notify();
    }

    /// The split's group at the sidebar's top: each pane's view in the split's order, the
    /// focused one lit; a click focuses its pane, its unsplit button closes it (the view goes
    /// back to its usual place). Rows slide in as they join.
    pub(super) fn sidebar_split(
        &self,
        split: &[(u64, Option<View>)],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if split.is_empty() {
            return None;
        }
        let focused = (!self.model.solo()).then(|| self.model.focused_id());
        let motion = if cx.reduce_motion() {
            std::time::Duration::ZERO
        } else {
            super::SLIDE
        };
        let mut group = div()
            .debug_selector(|| "sidebar-split".into())
            .p_1()
            .flex()
            .flex_col()
            .gap(px(2.))
            .rounded_xl()
            .bg(color(SURFACE))
            .border_1()
            .border_color(color(BORDER))
            .child(
                div()
                    .px_2()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(TEXT_4))
                    .child(icon(Icon::Grouping, 12., TEXT_4))
                    .child("Split"),
            );
        for &(id, view) in split {
            let Some(i) = self.model.pane_index(id) else {
                continue;
            };
            let active = focused == Some(id);
            let (glyph, label) = match view {
                Some(v) => (view_icon(v), v.title()),
                None => (Icon::Grouping, "Empty pane"),
            };
            let name = view.map_or("empty".to_owned(), |v| v.title().to_owned());
            let row_id = format!("split-{name}");
            let unsplit = format!("unsplit-{name}");
            let row = super::nav_item(SharedString::from(row_id), glyph, label, active, 0)
                .group("split-row")
                .child(
                    div()
                        .id(SharedString::from(unsplit.clone()))
                        .debug_selector(move || unsplit.clone())
                        .group("unsplit")
                        .size(px(22.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_lg()
                        .invisible()
                        .group_hover("split-row", |s| s.visible())
                        .hover(|h| h.bg(color(HOVER)))
                        .child(icon_hover(Icon::Close, 13., TEXT_4, "unsplit", TEXT))
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new("Unsplit: close this pane")
                                .build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.unsplit(id, window, cx);
                        })),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.focus_pane(i, window, cx);
                }));
            group = group.child(self.side_move(&side_key(id, view), row.into_any_element(), cx));
        }
        Some(
            group
                .with_animation(
                    SharedString::from(format!("split-group-{}", split[0].0)),
                    Animation::new(motion).with_easing(super::ease_out_cubic),
                    |el, t| el.opacity(0.4 + 0.6 * t),
                )
                .into_any_element(),
        )
    }

    /// The split's group on the collapsed sidebar's rail: each pane's view as a rail button
    /// in a box, the focused one lit; a click focuses its pane, the × at its corner (on hover)
    /// unsplits it. Buttons fade in as they join.
    pub(super) fn rail_split(
        &self,
        split: &[(u64, Option<View>)],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if split.is_empty() {
            return None;
        }
        let focused = (!self.model.solo()).then(|| self.model.focused_id());
        let mut group = div()
            .debug_selector(|| "rail-split".into())
            .p(px(3.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .rounded_xl()
            .bg(color(SURFACE))
            .border_1()
            .border_color(color(BORDER));
        for &(id, view) in split {
            let Some(i) = self.model.pane_index(id) else {
                continue;
            };
            let (glyph, label) = match view {
                Some(v) => (view_icon(v), v.title()),
                None => (Icon::Grouping, "Empty pane"),
            };
            let name = view.map_or("empty".to_owned(), |v| v.title().to_owned());
            let unsplit = format!("rail-unsplit-{name}");
            let button = super::rail_button(
                SharedString::from(format!("rail-split-{name}")),
                glyph,
                focused == Some(id),
                false,
                SharedString::from(format!("{label} · in the split")),
            )
            .group("rail-split-button")
            .child(
                div()
                    .id(SharedString::from(unsplit.clone()))
                    .debug_selector(move || unsplit.clone())
                    .group("rail-unsplit")
                    .absolute()
                    .top(px(-3.))
                    .right(px(-3.))
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(color(LIFTED))
                    .border_1()
                    .border_color(color(BORDER_STRONG))
                    .invisible()
                    .group_hover("rail-split-button", |s| s.visible())
                    .child(icon_hover(Icon::Close, 10., TEXT_3, "rail-unsplit", TEXT))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.unsplit(id, window, cx);
                    })),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.focus_pane(i, window, cx);
            }));
            group = group.child(self.side_move(&side_key(id, view), button.into_any_element(), cx));
        }
        Some(group.into_any_element())
    }

    /// Pane `id` taken out of the split: closed, its view back in its usual place.
    fn unsplit(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let was = self.model.shown_split();
        self.model.unsplit(id);
        self.pane_closing(was, id, cx);
        self.glide = None;
        self.sync_filter(window, cx);
        self.focus_main(window, cx);
        cx.notify();
    }

    /// The focused pane's width, as drawn last; before that, `fallback`.
    pub(super) fn focused_width(&self, fallback: f32) -> f32 {
        if self.model.shown_split().ids().len() < 2 {
            return fallback;
        }
        let id = self.model.panes()[self.model.focused()].id;
        self.pane_bounds
            .borrow()
            .get(&id)
            .map_or(fallback, |b| f32::from(b.size.width))
    }
}

/// How a line of the sidebar moves as the split changes: from `by` above (negative) or below
/// where it is now, to its place; or in, where it had no place before.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum SideMove {
    By(f32),
    In,
}

/// A sidebar line's name across layouts: its view's, or, for a placeholder, its pane's.
fn side_key(id: u64, view: Option<View>) -> String {
    view.map_or(format!("empty-{id}"), |v| v.title().to_owned())
}

/// Where each line of the sidebar (or of the rail) sits from its top, as it lays them out with
/// `split`'s views grouped: the group (a box, its header on the sidebar), then each view not
/// in it, the spaces and headings between. Only the differences between two layouts are used.
fn side_ys(split: &[(u64, Option<View>)], rail: bool) -> HashMap<String, f32> {
    let gap = 2.;
    let (row, pad, header) = if rail {
        (36., 3., 0.)
    } else {
        (super::NAV_ROW, 4., 22. + gap)
    };
    let mut y = 0.;
    let mut out = HashMap::new();
    let place = |h: f32, y: &mut f32| {
        let top = *y;
        *y += h + gap;
        top
    };
    if !split.is_empty() {
        let n = split.len() as f32;
        let top = place(2. + 2. * pad + header + n * row + (n - 1.) * gap, &mut y);
        for (k, &(id, view)) in split.iter().enumerate() {
            out.insert(
                side_key(id, view),
                top + 1. + pad + header + k as f32 * (row + gap),
            );
        }
        place(10., &mut y);
    }
    for view in View::ALL {
        if matches!(view, View::Tasks | View::Graph) {
            place(14., &mut y);
            if !rail {
                place(24., &mut y);
            }
        }
        if split.iter().any(|(_, v)| *v == Some(view)) {
            continue;
        }
        out.insert(view.title().to_owned(), place(row, &mut y));
    }
    out
}

impl LivtkApp {
    /// The split changed since the sidebar was last laid out: how each line moves from its old
    /// place to its new one. Not on the first layout, nor with less motion asked for.
    pub(super) fn track_side(&mut self, rail: bool, cx: &mut Context<Self>) {
        let now = self.model.split_views();
        let Some(was) = self.side_was.replace(now.clone()) else {
            return;
        };
        if was == now || cx.reduce_motion() {
            return;
        }
        let (before, after) = (side_ys(&was, rail), side_ys(&now, rail));
        self.side_moves = after
            .iter()
            .filter_map(|(key, y)| match before.get(key) {
                Some(old) if (old - y).abs() > 0.5 => Some((key.clone(), SideMove::By(old - y))),
                Some(_) => None,
                None => Some((key.clone(), SideMove::In)),
            })
            .collect();
        self.side_epoch += 1;
        let epoch = self.side_epoch;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(super::SLIDE).await;
            let _ = this.update(cx, |app, cx| {
                if app.side_epoch == epoch {
                    app.side_moves.clear();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Sidebar line `key`, sliding from where it was to its place, or fading in, while the
    /// split's change plays.
    pub(super) fn side_move(
        &self,
        key: &str,
        el: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = cx;
        let Some(&m) = self.side_moves.get(key) else {
            return el;
        };
        div()
            .relative()
            .child(el)
            .with_animation(
                SharedString::from(format!("side-{key}-{}", self.side_epoch)),
                Animation::new(super::SLIDE).with_easing(super::ease_out_cubic),
                move |d, t| match m {
                    SideMove::By(by) => d.top(px(by * (1. - t))),
                    SideMove::In => d.opacity(t),
                },
            )
            .into_any_element()
    }
}
