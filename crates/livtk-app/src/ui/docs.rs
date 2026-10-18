//! The Docs view: the project's docs as a tree at the left, the doc open in the middle as a
//! reader, and its outline at the right. The reader: a header (where the doc is, its title, when
//! it changed, the tasks linked to it), then the doc a section at each heading, at a reading
//! size and width, then the docs before and after it. The outline follows the reading and jumps
//! to a heading; a link to `doc.md#heading` lands on it. ⌘F finds in the doc, ⌘[ and ⌘] go back
//! and forward. What it shows is `model::docs`'s.

use std::ops::Range;
use std::time::Duration;

use gpui_component::Sizable;
use gpui_component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_component::text::{
    MarkdownNode, RangeHighlight, TextView, TextViewState, TextViewStyle, markdown_ast,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::icons::{Icon, icon, icon_hover};
use super::theme::*;
use super::{FollowLink, LivtkApp, RevealInDoc, ease_out_cubic, state_mark};
use crate::model::docs::{CalloutKind, DocRow, Saved, callout, find_in};

/// The tree's width, a row's height and its indent per level.
const TREE_W: f32 = 260.;
const TREE_ROW: f32 = 28.;
const INDENT: f32 = 14.;
/// The doc's measure: about seventy characters a line at its regular size (Settings sets the
/// size and the line's height).
const READING_W: f32 = 720.;
/// The tree's search field's height.
const SEARCH_H: f32 = 40.;
/// The outline at the right, shown when the reader has room for it beside the doc.
const OUTLINE_W: f32 = 220.;
/// A found match, and the one the find bar is on.
const FOUND: f32 = 0.28;
/// A doc's fade as it comes in, and the glide to a heading.
const DOC_FADE: Duration = Duration::from_millis(180);
const GLIDE: Duration = Duration::from_millis(260);
const FOUND_HERE: f32 = 0.6;

/// What the reader keeps between frames: its scrolling, each section's markdown (with the text
/// it holds), the doc it last laid out, and the find bar with its matches.
pub(super) struct Reader {
    pub scroll: ScrollHandle,
    sections: Vec<(String, Entity<TextViewState>)>,
    shown: Option<String>,
    pub find: Entity<InputState>,
    pub finding: bool,
    /// The tree's search, across the docs' titles and text.
    pub search: Entity<InputState>,
    /// The doc's markdown, while it is edited in the app; whether the file changed on disk
    /// since (a save then asks), and whether leaving would lose what was typed (Esc asks).
    pub editor: Option<Entity<EditorState>>,
    conflict: bool,
    leaving: bool,
    /// The glide to a heading under way, by number: a newer one stops it.
    glide: usize,
    /// Every match, by section and range in its rendered text; the one the bar is on.
    matches: Vec<(usize, Range<usize>)>,
    at: usize,
}

impl Reader {
    pub fn new(window: &mut Window, cx: &mut Context<LivtkApp>) -> Reader {
        Reader {
            scroll: ScrollHandle::new(),
            sections: Vec::new(),
            shown: None,
            find: cx.new(|cx| InputState::new(window, cx).placeholder("Find in the doc")),
            finding: false,
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search docs")),
            editor: None,
            conflict: false,
            leaving: false,
            glide: 0,
            matches: Vec::new(),
            at: 0,
        }
    }
}

impl LivtkApp {
    pub(super) fn docs_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let docs = &self.model.docs;
        if docs.tree.is_empty() {
            return super::empty(
                Icon::Docs,
                "No docs yet",
                &format!(
                    "Markdown files under {}/ show here, its folders as a tree.",
                    docs.dir
                ),
            )
            .child(super::empty_action(
                "empty-create-docs",
                "Start the docs",
                cx.listener(|this, _, _, cx| {
                    this.model.start_docs();
                    cx.notify();
                }),
            ))
            .into_any_element();
        }
        self.sync_sections(cx);
        div()
            .size_full()
            .flex()
            .child(self.docs_tree(cx))
            .child(self.reader(window, cx))
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
            // Room for the search field above, which floats there outside the list's keys.
            .pt(px(SEARCH_H + 8.))
            .flex()
            .flex_col()
            .gap(px(1.));
        if !docs.query.trim().is_empty() {
            return self.search_results(tree, cx);
        }
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
                            Icon::FolderOpen
                        } else {
                            Icon::Project
                        },
                        14.,
                        TEXT_4,
                    ))
                    .child(div().truncate().child(name))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.docs.index = i;
                        this.model.toggle_doc_folder(&path);
                        cx.notify();
                    })),
                DocRow::Doc { path, title, depth } => line
                    // A doc's name lines up with its folder's, past where the folder's icon is.
                    .pl(px(8. + depth as f32 * INDENT + 22.))
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

    /// What a search found, in place of the tree: each doc with the lines it is in. A click
    /// opens it with the find bar on the query.
    fn search_results(&self, mut list: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        let docs = &self.model.docs;
        if docs.results.is_empty() {
            return list.child(
                div()
                    .p_2()
                    .text_sm()
                    .text_color(color(TEXT_4))
                    .child("Nothing found"),
            );
        }
        for f in &docs.results {
            let path = f.path.clone();
            let id = format!("doc-found-{path}");
            list = list.child(
                div()
                    .id(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
                    .px_2()
                    .py(px(6.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .rounded_md()
                    .hover(|h| h.bg(color(HOVER)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(color(TEXT_2))
                            .truncate()
                            .child(f.title.clone()),
                    )
                    .children(f.lines.iter().take(2).map(|l| {
                        div()
                            .text_xs()
                            .text_color(color(TEXT_3))
                            .truncate()
                            .child(l.clone())
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(color(TEXT_4))
                            .truncate()
                            .child(f.path.clone()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_found(&path, window, cx);
                    })),
            );
        }
        list
    }

    /// A doc a search found, opened with the find bar on what was searched.
    pub(super) fn open_found(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.model.docs.query.clone();
        self.model.open_doc(path);
        self.reader
            .find
            .update(cx, |s, cx| s.set_value(query, window, cx));
        self.reader.finding = true;
        self.reader.find.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// The tree's search field, floating over the tree's top, outside the list's keys.
    pub(super) fn docs_search_field(&self) -> Div {
        div()
            .debug_selector(|| "docs-search".into())
            .absolute()
            .top_0()
            .left_0()
            .w(px(TREE_W))
            .h(px(SEARCH_H))
            .px_2()
            .pt_2()
            .child(
                div()
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(color(BORDER))
                    .bg(color(SURFACE))
                    .child(icon(Icon::Search, 13., TEXT_4))
                    .child(div().flex_1().min_w_0().child(super::flush_input(
                        Input::new(&self.reader.search).xsmall(),
                        24.,
                    ))),
            )
    }

    /// The search field changed: the docs searched again; Enter opens the first found.
    pub(super) fn on_docs_search(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let q = self.reader.search.read(cx).value().to_string();
                self.model.search_docs(&q);
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                if let Some(first) = self.model.docs.results.first().map(|f| f.path.clone()) {
                    self.open_found(&first, window, cx);
                }
            }
            _ => {}
        }
    }

    /// Each section's markdown, made or brought up to date for the open doc; a new doc starts
    /// at its top, or lands on the heading a link named once it is laid out.
    fn sync_sections(&mut self, cx: &mut Context<Self>) {
        let changed = self.reader.shown != self.model.docs.open;
        let texts: Vec<String> = self
            .model
            .docs
            .page
            .sections
            .iter()
            .map(|s| s.text.clone())
            .collect();
        let r = &mut self.reader;
        r.sections.truncate(texts.len());
        for (i, text) in texts.iter().enumerate() {
            match r.sections.get_mut(i) {
                Some((had, state)) if had != text => {
                    state.update(cx, |s, cx| s.set_text(text, cx));
                    *had = text.clone();
                }
                Some(_) => {}
                None => {
                    let state = cx.new(|cx| TextViewState::markdown(text, cx).selectable(true));
                    r.sections.push((text.clone(), state));
                }
            }
        }
        if changed {
            r.shown = self.model.docs.open.clone();
            r.scroll.set_offset(point(px(0.), px(0.)));
            r.matches.clear();
            // Once laid out: land on the heading a link named, find again in the new doc.
            let finding = r.finding;
            if self.model.docs.anchor.is_some() || finding {
                cx.spawn(async move |this, cx| {
                    let _ = this.update(cx, |this, cx| {
                        if finding {
                            this.find_matches(cx);
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            return;
        }
        if let Some(slug) = self.model.docs.anchor.take()
            && let Some(i) = self.model.docs.page.section_of(&slug)
        {
            self.glide_to(i + 1, cx);
        }
    }

    /// The reader, and its outline beside it when there is room.
    fn reader(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let docs = &self.model.docs;
        let room = self.content_width(window) - TREE_W;
        let headings = docs
            .page
            .sections
            .iter()
            .filter(|s| s.heading.is_some())
            .count();
        let outline =
            (room >= READING_W + OUTLINE_W + 96. && headings >= 2).then(|| self.outline(cx));
        let mut page = div()
            .id("doc-page")
            .debug_selector(|| "doc-page".into())
            .track_scroll(&self.reader.scroll)
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            // A column, so the doc is as tall as it is (a row would stretch it to the page's
            // height and leave nothing to scroll), centred across.
            .flex()
            .flex_col()
            .items_center()
            .pb(px(96.))
            .child(self.doc_header(cx));
        match &docs.text {
            Some(_) => {
                let reading = self.model.reading;
                for (i, (_, state)) in self.reader.sections.iter().enumerate() {
                    page = page.child(section(i, state, reading));
                }
                page = page
                    .children(self.mentioned_in(cx))
                    .child(self.doc_footer(cx));
            }
            None => {
                if let Some(path) = &docs.open {
                    page = page.child(
                        measure()
                            .text_color(color(TEXT_3))
                            .child(format!("{path} is gone from the project.")),
                    );
                }
            }
        }
        // A doc coming in fades up, briefly; with less motion asked for, it is simply there.
        let fade = if cx.reduce_motion() {
            Duration::ZERO
        } else {
            DOC_FADE
        };
        let page = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .child(page)
            .with_animation(
                SharedString::from(format!("doc-in-{}", docs.open.clone().unwrap_or_default())),
                Animation::new(fade).with_easing(ease_out_cubic),
                |el, t| el.opacity(0.35 + 0.65 * t),
            );
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .flex()
            .child(page)
            .children(outline)
    }

    /// Above the doc: where it is, its title, when it last changed, and the tasks linked to it.
    fn doc_header(&self, cx: &mut Context<Self>) -> Div {
        let docs = &self.model.docs;
        let Some(path) = docs.open.clone() else {
            return measure();
        };
        // Docs › Guide › Setup: the folders under the docs' own, by name.
        let under = path
            .strip_prefix(&format!("{}/", docs.dir))
            .unwrap_or(&path)
            .to_owned();
        let mut crumbs = div()
            .debug_selector(|| "doc-crumbs".into())
            .flex()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(color(TEXT_4))
            .child("Docs");
        let folders: Vec<&str> = under.split('/').collect();
        for folder in &folders[..folders.len().saturating_sub(1)] {
            crumbs = crumbs.child("›").child(capitalised(folder));
        }
        let title = docs
            .front
            .title
            .clone()
            .or_else(|| docs.page.title.clone())
            .or_else(|| docs.title_of(&path).map(str::to_owned))
            .unwrap_or_else(|| path.clone());
        let when = docs.modified.and_then(|t| {
            let t = jiff::Timestamp::try_from(t).ok()?;
            Some(format!("Edited {}", super::ago(&format!("{t:.3}"))))
        });
        let mut chips = div().flex().flex_wrap().items_center().gap_2();
        for n in &docs.linked {
            let key = n.key.clone();
            let (open_key, unlink_key) = (key.clone(), key.clone());
            let unlink_path = path.clone();
            let id = format!("doc-link-{key}");
            chips = chips.child(
                div()
                    .id(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
                    .group("doc-link")
                    .h(px(26.))
                    .max_w(px(280.))
                    .pl_2()
                    .pr_1()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded_full()
                    .border_1()
                    .border_color(color(BORDER))
                    .bg(color(SURFACE))
                    .text_xs()
                    .hover(|h| h.bg(color(HOVER)).border_color(color(BORDER_STRONG)))
                    .child(state_mark(n.kind, n.state))
                    .child(
                        div()
                            .flex_none()
                            .text_color(color(TEXT_3))
                            .child(key.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(color(TEXT_2))
                            .child(n.title.clone()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("doc-unlink-{key}")))
                            .debug_selector(move || format!("doc-unlink-{unlink_key}"))
                            .size(px(18.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .invisible()
                            .group_hover("doc-link", |s| s.visible())
                            .hover(|h| h.bg(color(SELECTED)))
                            .child(icon(Icon::Close, 11., TEXT_4))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.model.link_doc(&key, &unlink_path, false);
                                cx.notify();
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.show_node(&open_key);
                        cx.notify();
                    })),
            );
        }
        chips = chips.child(
            div()
                .id("doc-link-task")
                .debug_selector(|| "doc-link-task".into())
                .group("doc-link-task")
                .h(px(26.))
                .px_2()
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded_full()
                .text_xs()
                .text_color(color(TEXT_4))
                .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT_3)))
                .child(icon_hover(Icon::Link, 12., TEXT_4, "doc-link-task", TEXT_3))
                .child("Link a task")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.model.open_doc_node_picker();
                    this.reset_palette_input(window, cx);
                    cx.notify();
                })),
        );
        let file = path.clone();
        let actions = div()
            .flex()
            .items_center()
            .gap_1()
            .child(header_button(
                "doc-edit",
                Icon::Edit,
                "Edit",
                cx.listener(|this, _, window, cx| this.doc_start_edit(window, cx)),
            ))
            .child(header_button(
                "doc-open-editor",
                Icon::ArrowUpRight,
                "Open in editor",
                cx.listener(move |this, _, _, cx| this.open_in_editor(&file, cx)),
            ));
        measure()
            .debug_selector(|| "doc-header".into())
            .pt(px(40.))
            .pb(px(20.))
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(div().flex_1().child(crumbs))
                    .child(actions),
            )
            .child(
                div()
                    .debug_selector(|| "doc-title".into())
                    .text_size(px(30.))
                    .line_height(relative(1.2))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color(TEXT))
                    .child(title),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .when_some(docs.front.status.clone(), |d, status| {
                        d.child(
                            div()
                                .debug_selector(|| "doc-status".into())
                                .px_2()
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .rounded_full()
                                .bg(color(SELECTED))
                                .text_color(color(TEXT_2))
                                .child(capitalised(&status)),
                        )
                    })
                    .children(docs.front.tags.iter().map(|t| {
                        div()
                            .px_2()
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .border_1()
                            .border_color(color(BORDER))
                            .text_color(color(TEXT_3))
                            .child(format!("#{t}"))
                    }))
                    .when_some(when, |d, w| d.child(w).child("·"))
                    .child(path.clone()),
            )
            .child(chips)
    }

    /// Mentioned in: the docs that link to this one, and the tasks that mention it without
    /// being linked to it. A click opens one.
    fn mentioned_in(&self, cx: &mut Context<Self>) -> Option<Div> {
        let docs = &self.model.docs;
        if docs.backlinks.is_empty() && docs.mentions.is_empty() {
            return None;
        }
        let mut list = div().flex().flex_col().gap(px(2.));
        for d in &docs.backlinks {
            let path = d.path.clone();
            let id = format!("doc-backlink-{path}");
            list = list.child(
                div()
                    .id(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
                    .h(px(30.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .text_sm()
                    .hover(|h| h.bg(color(HOVER)))
                    .child(icon(Icon::Docs, 13., TEXT_4))
                    .child(
                        div()
                            .text_color(color(TEXT_2))
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
                            .child(d.path.clone()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.open_doc(&path);
                        cx.notify();
                    })),
            );
        }
        for n in &docs.mentions {
            let key = n.key.clone();
            let id = format!("doc-mention-{key}");
            list = list.child(
                div()
                    .id(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
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
                            .flex_none()
                            .text_color(color(TEXT_3))
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.show_node(&key);
                        cx.notify();
                    })),
            );
        }
        Some(
            measure()
                .debug_selector(|| "doc-mentioned".into())
                .mt(px(40.))
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color(TEXT_4))
                        .child("Mentioned in"),
                )
                .child(list),
        )
    }

    /// Under the doc: the docs before and after it, in the tree's order.
    fn doc_footer(&self, cx: &mut Context<Self>) -> Div {
        let (prev, next) = self.model.doc_neighbours();
        let pair = |d: Option<&livtk_core::docs::Doc>| d.map(|d| (d.path.clone(), d.title.clone()));
        let (prev, next) = (pair(prev), pair(next));
        let card = |id: &'static str, label: &'static str, doc: Option<(String, String)>| {
            let Some((path, title)) = doc else {
                return div().flex_1();
            };
            let right = id == "doc-next";
            div().flex_1().child(
                div()
                    .id(id)
                    .debug_selector(move || id.into())
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(right, |d| d.items_end())
                    .rounded_lg()
                    .border_1()
                    .border_color(color(BORDER))
                    .hover(|h| h.bg(color(HOVER)).border_color(color(BORDER_STRONG)))
                    .child(div().text_xs().text_color(color(TEXT_4)).child(label))
                    .child(
                        div()
                            .text_sm()
                            .text_color(color(TEXT_2))
                            .truncate()
                            .child(title),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.open_doc(&path);
                        cx.notify();
                    })),
            )
        };
        let before = card("doc-prev", "Previous", prev);
        let after = card("doc-next", "Next", next);
        measure()
            .debug_selector(|| "doc-footer".into())
            .mt(px(48.))
            .child(
                div()
                    .pt_6()
                    .border_t_1()
                    .border_color(color(BORDER))
                    .flex()
                    .gap_3()
                    .child(before)
                    .child(after),
            )
    }

    /// On this page: the doc's headings, the one being read lit; a click goes there.
    fn outline(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let page = &self.model.docs.page;
        // The section at the top of the reader (the header is the scroll's first child).
        let reading = self.reader.scroll.top_item().saturating_sub(1);
        let mut list = div()
            .id("doc-outline")
            .debug_selector(|| "doc-outline".into())
            .w(px(OUTLINE_W))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .pt(px(48.))
            .pr_6()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .pb_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(TEXT_4))
                    .child(icon(Icon::Outline, 12., TEXT_4))
                    .child("On this page"),
            );
        for (i, s) in page.sections.iter().enumerate() {
            let Some(h) = &s.heading else { continue };
            let lit = i == reading;
            let id = format!("doc-outline-{}", h.slug);
            list = list.child(
                div()
                    .id(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
                    .py(px(3.))
                    .pl(px(10. + f32::from(h.level.saturating_sub(2)) * 12.))
                    .border_l_2()
                    .border_color(color(if lit { ACCENT } else { BORDER }))
                    .text_size(px(13.))
                    .text_color(color(if lit { TEXT } else { TEXT_3 }))
                    .hover(|s| s.text_color(color(TEXT_2)))
                    .truncate()
                    .child(h.title.clone())
                    .on_click(cx.listener(move |this, _, _, cx| this.glide_to(i + 1, cx))),
            );
        }
        list
    }

    /// The reader glides to put its `item`th child at its top; with less motion asked for, it
    /// is there at once.
    fn glide_to(&mut self, item: usize, cx: &mut Context<Self>) {
        let scroll = self.reader.scroll.clone();
        let (Some(at), view) = (scroll.bounds_for_item(item), scroll.bounds()) else {
            scroll.scroll_to_top_of_item(item);
            cx.notify();
            return;
        };
        let from = scroll.offset();
        let to = (from.y - (at.top() - view.top()))
            .min(px(0.))
            .max(-scroll.max_offset().y);
        if cx.reduce_motion() {
            scroll.set_offset(point(from.x, to));
            cx.notify();
            return;
        }
        self.reader.glide += 1;
        let glide = self.reader.glide;
        cx.spawn(async move |this, cx| {
            let start = std::time::Instant::now();
            loop {
                let t = (start.elapsed().as_secs_f32() / GLIDE.as_secs_f32()).min(1.);
                let eased = ease_out_cubic(t);
                let alive = this.update(cx, |this, cx| {
                    // A newer glide takes over.
                    if this.reader.glide != glide {
                        return false;
                    }
                    this.reader
                        .scroll
                        .set_offset(point(from.x, from.y + (to - from.y) * eased));
                    cx.notify();
                    true
                });
                if !matches!(alive, Ok(true)) || t >= 1. {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
            }
        })
        .detach();
    }

    /// ⌘F: a bar over the reader's top right: the query, where in the matches it is, the way
    /// to the next and the one before.
    pub(super) fn find_bar(&self, cx: &mut Context<Self>) -> Div {
        let r = &self.reader;
        let count = if r.matches.is_empty() {
            if r.find.read(cx).value().trim().is_empty() {
                String::new()
            } else {
                "No matches".to_owned()
            }
        } else {
            format!("{} of {}", r.at + 1, r.matches.len())
        };
        let step = |id: &'static str, glyph: Icon, by: isize| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .hover(|h| h.bg(color(HOVER)))
                .child(icon(glyph, 12., TEXT_3))
                .on_click(cx.listener(move |this, _, _, cx| this.step_find(by, cx)))
        };
        let (up, down) = (
            step("doc-find-prev", Icon::ChevronUp, -1),
            step("doc-find-next", Icon::ChevronDown, 1),
        );
        div()
            .debug_selector(|| "doc-find".into())
            .absolute()
            .top(px(12.))
            .right(px(16.))
            .w(px(320.))
            .h(px(36.))
            .pl_3()
            .pr_1()
            .flex()
            .items_center()
            .gap_1()
            .rounded_lg()
            .bg(color(SURFACE))
            .border_1()
            .border_color(color(BORDER_STRONG))
            .shadow_lg()
            .occlude()
            .child(icon(Icon::Search, 13., TEXT_4))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(super::flush_input(Input::new(&r.find).xsmall(), 26.)),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(count),
            )
            .child(up)
            .child(down)
            .child(
                div()
                    .id("doc-find-close")
                    .debug_selector(|| "doc-find-close".into())
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .hover(|h| h.bg(color(HOVER)))
                    .child(icon(Icon::Close, 12., TEXT_3))
                    .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
            )
    }

    /// ⌘F in the Docs view: the find bar, its field given the keys.
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.view != super::View::Docs || self.model.docs.text.is_none() {
            return;
        }
        self.reader.finding = true;
        self.reader.find.read(cx).focus_handle(cx).focus(window, cx);
        self.find_matches(cx);
        cx.notify();
    }

    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reader.finding = false;
        self.reader.matches.clear();
        for (_, state) in &self.reader.sections {
            state.update(cx, |s, cx| s.clear_range_highlights(cx));
        }
        self.focus_main(window, cx);
        cx.notify();
    }

    /// The find field changed, or Enter in it: matches found again, or the next one.
    pub(super) fn on_find(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => self.find_matches(cx),
            InputEvent::PressEnter { shift, .. } => self.step_find(if *shift { -1 } else { 1 }, cx),
            _ => {}
        }
    }

    /// Every match of the query in the doc's rendered text, painted; the bar on the first.
    pub(super) fn find_matches(&mut self, cx: &mut Context<Self>) {
        let query = self.reader.find.read(cx).value().to_string();
        let mut all = Vec::new();
        for (i, (_, state)) in self.reader.sections.iter().enumerate() {
            let text = state.read(cx).rendered_text();
            for range in find_in(text.as_str(), &query) {
                all.push((i, range));
            }
        }
        self.reader.matches = all;
        self.reader.at = 0;
        self.paint_matches(cx);
        self.reveal_match(cx);
        cx.notify();
    }

    fn step_find(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.reader.matches.len();
        if n == 0 {
            return;
        }
        self.reader.at = (self.reader.at as isize + by).rem_euclid(n as isize) as usize;
        self.paint_matches(cx);
        self.reveal_match(cx);
        cx.notify();
    }

    fn paint_matches(&self, cx: &mut Context<Self>) {
        let r = &self.reader;
        for (i, (_, state)) in r.sections.iter().enumerate() {
            let highlights: Vec<RangeHighlight> = r
                .matches
                .iter()
                .enumerate()
                .filter(|(_, (s, _))| *s == i)
                .map(|(k, (_, range))| {
                    let alpha = if k == r.at { FOUND_HERE } else { FOUND };
                    RangeHighlight::new(range.clone(), with_alpha(PROGRESS, alpha))
                })
                .collect();
            state.update(cx, |s, cx| {
                let _ = s.set_range_highlights(highlights, cx);
            });
        }
    }

    /// The match the bar is on brought into view: its section, then its line.
    fn reveal_match(&self, cx: &mut Context<Self>) {
        let r = &self.reader;
        let Some((i, range)) = r.matches.get(r.at).cloned() else {
            return;
        };
        r.scroll.scroll_to_item(i + 1);
        if let Some((_, state)) = r.sections.get(i) {
            state.update(cx, |s, cx| {
                let _ = s.reveal_range(range, cx);
            });
        }
    }

    /// A section asked for a line of it to be seen (a match found): the reader scrolls to it.
    pub(super) fn reveal_in_doc(&mut self, top: f32, cx: &mut Context<Self>) {
        let scroll = &self.reader.scroll;
        let view = scroll.bounds();
        let (view_top, view_h) = (f32::from(view.top()), f32::from(view.size.height));
        if top < view_top + 48. || top > view_top + view_h - 48. {
            let offset = scroll.offset();
            let by = top - (view_top + view_h / 3.);
            scroll.set_offset(point(offset.x, offset.y - px(by)));
            cx.notify();
        }
    }
}

impl LivtkApp {
    /// Edit the open doc in the app: the reader gives way to its markdown.
    pub(super) fn doc_start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.model.edit_doc() else {
            return;
        };
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("markdown")
                .line_number(false)
                .soft_wrap(true)
        });
        editor.update(cx, |s, cx| s.set_value(text, window, cx));
        editor.read(cx).focus_handle(cx).focus(window, cx);
        self.reader.editor = Some(editor);
        self.reader.conflict = false;
        self.reader.leaving = false;
        cx.notify();
    }

    /// What is typed differs from what was last read or saved.
    fn edit_dirty(&self, cx: &App) -> bool {
        match (&self.reader.editor, &self.model.docs.editing) {
            (Some(editor), Some(edit)) => editor.read(cx).value().as_ref() != edit.base,
            _ => false,
        }
    }

    /// ⌘S: written, unless the file changed on disk meanwhile (then asked); `force`, written
    /// over whatever it holds.
    pub(super) fn doc_save(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(editor) = &self.reader.editor else {
            return;
        };
        let text = editor.read(cx).value().to_string();
        match self.model.save_doc(&text, force) {
            Saved::Written => {
                self.reader.conflict = false;
                self.reader.leaving = false;
            }
            Saved::Changed => self.reader.conflict = true,
            Saved::Failed(why) => self.model.warn(format!("The doc was not saved: {why}")),
        }
        cx.notify();
    }

    /// Esc: the reader again; with unsaved text, asked first (a second Esc keeps editing).
    pub(super) fn doc_leave_edit(
        &mut self,
        discard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.edit_dirty(cx) && !discard {
            self.reader.leaving = !self.reader.leaving;
            cx.notify();
            return;
        }
        self.reader.editor = None;
        self.reader.conflict = false;
        self.reader.leaving = false;
        self.model.stop_edit_doc();
        self.focus_main(window, cx);
        cx.notify();
    }

    /// The doc in the editor the system has for it: `$VISUAL`, else VS Code, else the one
    /// macOS opens text with.
    pub(super) fn open_in_editor(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(file) = self.model.doc_file(path) else {
            return;
        };
        let visual = std::env::var("VISUAL")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let run = |program: &str, args: &[&str]| {
            std::process::Command::new(program)
                .args(args)
                .arg(&file)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .is_ok()
        };
        let opened = match &visual {
            Some(v) => {
                let mut parts = v.split_whitespace();
                let program = parts.next().unwrap_or_default();
                let args: Vec<&str> = parts.filter(|a| *a != "-w" && *a != "--wait").collect();
                run(program, &args)
            }
            None => run("code", &[]) || run("open", &["-t"]) || run("xdg-open", &[]),
        };
        if !opened {
            self.model
                .warn("No editor would open the doc. Set VISUAL to the one you use.");
        }
        cx.notify();
    }

    /// The doc being edited, in the content's place (outside the list's keys, so typing is
    /// text): a bar with where it is, what to do, and the markdown.
    pub(super) fn doc_editor(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.reader.editor.as_ref()?;
        let path = self.model.docs.editing.as_ref()?.path.clone();
        let dirty = self.edit_dirty(cx);
        let bar = div()
            .debug_selector(|| "doc-editor-bar".into())
            .h(px(44.))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(color(BORDER))
            .text_sm()
            .child(icon(Icon::Edit, 14., TEXT_3))
            .child(div().text_color(color(TEXT_2)).child(path))
            .when(dirty, |d| {
                d.child(
                    div()
                        .debug_selector(|| "doc-unsaved".into())
                        .size(px(6.))
                        .rounded_full()
                        .bg(color(PROGRESS)),
                )
            })
            .child(div().flex_1())
            .child(bar_button(
                "doc-edit-done",
                "Done",
                "Esc",
                false,
                cx.listener(|this, _, window, cx| this.doc_leave_edit(false, window, cx)),
            ))
            .child(bar_button(
                "doc-edit-save",
                "Save",
                "⌘S",
                true,
                cx.listener(|this, _, _, cx| this.doc_save(false, cx)),
            ));
        let ask = |id: &'static str, text: &'static str, hue: u32| {
            div()
                .debug_selector(move || id.into())
                .h(px(40.))
                .flex_none()
                .px_4()
                .flex()
                .items_center()
                .gap_3()
                .bg(with_alpha(hue, 0.08))
                .border_b_1()
                .border_color(color(BORDER))
                .text_sm()
                .child(icon(Icon::Error, 14., hue))
                .child(div().flex_1().text_color(color(TEXT_2)).child(text))
        };
        let conflict = self.reader.conflict.then(|| {
            ask(
                "doc-conflict",
                "The doc changed on disk since you began. Save over it, or take what is there?",
                PROGRESS,
            )
            .child(bar_button(
                "doc-take-theirs",
                "Take theirs",
                "",
                false,
                cx.listener(|this, _, window, cx| {
                    this.reader.editor = None;
                    this.model.stop_edit_doc();
                    this.doc_start_edit(window, cx);
                }),
            ))
            .child(bar_button(
                "doc-save-over",
                "Save over it",
                "",
                true,
                cx.listener(|this, _, _, cx| this.doc_save(true, cx)),
            ))
        });
        let leaving = self.reader.leaving.then(|| {
            ask("doc-leaving", "What you typed is not saved.", PROGRESS)
                .child(bar_button(
                    "doc-discard",
                    "Discard",
                    "",
                    false,
                    cx.listener(|this, _, window, cx| this.doc_leave_edit(true, window, cx)),
                ))
                .child(bar_button(
                    "doc-save-leave",
                    "Save and close",
                    "",
                    true,
                    cx.listener(|this, _, window, cx| {
                        this.doc_save(false, cx);
                        if !this.reader.conflict {
                            this.doc_leave_edit(true, window, cx);
                        }
                    }),
                ))
        });
        Some(
            div()
                .debug_selector(|| "doc-editor".into())
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(bar)
                .children(conflict)
                .children(leaving)
                .child(
                    div().flex_1().min_h_0().flex().justify_center().child(
                        div()
                            .w_full()
                            .max_w(px(READING_W + 120.))
                            .h_full()
                            .px_6()
                            .py_4()
                            .text_size(px(14.))
                            .child(Editor::new(editor).appearance(false).h_full()),
                    ),
                )
                .into_any_element(),
        )
    }
}

/// A small button of the header's: an icon and a name, quiet.
fn header_button(
    id: &'static str,
    glyph: Icon,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .group(id)
        .h(px(26.))
        .px_2()
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded_md()
        .text_xs()
        .text_color(color(TEXT_4))
        .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT_2)))
        .child(icon_hover(glyph, 13., TEXT_4, id, TEXT_2))
        .child(label)
        .on_click(on_click)
}

/// A button of the editor's bar: a name and its keys; the main one in the accent.
fn bar_button(
    id: &'static str,
    label: &'static str,
    keys: &'static str,
    main: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .h(px(28.))
        .px_3()
        .flex()
        .items_center()
        .gap_2()
        .rounded_lg()
        .font_weight(FontWeight::MEDIUM)
        .when(main, |d| {
            d.bg(color(ACCENT))
                .text_color(color(TEXT))
                .hover(|h| h.bg(color(ACCENT_HOVER)))
        })
        .when(!main, |d| {
            d.border_1()
                .border_color(color(BORDER_STRONG))
                .text_color(color(TEXT_2))
                .hover(|h| h.bg(color(HOVER)))
        })
        .child(label)
        .when(!keys.is_empty(), |d| {
            d.child(
                div()
                    .text_xs()
                    .text_color(with_alpha(TEXT, 0.6))
                    .child(keys),
            )
        })
        .on_click(on_click)
}

/// The doc's width, centred in the reader, with room at either side.
fn measure() -> Div {
    div().w_full().max_w(px(READING_W + 80.)).px(px(40.))
}

/// One section of the doc, in the reading style: links followed by the app, code blocks with a
/// copy button, a line asked to be seen scrolled to.
fn section(i: usize, state: &Entity<TextViewState>, reading: crate::model::Reading) -> Div {
    let style = TextViewStyle::default()
        .paragraph_gap(rems(reading.gap()))
        .heading_font_size(|level, _| {
            px(match level {
                1 => 26.,
                2 => 21.,
                3 => 18.,
                _ => 16.,
            })
        });
    let id = format!("doc-section-{i}");
    measure()
        .debug_selector(move || id.clone())
        .pt_4()
        .text_size(px(reading.size.points()))
        .line_height(relative(reading.line()))
        .text_color(color(TEXT_2))
        .child(
            TextView::new(state)
                .style(style)
                // Callouts (`> [!NOTE]`) and mermaid diagrams: blocks of their own.
                .markdown_block_parser(|node, cx| match node {
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
                .markdown_block_renderer("mermaid", |node, _, _| {
                    node.data::<String>()
                        .map(|source| super::diagram::mermaid(source, READING_W))
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
                .on_reveal(|bounds, window, cx| {
                    window.dispatch_action(
                        Box::new(RevealInDoc {
                            top: f32::from(bounds.top()),
                        }),
                        cx,
                    );
                })
                .code_block_actions(|block, _, _| {
                    let code = block.code();
                    div()
                        .id(SharedString::from(format!(
                            "copy-code-{:x}",
                            hash_of(&code)
                        )))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .hover(|h| h.bg(color(HOVER)))
                        .child(icon(Icon::Copy, 13., TEXT_4))
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new("Copy").build(window, cx)
                        })
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                        })
                }),
        )
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
        .rounded_md()
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

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}
