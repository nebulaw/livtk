//! The app's markdown, in one place: every source the app draws (a doc's sections, a task's
//! body block by block) goes through `markdown`, so they read alike and gain alike. Callouts
//! (`> [!NOTE]`) and mermaid diagrams draw as blocks of their own; a link is followed by the
//! app; a code block has a copy button; headings take their sizes from the text style tokens.
//! What a source may hold beyond markdown (a reference to a task, a project, someone) is added
//! here, once, for every place.

use gpui_component::text::{MarkdownNode, TextView, TextViewStyle, markdown_ast};

use super::super::icons::{Icon, icon, icon_hover};
use super::super::theme::*;
use super::super::{FollowLink, ToggleTaskBox};
use super::style::heading_size;
use crate::model::docs::{CalloutKind, callout};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use livtk_core::md::ListItem;

/// `view` as the app draws markdown, diagrams at most `width` across.
pub(in crate::ui) fn markdown(view: TextView, width: f32) -> TextView {
    view.markdown_block_parser(|node, cx| match node {
        markdown_ast::Node::Blockquote(_) => {
            let (kind, title, body) = callout(cx.node_source(node)?)?;
            Some(MarkdownNode::new("callout", (kind, title, body)))
        }
        markdown_ast::Node::Code(code) if code.lang.as_deref() == Some("mermaid") => {
            Some(MarkdownNode::new("mermaid", code.value.clone()))
        }
        _ => None,
    })
    .markdown_block_renderer("callout", |node, _, _| {
        node.data::<(CalloutKind, Option<String>, String)>()
            .map(|(kind, title, body)| callout_box(*kind, title.as_deref(), body))
            .unwrap_or_else(|| div().into_any_element())
    })
    .markdown_block_renderer("mermaid", move |node, _, _| {
        node.data::<String>()
            .map(|source| super::super::diagram::mermaid(source, width))
            .unwrap_or_else(|| div().into_any_element())
    })
    .on_link_click(|href, _, window, cx| {
        window.dispatch_action(
            Box::new(FollowLink {
                href: href.to_string(),
            }),
            cx,
        );
    })
    .code_block_actions(|block, _, _| copy_button(block.code()))
}

/// `view` with its lists that hold task boxes drawn by hand, each box a button that ticks it:
/// `base` is where the view's source starts in the source the boxes are ticked in.
/// Only a list at the top, not one in a quote or nested in an item (those keep the renderer's
/// own boxes). `context`, the link definitions the items' links may name; `width`, how wide a
/// diagram in one may draw.
pub(in crate::ui) fn task_lists(
    view: TextView,
    base: usize,
    context: String,
    width: f32,
) -> TextView {
    view.markdown_block_parser(move |node, cx| match node {
        markdown_ast::Node::List(_) if node.position()?.start.column == 1 => {
            let text = cx.node_source(node)?;
            let start = cx.offset() + node.position()?.start.offset;
            let items = livtk_core::md::list_items(text);
            items.iter().any(|i| i.task.is_some()).then(|| {
                // Each box's place in the source ticked in, not in this list's.
                let items: Vec<ListItem> = items
                    .into_iter()
                    .map(|mut i| {
                        if let Some(t) = i.task.as_mut() {
                            t.mark += base + start;
                        }
                        i
                    })
                    .collect();
                MarkdownNode::new("task-list", items)
            })
        }
        _ => None,
    })
    .markdown_block_renderer("task-list", move |node, _, _| {
        node.data::<Vec<ListItem>>()
            .map(|items| task_list(items, &context, width))
            .unwrap_or_else(|| div().into_any_element())
    })
}

/// A list with task boxes: each item its box (or its marker), then its content as the app's
/// markdown, its links followed.
fn task_list(items: &[ListItem], context: &str, width: f32) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .children(items.iter().map(|item| {
            let checked = item.task.is_some_and(|t| t.checked);
            let lead = match item.task {
                Some(t) => task_box(t.mark, t.checked),
                None => div()
                    .flex_none()
                    .w(px(16.))
                    .text_color(color(TEXT_4))
                    .child(if item.marker.ends_with(['.', ')']) {
                        item.marker.clone()
                    } else {
                        "•".to_owned()
                    })
                    .into_any_element(),
            };
            div().flex().items_start().gap_2().child(lead).child(
                div()
                    .flex_1()
                    .min_w_0()
                    .when(checked, |d| d.text_color(color(TEXT_4)))
                    .child(markdown(
                        TextView::markdown(
                            SharedString::from(format!("task-item-{:x}", hash_of(&item.content))),
                            if context.is_empty() {
                                item.content.clone()
                            } else {
                                format!("{}\n\n{context}", item.content)
                            },
                        ),
                        width,
                    )),
            )
        }))
        .into_any_element()
}

/// A task's box at `at` in its source: a click ticks or unticks it.
fn task_box(at: usize, checked: bool) -> AnyElement {
    let id = format!("task-box-{at}");
    div()
        .flex_none()
        .h(px(22.))
        .flex()
        .items_center()
        .child(
            div()
                .id(SharedString::from(id.clone()))
                .debug_selector(move || id.clone())
                .size(px(15.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border(px(1.5))
                .border_color(color(if checked { ACCENT } else { TEXT_4 }))
                .when(checked, |d| {
                    d.bg(color(ACCENT)).child(icon(Icon::Check, 11., TEXT))
                })
                .cursor_pointer()
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    window.dispatch_action(Box::new(ToggleTaskBox { at }), cx);
                }),
        )
        .into_any_element()
}

/// The style a body's blocks are drawn in: headings at the text style tokens' sizes, so a
/// heading reads and edits at one size.
pub(in crate::ui) fn body_style() -> TextViewStyle {
    TextViewStyle::default().heading_font_size(|level, _| heading_size(level))
}

/// A code block's copy button, in its corner.
fn copy_button(code: SharedString) -> AnyElement {
    div()
        .id(SharedString::from(format!(
            "copy-code-{:x}",
            hash_of(&code)
        )))
        .group("copy-code")
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .hover(|h| h.bg(color(HOVER)))
        .child(icon_hover(Icon::Copy, 14., TEXT_4, "copy-code", TEXT))
        .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Copy").build(window, cx))
        .on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
        })
        .into_any_element()
}

/// A callout, as GitHub draws them: a bar and an icon in its kind's colour, its title (its
/// kind's name, unless it gives one), then its body as markdown.
fn callout_box(kind: CalloutKind, title: Option<&str>, body: &str) -> AnyElement {
    let (hue, glyph) = match kind {
        CalloutKind::Note => (ACCENT, Icon::Info),
        CalloutKind::Tip => (REVIEW, Icon::Claude),
        CalloutKind::Important => (0xb46ce0, Icon::Message),
        CalloutKind::Warning => (PROGRESS, Icon::Error),
        CalloutKind::Caution => (RED, Icon::Bug),
    };
    div()
        .debug_selector(move || format!("callout-{}", kind.name().to_lowercase()))
        .w_full()
        .pl_4()
        .pr_3()
        .py_3()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .border_l_2()
        .border_color(color(hue))
        .bg(with_alpha(hue, 0.06))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color(hue))
                .child(icon(glyph, 14., hue))
                .child(title.unwrap_or(kind.name()).to_owned()),
        )
        .when(!body.is_empty(), |d| {
            d.child(TextView::markdown(
                SharedString::from(format!("callout-{:x}", hash_of(body))),
                body.to_owned(),
            ))
        })
        .into_any_element()
}

/// A short hash of `s`, to tell code blocks apart.
fn hash_of(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}
