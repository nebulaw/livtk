//! The Docs view: the project's docs as a tree at the left, the doc open at the right in the
//! app's markdown at a reading width, and under it the nodes linked to it. A link in a doc opens
//! another doc here, a node's key opens the node in the detail panel, a web link the browser.
//! What it shows is `model::docs`'s.

use gpui_component::text::TextView;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::icons::{Icon, icon, icon_hover};
use super::theme::*;
use super::{FollowLink, LivtkApp, state_mark};
use crate::model::docs::{DocRow, link_keys};

/// The tree's width, a row's height and its indent per level, and the doc's reading width.
const TREE_W: f32 = 260.;
const TREE_ROW: f32 = 28.;
const INDENT: f32 = 14.;
const READING_W: f32 = 760.;

impl LivtkApp {
    pub(super) fn docs_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let docs = &self.model.docs;
        if docs.tree.is_empty() {
            let dir = &docs.dir;
            return super::empty(
                Icon::Docs,
                "No docs yet",
                &format!("Markdown files under {dir}/ show here, its folders as a tree."),
            )
            .into_any_element();
        }
        div()
            .size_full()
            .flex()
            .child(self.docs_tree(cx))
            .child(self.doc_page(cx))
            .into_any_element()
    }

    /// The tree: folders open and shut, docs open on a click; the highlighted row follows the
    /// keys (J, K and Enter, as in the lists).
    fn docs_tree(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let docs = &self.model.docs;
        let mut tree = div()
            .id("docs-tree")
            .debug_selector(|| "docs-tree".into())
            .w(px(TREE_W))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(color(BORDER))
            .p_2()
            .flex()
            .flex_col()
            .gap(px(1.));
        for (i, row) in docs.rows().into_iter().enumerate() {
            let highlighted = i == docs.index;
            let open =
                matches!(&row, DocRow::Doc { path, .. } if docs.open.as_deref() == Some(path));
            let id = format!("doc-row-{}", row.path());
            let line = div()
                .id(SharedString::from(id.clone()))
                .debug_selector(move || id.clone())
                .group("doc-row")
                .h(px(TREE_ROW))
                .flex_none()
                .pr_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .text_sm()
                .when(highlighted || open, |d| d.bg(color(SELECTED)))
                .when(!highlighted && !open, |d| d.hover(|h| h.bg(color(HOVER))))
                .text_color(color(if open { TEXT } else { TEXT_3 }));
            let line = match row {
                DocRow::Folder {
                    path,
                    name,
                    depth,
                    open,
                } => line
                    .pl(px(8. + depth as f32 * INDENT))
                    .child(icon(
                        if open {
                            Icon::CaretDown
                        } else {
                            Icon::CaretRight
                        },
                        9.,
                        TEXT_4,
                    ))
                    .child(div().truncate().child(name))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.docs.index = i;
                        this.model.toggle_doc_folder(&path);
                        cx.notify();
                    })),
                DocRow::Doc { path, title, depth } => line
                    .pl(px(8. + depth as f32 * INDENT))
                    .child(icon_hover(Icon::Docs, 13., TEXT_4, "doc-row", TEXT_3))
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.docs.index = i;
                        this.model.open_doc(&path);
                        cx.notify();
                    })),
            };
            tree = tree.child(line);
        }
        tree
    }

    /// The open doc at a reading width, and the nodes linked to it.
    fn doc_page(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let docs = &self.model.docs;
        let prefix = docs.prefix.clone();
        let mut page = div()
            .w_full()
            .max_w(px(READING_W))
            .px_8()
            .py_6()
            .flex()
            .flex_col()
            .gap_6();
        match (&docs.open, &docs.text) {
            (Some(path), Some(text)) => {
                let path = path.clone();
                page = page
                    .child(
                        div()
                            .debug_selector(|| "doc-path".into())
                            .text_xs()
                            .text_color(color(TEXT_4))
                            .child(path.clone()),
                    )
                    .child(
                        div().debug_selector(|| "doc-text".into()).child(
                            TextView::markdown(
                                SharedString::from(format!("doc-{path}")),
                                link_keys(text, &prefix),
                            )
                            .selectable(true)
                            // A link is the app's to follow: another doc here, a node in the
                            // panel, a web page in the browser.
                            .on_link_click(|href, _, window, cx| {
                                window.dispatch_action(
                                    Box::new(FollowLink {
                                        href: href.to_string(),
                                    }),
                                    cx,
                                );
                            }),
                        ),
                    );
            }
            (Some(path), None) => {
                page = page.child(
                    div()
                        .text_color(color(TEXT_3))
                        .child(format!("{path} is gone from the project.")),
                );
            }
            _ => {}
        }
        if docs.open.is_some() {
            page = page.child(self.doc_links(cx));
        }
        div()
            .id("doc-page")
            .debug_selector(|| "doc-page".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .flex()
            .justify_center()
            .child(page)
    }

    /// Under the doc: the nodes linked to it (a click opens one in the panel, × takes the link
    /// away), and Link a task…
    fn doc_links(&self, cx: &mut Context<Self>) -> Div {
        let docs = &self.model.docs;
        let path = docs.open.clone().unwrap_or_default();
        let mut list = div()
            .debug_selector(|| "doc-links".into())
            .pt_4()
            .border_t_1()
            .border_color(color(BORDER))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .pb_1()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(TEXT_4))
                    .child("Linked"),
            );
        for n in &docs.linked {
            let key = n.key.clone();
            let (open_key, unlink_key, unlink_path) = (key.clone(), key.clone(), path.clone());
            let unlinked = key.clone();
            let id = format!("doc-link-{key}");
            list = list.child(
                div()
                    .id(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
                    .group("doc-link")
                    .h(px(30.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .text_sm()
                    .hover(|h| h.bg(color(HOVER)))
                    .child(state_mark(n.kind, n.state))
                    .child(
                        div()
                            .w(px(64.))
                            .flex_none()
                            .text_color(color(TEXT_4))
                            .child(key.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(color(TEXT_2))
                            .child(n.title.clone()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("doc-unlink-{key}")))
                            .debug_selector(move || format!("doc-unlink-{unlink_key}"))
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .invisible()
                            .group_hover("doc-link", |s| s.visible())
                            .hover(|h| h.bg(color(SELECTED)))
                            .child(icon(Icon::Close, 12., TEXT_4))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.model.link_doc(&unlinked, &unlink_path, false);
                                cx.notify();
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.show_node(&open_key);
                        cx.notify();
                    })),
            );
        }
        list.child(
            div()
                .id("doc-link-task")
                .debug_selector(|| "doc-link-task".into())
                .mt_1()
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .text_sm()
                .text_color(color(TEXT_4))
                .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT_3)))
                .child(icon(Icon::Link, 13., TEXT_4))
                .child("Link a task…")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.model.open_doc_node_picker();
                    this.reset_palette_input(window, cx);
                    cx.notify();
                })),
        )
    }
}
