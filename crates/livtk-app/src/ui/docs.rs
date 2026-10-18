//! The Docs view: the project's docs as a tree at the left, the doc open in the middle as a
//! reader, and its outline at the right. The reader: a header (where the doc is, its title, when
//! it changed, the tasks linked to it), then the doc a section at each heading, at a reading
//! size and width, then the docs before and after it. The outline follows the reading and jumps
//! to a heading; a link to `doc.md#heading` lands on it. ⌘F finds in the doc, ⌘[ and ⌘] go back
//! and forward. What it shows is `model::docs`'s.

use std::ops::Range;

use gpui_component::Sizable;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::text::{RangeHighlight, TextView, TextViewState, TextViewStyle};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::icons::{Icon, icon, icon_hover};
use super::theme::*;
use super::{FollowLink, LivtkApp, RevealInDoc, state_mark};
use crate::model::docs::{DocRow, find_in};

/// The tree's width, a row's height and its indent per level.
const TREE_W: f32 = 260.;
const TREE_ROW: f32 = 28.;
const INDENT: f32 = 14.;
/// The doc's measure (about seventy characters a line at its size), its text and line height.
const READING_W: f32 = 720.;
const READING_TEXT: f32 = 16.;
const READING_LINE: f32 = 1.65;
/// The outline at the right, shown when the reader has room for it beside the doc.
const OUTLINE_W: f32 = 220.;
/// A found match, and the one the find bar is on.
const FOUND: f32 = 0.28;
const FOUND_HERE: f32 = 0.6;

/// What the reader keeps between frames: its scrolling, each section's markdown (with the text
/// it holds), the doc it last laid out, and the find bar with its matches.
pub(super) struct Reader {
    pub scroll: ScrollHandle,
    sections: Vec<(String, Entity<TextViewState>)>,
    shown: Option<String>,
    pub find: Entity<InputState>,
    pub finding: bool,
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
            self.reader.scroll.scroll_to_top_of_item(i + 1);
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
                for (i, (_, state)) in self.reader.sections.iter().enumerate() {
                    page = page.child(section(i, state));
                }
                page = page.child(self.doc_footer(cx));
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
            .page
            .title
            .clone()
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
        measure()
            .debug_selector(|| "doc-header".into())
            .pt(px(40.))
            .pb(px(20.))
            .flex()
            .flex_col()
            .gap_3()
            .child(crumbs)
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
                    .when_some(when, |d, w| d.child(w).child("·"))
                    .child(path.clone()),
            )
            .child(chips)
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.reader.scroll.scroll_to_top_of_item(i + 1);
                        cx.notify();
                    })),
            );
        }
        list
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

/// The doc's width, centred in the reader, with room at either side.
fn measure() -> Div {
    div().w_full().max_w(px(READING_W + 80.)).px(px(40.))
}

/// One section of the doc, in the reading style: links followed by the app, code blocks with a
/// copy button, a line asked to be seen scrolled to.
fn section(i: usize, state: &Entity<TextViewState>) -> Div {
    let style = TextViewStyle::default()
        .paragraph_gap(rems(1.))
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
        .text_size(px(READING_TEXT))
        .line_height(relative(READING_LINE))
        .text_color(color(TEXT_2))
        .child(
            TextView::new(state)
                .style(style)
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
