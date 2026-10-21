//! The Docs view, in the app's grammar (docs/design.md): the docs as a list in the middle, a
//! folder's band over its docs; a doc opened to read takes the list's place behind a bar (back
//! and forward, where it is, Edit, Open in editor), its details in the panel (what it says of
//! itself, the tasks linked to it, what mentions it, On this page). The doc reads a section at
//! each heading, at a reading size and width, then the docs before and after it. On this page
//! follows the reading and jumps to a heading; a link to `doc.md#heading` lands on it. ⌘F finds
//! in the doc, ⌘[ and ⌘] go back and forward. What it shows is `model::docs`'s.

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
use super::node_ref::{OpenIn, unlink_button};
use super::theme::*;
use super::{FollowLink, LivtkApp, RevealInDoc, ease_out_cubic, state_mark};
use crate::model::docs::{CalloutKind, DocRow, Saved, callout, capitalised, find_in};

/// The marks of linked tasks a doc's row shows, before it counts the rest.
const MARKS: usize = 4;
/// How far each folder's lines sit in from it, and the slot for a folder's caret.
const INDENT: f32 = 20.;
const CARET_W: f32 = 12.;
/// The doc's measure: about seventy characters a line at its regular size (Settings sets the
/// size and the line's height).
const READING_W: f32 = 720.;
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
    /// The list's scrolling.
    pub list: ScrollHandle,
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
            list: ScrollHandle::new(),
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
                &format!("Markdown files under {}/ show here, by folder.", docs.dir),
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
        if docs.reading && docs.open.is_some() {
            self.sync_sections(cx);
            return self.reader(window, cx).into_any_element();
        }
        self.docs_list(cx).into_any_element()
    }

    /// The docs as a list, as the tasks are, nested by folder: a folder's line opens or shuts
    /// it, a doc's opens it to read. A search in the filter lists what it found instead.
    fn docs_list(&self, cx: &mut Context<Self>) -> Div {
        let docs = &self.model.docs;
        let rows = docs.rows();
        if rows.is_empty() {
            return div().size_full().child(super::empty(
                Icon::Search,
                "Nothing found",
                "No doc's title or text holds what the filter says.",
            ));
        }
        let mut list = div()
            .id("docs-list")
            .debug_selector(|| "docs-list".into())
            .track_scroll(&self.reader.list)
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for (i, row) in rows.into_iter().enumerate() {
            let highlighted = i == docs.index;
            list = list.child(match row {
                DocRow::Folder {
                    path,
                    name,
                    depth,
                    count,
                    open,
                } => self.doc_folder(i, path, name, depth, count, open, highlighted, cx),
                DocRow::Doc {
                    path,
                    title,
                    depth,
                    line,
                } => self.doc_line(i, path, title, depth, line, highlighted, cx),
            });
        }
        // Room at the end, under the dock.
        div()
            .size_full()
            .child(list.children((0..super::DOCK_ROWS).map(|_| super::dock_room())))
    }

    /// A line of the list at its depth: the list's row, indented, with a guide down from each
    /// folder it is in, and the slot for a folder's caret, so icons line up at a depth.
    fn nested_row(&self, id: String, path: &str, depth: usize, highlighted: bool) -> Stateful<Div> {
        let (own, moving) = self.highlight(path, highlighted);
        let guides = (0..depth).map(|k| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(16. + k as f32 * INDENT + CARET_W / 2.))
                .w(px(1.))
                .bg(color(BORDER))
        });
        div()
            .id(SharedString::from(id.clone()))
            .debug_selector(move || id.clone())
            .relative()
            .w_full()
            .h(px(ROW))
            .flex_none()
            .pr_4()
            .pl(px(16. + depth as f32 * INDENT))
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(color(CONTENT))
            .when(own, |d| d.bg(color(SELECTED)))
            .when(!highlighted, |d| d.hover(|s| s.bg(color(HOVER))))
            .children(moving.map(|(from, to, epoch)| super::glide_highlight(from, to, epoch)))
            .children(guides)
    }

    /// A folder's line: its caret, its icon, its name and how many docs are under it. A click
    /// opens or shuts it.
    #[allow(clippy::too_many_arguments)]
    fn doc_folder(
        &self,
        i: usize,
        path: String,
        name: String,
        depth: usize,
        count: usize,
        open: bool,
        highlighted: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.nested_row(format!("doc-folder-{path}"), &path, depth, highlighted)
            .child(
                div()
                    .w(px(CARET_W))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(icon(
                        if open {
                            Icon::CaretDown
                        } else {
                            Icon::CaretRight
                        },
                        10.,
                        TEXT_4,
                    )),
            )
            .child(icon(
                if open {
                    Icon::FolderOpen
                } else {
                    Icon::Project
                },
                15.,
                TEXT_3,
            ))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(TEXT))
                    .child(name),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(color(TEXT_4))
                    .child(format!("{count}")),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.docs.index = i;
                this.model.toggle_doc_folder(&path);
                cx.notify();
            }))
            .into_any_element()
    }

    /// A doc's line: its title (with, found by a search, its folder and the line it was found
    /// by), the states of the tasks linked to it, when it last changed.
    #[allow(clippy::too_many_arguments)]
    fn doc_line(
        &self,
        i: usize,
        path: String,
        title: String,
        depth: usize,
        line: Option<String>,
        highlighted: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let docs = &self.model.docs;
        let marks = docs.marks.get(&path).cloned().unwrap_or_default();
        let more = marks.len().saturating_sub(MARKS);
        let edited = docs.edited.get(&path).and_then(|t| {
            let t = jiff::Timestamp::try_from(*t).ok()?;
            Some(super::ago(&format!("{t:.3}")))
        });
        let searched = !docs.query.trim().is_empty();
        // Found by a search: the folder it is in, under the docs' own.
        let folder = searched
            .then(|| {
                let under = path
                    .strip_prefix(&format!("{}/", docs.dir))
                    .unwrap_or(&path);
                under.rsplit_once('/').map(|(f, _)| f.to_owned())
            })
            .flatten();
        self.nested_row(format!("doc-row-{path}"), &path, depth, highlighted)
            .child(div().w(px(CARET_W)).flex_none())
            .child(icon(Icon::Article, 15., TEXT_4))
            .child(
                div()
                    .flex_none()
                    .max_w(relative(0.5))
                    .truncate()
                    .text_color(color(TEXT))
                    .child(title),
            )
            .children(folder.map(|f| {
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(f)
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(color(TEXT_3))
                    .children(line),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .children(
                        marks
                            .iter()
                            .take(MARKS)
                            .map(|&(kind, state)| state_mark(kind, state)),
                    )
                    .when(more > 0, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(color(TEXT_4))
                                .child(format!("+{more}")),
                        )
                    }),
            )
            .child(
                div()
                    .w(px(60.))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .children(edited),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.model.docs.index = i;
                if searched {
                    this.open_found(&path, window, cx);
                } else {
                    this.model.open_doc(&path);
                    cx.notify();
                }
            }))
            .into_any_element()
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

    /// The doc being read, in the list's place: a bar (the way back, where it is, what to do
    /// with it), then the doc.
    fn reader(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let _ = window;
        let docs = &self.model.docs;
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
            .child(self.doc_header());
        match &docs.text {
            Some(_) => {
                let reading = self.model.reading;
                for (i, (_, state)) in self.reader.sections.iter().enumerate() {
                    page = page.child(section(i, state, reading));
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
        // A doc coming in fades up, briefly; with less motion asked for, it is simply there.
        let fade = if cx.reduce_motion() {
            Duration::ZERO
        } else {
            DOC_FADE
        };
        let page = div().flex_1().min_h_0().flex().child(page).with_animation(
            SharedString::from(format!("doc-in-{}", docs.open.clone().unwrap_or_default())),
            Animation::new(fade).with_easing(ease_out_cubic),
            |el, t| el.opacity(0.35 + 0.65 * t),
        );
        div()
            .debug_selector(|| "doc-reader".into())
            .size_full()
            .flex()
            .flex_col()
            .child(self.reader_bar(cx))
            .child(page)
    }

    /// The reader's bar: back and forward, where the doc is (Docs, a click away from the list),
    /// Edit and Open in editor, and the way back to the list.
    fn reader_bar(&self, cx: &mut Context<Self>) -> Div {
        let docs = &self.model.docs;
        let path = docs.open.clone().unwrap_or_default();
        let under = path
            .strip_prefix(&format!("{}/", docs.dir))
            .unwrap_or(&path)
            .to_owned();
        let title = self.doc_title();
        let mut crumbs = div()
            .debug_selector(|| "doc-crumbs".into())
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .text_color(color(TEXT_4))
            .child(
                div()
                    .id("doc-crumb-docs")
                    .debug_selector(|| "doc-crumb-docs".into())
                    .flex_none()
                    .px_1()
                    .rounded_lg()
                    .hover(|h| h.text_color(color(TEXT_2)).bg(color(HOVER)))
                    .child("Docs")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.leave_reading(window, cx);
                        cx.notify();
                    })),
            );
        let folders: Vec<&str> = under.split('/').collect();
        for folder in &folders[..folders.len().saturating_sub(1)] {
            crumbs = crumbs
                .child(icon(Icon::CaretRight, 8., TEXT_4))
                .child(div().flex_none().child(capitalised(folder)));
        }
        crumbs = crumbs.child(icon(Icon::CaretRight, 8., TEXT_4)).child(
            div()
                .min_w_0()
                .truncate()
                .text_color(color(TEXT_2))
                .child(title),
        );
        let file = path.clone();
        div()
            .debug_selector(|| "doc-bar".into())
            .h(px(46.))
            .flex_none()
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(color(BORDER))
            .child(bar_icon(
                "doc-back",
                Icon::ArrowLeft,
                "Back · ⌘[",
                !docs.back.is_empty(),
                cx.listener(|this, _, _, cx| {
                    this.model.doc_back();
                    cx.notify();
                }),
            ))
            .child(bar_icon(
                "doc-forward",
                Icon::ArrowRight,
                "Forward · ⌘]",
                !docs.forward.is_empty(),
                cx.listener(|this, _, _, cx| {
                    this.model.doc_forward();
                    cx.notify();
                }),
            ))
            .child(crumbs)
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
            ))
            .child(bar_icon(
                "doc-close",
                Icon::Close,
                "Back to the docs · Esc",
                true,
                cx.listener(|this, _, window, cx| {
                    this.leave_reading(window, cx);
                    cx.notify();
                }),
            ))
    }

    /// Done reading: the find bar shut with it, the list again.
    pub(super) fn leave_reading(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.reader.finding {
            self.close_find(window, cx);
        }
        self.model.leave_doc();
        cx.notify();
    }

    /// The open doc's title: its frontmatter's, its first heading, else its name.
    fn doc_title(&self) -> String {
        let docs = &self.model.docs;
        let path = docs.open.clone().unwrap_or_default();
        docs.front
            .title
            .clone()
            .or_else(|| docs.page.title.clone())
            .or_else(|| docs.title_of(&path).map(str::to_owned))
            .unwrap_or(path)
    }

    /// Above the doc, its title alone: the rest of what it is shows in the panel.
    fn doc_header(&self) -> Div {
        measure()
            .debug_selector(|| "doc-header".into())
            .pt(px(40.))
            .pb(px(12.))
            .child(
                div()
                    .debug_selector(|| "doc-title".into())
                    .text_size(px(30.))
                    .line_height(relative(1.2))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color(TEXT))
                    .child(self.doc_title()),
            )
    }

    /// The open doc's details in the panel, laid out as a task's: what it says of itself, the
    /// tasks linked to it, what mentions it, and On this page.
    pub(super) fn doc_panel(&self, width: f32, cx: &mut Context<Self>) -> Stateful<Div> {
        let docs = &self.model.docs;
        let path = docs.open.clone().unwrap_or_default();
        let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        let edited = docs.modified.and_then(|t| {
            let t = jiff::Timestamp::try_from(t).ok()?;
            Some(super::ago(&format!("{t:.3}")))
        });
        let mut props = div().flex().flex_col().gap_2();
        if let Some(status) = docs.front.status.clone() {
            props = props.child(super::prop_row(
                "Status",
                div().child(
                    div()
                        .debug_selector(|| "doc-status".into())
                        .px_2()
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .rounded_full()
                        .border_1()
                        .border_color(color(BORDER_STRONG))
                        .text_xs()
                        .child(capitalised(&status)),
                ),
            ));
        }
        if !docs.front.tags.is_empty() {
            props = props.child(super::prop_row(
                "Tags",
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .children(docs.front.tags.iter().map(|t| {
                        div()
                            .px_2()
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .border_1()
                            .border_color(color(BORDER_STRONG))
                            .text_xs()
                            .text_color(color(TEXT_3))
                            .child(format!("#{t}"))
                    })),
            ));
        }
        props = props
            .children(edited.map(|e| super::prop_row("Edited", div().child(e))))
            .child(super::prop_row(
                "Path",
                div().min_w_0().truncate().child(path.clone()),
            ));
        div()
            .id("doc-panel")
            .debug_selector(|| "doc-panel".into())
            .w(px(width))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(46.))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(color(BORDER))
                    .child(icon(Icon::Article, 15., TEXT_4))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(color(TEXT_3))
                            .child(name),
                    )
                    .child(div().flex_1())
                    .child(bar_icon(
                        "close-doc-panel",
                        Icon::DockRight,
                        "Close · ⌥⌘B",
                        true,
                        cx.listener(|this, _, _, cx| {
                            this.model.docs.hide_panel = true;
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div()
                    .id("doc-panel-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(props)
                    .child(self.doc_tasks(&path, cx))
                    .children(self.mentioned_in(cx))
                    .children(self.outline(cx)),
            )
    }

    /// The tasks linked to the doc, as the task panel lists its links: each a click away, an ×
    /// taking the link away; Link a task adds one.
    fn doc_tasks(&self, path: &str, cx: &mut Context<Self>) -> Div {
        let mut out = div()
            .debug_selector(|| "doc-tasks".into())
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(label("Tasks"))
                    .child(div().flex_1())
                    .child(header_button(
                        "doc-link-task",
                        Icon::Link,
                        "Link a task",
                        cx.listener(|this, _, window, cx| {
                            this.model.open_doc_node_picker();
                            this.reset_palette_input(window, cx);
                            cx.notify();
                        }),
                    )),
            );
        if self.model.docs.linked.is_empty() {
            return out.child(
                div().text_xs().text_color(color(TEXT_4)).child(
                    "None yet: a linked task's doc is put in front of Claude when it starts.",
                ),
            );
        }
        for n in &self.model.docs.linked {
            let (key, path) = (n.key.clone(), path.to_owned());
            let unlink = unlink_button(
                format!("doc-unlink-{key}"),
                move |this, _| this.model.link_doc(&key, &path, false),
                cx,
            );
            out = out.child(self.node_ref(
                format!("doc-link-{}", n.key),
                &n.key,
                n.kind,
                n.state,
                &n.title,
                OpenIn::Beside,
                Some(unlink),
                cx,
            ));
        }
        out
    }

    /// Mentioned in: the docs that link to this one, and the tasks that mention it without
    /// being linked to it. A click opens one.
    fn mentioned_in(&self, cx: &mut Context<Self>) -> Option<Div> {
        let docs = &self.model.docs;
        if docs.backlinks.is_empty() && docs.mentions.is_empty() {
            return None;
        }
        let mut list = div()
            .debug_selector(|| "doc-mentioned".into())
            .flex()
            .flex_col()
            .gap_1()
            .child(label("Mentioned in"));
        for d in &docs.backlinks {
            let path = d.path.clone();
            list = list.child(
                link_row(format!("doc-backlink-{path}"))
                    .child(icon(Icon::Article, 14., TEXT_4))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(color(TEXT_2))
                            .child(d.title.clone()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.open_doc(&path);
                        cx.notify();
                    })),
            );
        }
        for n in &docs.mentions {
            list = list.child(self.node_ref(
                format!("doc-mention-{}", n.key),
                &n.key,
                n.kind,
                n.state,
                &n.title,
                OpenIn::Beside,
                None,
                cx,
            ));
        }
        Some(list)
    }

    /// Under the doc: the docs before and after it, in the list's order.
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

    /// On this page: the doc's headings, the one being read lit; a click goes there. Only for a
    /// doc with headings to go between.
    fn outline(&self, cx: &mut Context<Self>) -> Option<Div> {
        let page = &self.model.docs.page;
        if page.sections.iter().filter(|s| s.heading.is_some()).count() < 2 {
            return None;
        }
        // The section at the top of the reader (the header is the scroll's first child).
        let reading = self.reader.scroll.top_item().saturating_sub(1);
        let mut list = div()
            .debug_selector(|| "doc-outline".into())
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(label("On this page"));
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
                    .text_color(color(if lit { TEXT } else { TEXT_3 }))
                    .hover(|s| s.text_color(color(TEXT_2)))
                    .truncate()
                    .child(h.title.clone())
                    .on_click(cx.listener(move |this, _, _, cx| this.glide_to(i + 1, cx))),
            );
        }
        Some(list)
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
        let step = |id: &'static str, glyph: Icon, tip: &'static str, by: isize| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .group(id)
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_lg()
                .hover(|h| h.bg(color(HOVER)))
                .child(icon_hover(glyph, 14., TEXT_4, id, TEXT))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| this.step_find(by, cx)))
        };
        let (up, down) = (
            step("doc-find-prev", Icon::ChevronUp, "Previous match · ⇧↵", -1),
            step("doc-find-next", Icon::ChevronDown, "Next match · ↵", 1),
        );
        div()
            .debug_selector(|| "doc-find".into())
            .absolute()
            // Under the reader's bar.
            .top(px(46. + 10.))
            .right(px(16.))
            .w(px(320.))
            .h(px(36.))
            .pl_3()
            .pr_1()
            .flex()
            .items_center()
            .gap_1()
            .rounded_xl()
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
                    .group("doc-find-close")
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_lg()
                    .hover(|h| h.bg(color(HOVER)))
                    .child(icon_hover(Icon::Close, 14., TEXT_4, "doc-find-close", TEXT))
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("Close · Esc").build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
            )
    }

    /// ⌘F in the Docs view: the find bar, its field given the keys.
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let docs = &self.model.docs;
        if self.model.view() != super::View::Docs || !docs.reading || docs.text.is_none() {
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
            .h(px(46.))
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
        .h(px(24.))
        .px_2()
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded_lg()
        .text_xs()
        .text_color(color(TEXT_3))
        .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
        .child(icon_hover(glyph, 14., TEXT_3, id, TEXT))
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
                        .group("copy-code")
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_lg()
                        .hover(|h| h.bg(color(HOVER)))
                        .child(icon_hover(Icon::Copy, 14., TEXT_4, "copy-code", TEXT))
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

/// A section's name, as the panel's are.
fn label(text: &'static str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color(TEXT_4))
        .child(text)
}

/// A row of the panel's lists, as the task panel's links are: a click opens what it names.
fn link_row(id: String) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || id.clone())
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
        .hover(|h| h.bg(color(HOVER)))
}

/// An icon button of a bar: brightening on hover, its name and keys in a tooltip; dim and
/// still when there is nowhere to go.
fn bar_icon(
    id: &'static str,
    glyph: Icon,
    tip: &'static str,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .group(id)
        .size(px(26.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx)
        })
        .when(enabled, |d| {
            d.hover(|h| h.bg(color(HOVER)))
                .child(icon_hover(glyph, 16., TEXT_4, id, TEXT_2))
                .on_click(on_click)
        })
        .when(!enabled, |d| d.child(icon(glyph, 16., BORDER_STRONG)))
}
