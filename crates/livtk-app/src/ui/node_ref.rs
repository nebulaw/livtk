//! A node named anywhere but its own row: by its state mark, key and title, a click going to it.
//! Links, a doc's tasks, a session's work and a sub-task's parent all name nodes this way, so
//! each goes the same way.

use super::*;

/// Where a node named somewhere opens.
#[derive(Clone, Copy)]
pub(super) enum OpenIn {
    /// In its own view, past whatever hid it there, selected with its panel.
    View,
    /// In the panel, beside what is shown (a doc being read stays read).
    Beside,
}

/// Open the node `key` as `open` says.
fn open_node(model: &mut Model, key: &str, open: OpenIn) {
    match open {
        OpenIn::View => model.go_to(key),
        OpenIn::Beside => model.show_node(key),
    }
}

impl LivtkApp {
    /// A node named in a list of them: a row with its state mark, key and title; a click opens
    /// it. `end` (an unlink button) answers its own clicks.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn node_ref(
        &self,
        selector: String,
        key: &str,
        kind: Kind,
        state: State,
        title: &str,
        open: OpenIn,
        end: Option<Stateful<Div>>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        super::parts::node_row(self.link_item(selector, key, kind, state, title, open, end, cx))
    }

    /// A node as a list of links holds it: a click opens it as `open` says.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn link_item(
        &self,
        selector: String,
        key: &str,
        kind: Kind,
        state: State,
        title: &str,
        open: OpenIn,
        end: Option<Stateful<Div>>,
        cx: &mut Context<Self>,
    ) -> super::parts::LinkItem {
        let (this, go) = (cx.entity().downgrade(), key.to_owned());
        super::parts::LinkItem {
            selector,
            key: key.to_owned(),
            kind,
            state,
            title: title.to_owned(),
            end,
            open: std::rc::Rc::new(move |_, cx| {
                let _ = this.update(cx, |this, cx| {
                    open_node(&mut this.model, &go, open);
                    cx.notify();
                });
            }),
        }
    }

    /// `el`, a click on which opens the node `key`, and only that: not what `el` is inside.
    pub(super) fn goes_to(
        &self,
        el: Stateful<Div>,
        key: &str,
        open: OpenIn,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let go = key.to_owned();
        el.on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            open_node(&mut this.model, &go, open);
            cx.notify();
        }))
    }

    /// A sub-task's parent, named quietly in its row: a click goes to the parent, not the row.
    pub(super) fn parent_chip(&self, key: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let selector = format!("parent-{key}");
        let chip = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded_md()
            .text_xs()
            .text_color(color(TEXT_4))
            .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT_2)))
            .child(icon(Icon::SubTask, 12., TEXT_4))
            .child(key.to_owned());
        self.goes_to(chip, key, OpenIn::View, cx)
    }
}

/// A row in a list of links (a node, a doc, a session): one frame, so every link reads alike.
/// What leads it (a state mark or an icon), its title, a quiet aside and an end go inside.
pub(super) fn link_row(selector: String) -> Stateful<Div> {
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector.clone())
        .h(px(28.))
        .pl_2()
        .pr_1()
        .flex()
        .items_center()
        .gap_2()
        .rounded_lg()
        .bg(color(CONTENT))
        .border_1()
        .border_color(color(BORDER))
}

/// A link row's title: it takes the room left, cut with an ellipsis.
pub(super) fn link_title(title: impl Into<SharedString>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_color(color(TEXT_2))
        .child(title.into())
}

/// A link row's quiet aside after its title (a doc's path, who worked in a session and when).
pub(super) fn link_aside(text: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .max_w(px(160.))
        .truncate()
        .text_xs()
        .text_color(color(TEXT_4))
        .child(text.into())
}

/// The × at the end of a link row: it removes the link, and only that, never opening the node.
pub(super) fn unlink_button(
    selector: String,
    on: impl Fn(&mut LivtkApp, &mut Context<LivtkApp>) + 'static,
    cx: &mut Context<LivtkApp>,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector.clone())
        .group("unlink")
        .size(px(20.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .hover(|h| h.bg(color(HOVER)))
        .child(icon_hover(Icon::Unlink, 14., TEXT_4, "unlink", TEXT))
        .tooltip(|window, cx| {
            gpui_kit::component::tooltip::Tooltip::new("Remove the link").build(window, cx)
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            on(this, cx);
            cx.notify();
        }))
}
