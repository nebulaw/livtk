//! The panes: the middle of the window as a split of panes (docs/design.md), each a view in its
//! own content box, a divider between them to drag. The focused pane is drawn as the app always
//! drew its one view (the list's keys, the terminal, the doc editor, the dock); the others are
//! drawn as they stand, and a press in one focuses it. A placeholder pane offers the views.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::icons::{Icon, icon, icon_hover};
use super::theme::*;
use super::{LivtkApp, View, empty_action, view_icon};
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
        let split = self.model.split.clone();
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
                out = out.child(self.divider(key.clone(), i - 1, axis, cx));
            }
            path.push(i);
            let inner = self.split_element(part, path, focused, window, cx);
            path.pop();
            let mut cell = div().min_w_0().min_h_0().flex().child(inner);
            // Shares as growth from nothing, so the gutters take their room first.
            cell.style().flex_grow = Some(*share);
            cell.style().flex_shrink = Some(1.);
            cell.style().flex_basis = Some(px(0.).into());
            out = out.child(cell);
        }
        out.into_any_element()
    }

    /// The gutter between part `before` and the next of the split at `path`: dragged, it moves
    /// the room between them.
    fn divider(
        &self,
        path: Vec<usize>,
        before: usize,
        axis: Axis,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = axis == Axis::Row;
        let id = format!("divider-{}-{before}", path_name(&path));
        let dragging = self.split_drag.as_ref() == Some(&(path.clone(), before));
        div()
            .id(SharedString::from(id.clone()))
            .debug_selector(move || id.clone())
            .flex_none()
            .when(row, |d| d.w(px(GUTTER)).h_full())
            .when(!row, |d| d.h(px(GUTTER)).w_full())
            .flex()
            .items_center()
            .justify_center()
            .group("divider")
            .cursor(if row {
                CursorStyle::ResizeLeftRight
            } else {
                CursorStyle::ResizeUpDown
            })
            .child(
                div()
                    .rounded_full()
                    .when(row, |d| d.w(px(2.)).h(px(32.)))
                    .when(!row, |d| d.h(px(2.)).w(px(32.)))
                    .bg(color(if dragging { ACCENT } else { BORDER_STRONG }))
                    .invisible()
                    .group_hover("divider", |s| s.visible())
                    .when(dragging, |d| d.visible()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.split_drag = Some((path.clone(), before));
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    /// While a divider is dragged: the pointer anywhere in the window moves it, and letting go
    /// ends it.
    pub(super) fn split_drag_listener(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (path, before) = self.split_drag.clone()?;
        let this = cx.entity().downgrade();
        Some(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let moved = this.clone();
                    let path = path.clone();
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                        if phase != DispatchPhase::Capture {
                            return;
                        }
                        let _ = moved.update(cx, |app, cx| {
                            if e.pressed_button != Some(MouseButton::Left) {
                                app.split_drag = None;
                            } else {
                                app.drag_divider(&path, before, e.position);
                            }
                            cx.notify();
                        });
                    });
                    let released = this.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                        if phase != DispatchPhase::Capture {
                            return;
                        }
                        let _ = released.update(cx, |app, cx| {
                            app.split_drag = None;
                            cx.notify();
                        });
                    });
                },
            )
            .absolute()
            .size_full()
            .into_any_element(),
        )
    }

    /// The divider after part `before` of the split at `path` dragged to `at`.
    fn drag_divider(&mut self, path: &[usize], before: usize, at: Point<Pixels>) {
        let Some(bounds) = self.split_bounds.borrow().get(path).copied() else {
            return;
        };
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
            return div().into_any_element();
        };
        let is_focused = i == self.model.focused();
        let several = self.model.panes().len() > 1;
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
            Some(t) => div().flex_1().min_h_0().child(t).into_any_element(),
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
        self.model.split_pane(axis);
        self.glide = None;
        self.sync_filter(window, cx);
        cx.notify();
    }

    pub(super) fn close_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model.close_pane();
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

    /// The focused pane's width, as drawn last; before that, `fallback`.
    pub(super) fn focused_width(&self, fallback: f32) -> f32 {
        if self.model.panes().len() < 2 {
            return fallback;
        }
        let id = self.model.panes()[self.model.focused()].id;
        self.pane_bounds
            .borrow()
            .get(&id)
            .map_or(fallback, |b| f32::from(b.size.width))
    }
}

/// A split's path as a name: "0-1" (the root's is "root").
fn path_name(path: &[usize]) -> String {
    if path.is_empty() {
        "root".into()
    } else {
        path.iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join("-")
    }
}
