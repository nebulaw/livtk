//! A piece of text that reads as itself and is edited in place: the same style, the same room
//! around it, no background of its own while edited, so only the caret says it is. Configured by
//! its role (its look, from `TextRole`), what opens it, one line or several, whether it grows
//! with its text or scrolls, and its selectors; a title, a body, and later a
//! block of a body or a doc's line, all edit through it.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use super::super::*;
use super::style::TextRole;

/// One line (Enter saves; a long one still wraps, as it reads) or several (Enter starts a new
/// line, ⌘Enter saves).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Lines {
    One,
    Many,
}

/// How tall the editor is: as tall as its text, as it reads, or at most `rows`, scrolling past.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Fit {
    Grow,
    #[allow(
        dead_code,
        reason = "for a long field in a small place; nothing scrolls yet"
    )]
    Scroll {
        rows: usize,
    },
}

/// Rows enough that a growing editor never scrolls.
const GROW_ROWS: usize = 100_000;

/// How tall each piece of text last read, by its reading selector: its editor is at least as
/// tall on its first frame, before the field has laid out its rows, so nothing under it moves
/// and a scrolled panel keeps its place.
#[derive(Clone, Default)]
pub(in crate::ui) struct ReadHeights(Rc<RefCell<Reads>>);

#[derive(Default)]
struct Reads {
    heights: HashMap<SharedString, Pixels>,
}

impl ReadHeights {
    fn get(&self, id: &SharedString) -> Option<Pixels> {
        self.0.borrow().heights.get(id).copied()
    }
}

/// Which way the caret leaves a piece of text past its first or last line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Edge {
    Above,
    Below,
}

/// A piece of text, read or edited in place.
#[derive(Clone, Debug)]
pub(in crate::ui) struct InlineEdit {
    role: TextRole,
    lines: Lines,
    fit: Fit,
    /// Up on its first line, Down on its last, leave it (to the text above or below, as
    /// `LivtkApp::leave_edit` says) instead of going nowhere.
    walks: bool,
    /// Room below it that is its own: a click there opens it (the gap to what follows).
    below: Pixels,
    placeholder: &'static str,
    /// Its selectors reading, and editing.
    read_id: SharedString,
    edit_id: SharedString,
}

impl InlineEdit {
    /// One line, growing with its text. A double-click opens it, the word under it picked.
    pub(in crate::ui) fn new(
        role: TextRole,
        read_id: impl Into<SharedString>,
        edit_id: impl Into<SharedString>,
    ) -> Self {
        InlineEdit {
            role,
            lines: Lines::One,
            fit: Fit::Grow,
            walks: false,
            below: px(0.),
            placeholder: "",
            read_id: read_id.into(),
            edit_id: edit_id.into(),
        }
    }

    pub(in crate::ui) const fn below(mut self, below: Pixels) -> Self {
        self.below = below;
        self
    }

    pub(in crate::ui) const fn walks(mut self, walks: bool) -> Self {
        self.walks = walks;
        self
    }

    pub(in crate::ui) const fn lines(mut self, lines: Lines) -> Self {
        self.lines = lines;
        self
    }

    #[allow(dead_code, reason = "every field grows for now")]
    pub(in crate::ui) const fn fit(mut self, fit: Fit) -> Self {
        self.fit = fit;
        self
    }

    pub(in crate::ui) const fn placeholder(mut self, placeholder: &'static str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// The field that edits `value`, made to its settings, `picked` (a byte range in `value`)
    /// selected, or the caret at the end when nothing is.
    pub(in crate::ui) fn field(
        &self,
        value: String,
        picked: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TextareaState> {
        let end = value.len();
        let picked = picked.map_or(end..end, |r| r.start.min(end)..r.end.min(end));
        let rows = match self.fit {
            Fit::Grow => GROW_ROWS,
            Fit::Scroll { rows } => rows,
        };
        cx.new(|cx| {
            let mut field = TextareaState::new(window, cx)
                .placeholder(self.placeholder)
                .auto_grow(1, rows)
                .submit_on_enter(self.lines == Lines::One)
                .default_value(value);
            field.set_selected_range(picked, cx);
            field
        })
    }

    /// Whether an Enter (`secondary`: with ⌘ or Ctrl) saves.
    pub(in crate::ui) const fn saves_on_enter(&self, secondary: bool) -> bool {
        match self.lines {
            Lines::One => true,
            Lines::Many => secondary,
        }
    }

    /// What the field holds, to save: one line keeps no line breaks (a pasted one becomes a
    /// space); several keep every byte but the line breaks after the last line.
    pub(in crate::ui) fn value(&self, field: &Entity<TextareaState>, cx: &App) -> String {
        let text = field.read(cx).value();
        match self.lines {
            Lines::One => text.split_whitespace().collect::<Vec<_>>().join(" "),
            Lines::Many => text.trim_end_matches(['\n', '\r']).to_owned(),
        }
    }

    /// Reading plain `text`: as `reading`, `open` given the word the double-click fell on.
    pub(in crate::ui) fn reading_text(
        self,
        text: impl Into<SharedString>,
        open: impl Fn(&mut LivtkApp, Option<Range<usize>>, &mut Window, &mut Context<LivtkApp>)
        + 'static,
        heights: &ReadHeights,
        cx: &mut Context<LivtkApp>,
    ) -> AnyElement {
        let text: SharedString = text.into();
        let styled = StyledText::new(text.clone());
        let layout = styled.layout().clone();
        self.read_frame(styled, heights, cx, move |this, at, window, cx| {
            // Past a line's end, the nearest place in it.
            let index = layout
                .index_for_position(at)
                .unwrap_or_else(|nearest| nearest);
            open(this, Some(word_at(&text, index)), window, cx);
        })
    }

    /// Reading `content`, drawn by something else (rich text): in its style; `locate` says
    /// which bytes of its source the double-click picked, if it knows, to pick them editing.
    pub(in crate::ui) fn reading(
        self,
        content: impl IntoElement,
        locate: impl Fn(&App) -> Option<Range<usize>> + 'static,
        open: impl Fn(&mut LivtkApp, Option<Range<usize>>, &mut Window, &mut Context<LivtkApp>)
        + 'static,
        heights: &ReadHeights,
        cx: &mut Context<LivtkApp>,
    ) -> AnyElement {
        // The renderer picks the word under a double-click as the button goes down, after this
        // hears it: the editor opens once the press is handled, with that word picked.
        let (locate, open) = (Rc::new(locate), Rc::new(open));
        self.read_frame(content, heights, cx, move |_, _, window, cx| {
            let (locate, open) = (locate.clone(), open.clone());
            cx.defer_in(window, move |this, window, cx| {
                let picked = locate(cx);
                open(this, picked, window, cx);
            });
        })
    }

    fn read_frame(
        self,
        content: impl IntoElement,
        heights: &ReadHeights,
        cx: &mut Context<LivtkApp>,
        open: impl Fn(&mut LivtkApp, Point<Pixels>, &mut Window, &mut Context<LivtkApp>) + 'static,
    ) -> AnyElement {
        let id = self.read_id.clone();
        let (selector, key) = (id.clone(), id.clone());
        let store = heights.0.clone();
        self.role
            .style()
            .reading()
            .frame(div(), cx)
            .pb(self.role.style().reading().pad_y + self.below)
            .id(id)
            .debug_selector(move || selector.to_string())
            .relative()
            .child(content)
            .child(
                canvas(
                    move |bounds, _, _| {
                        store
                            .borrow_mut()
                            .heights
                            .insert(key.clone(), bounds.size.height);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            // On the second press, as an editor picks a word: not waiting for the release. Heard
            // on the way in, so the text's own handling of a press can't keep it from this.
            .capture_any_mouse_down(cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                if e.button == MouseButton::Left && e.click_count >= 2 {
                    open(this, e.position, window, cx);
                }
            }))
            .into_any_element()
    }

    /// Editing: the field in the role's style, with no frame, background or room of its own, so
    /// its text sits where the read text did; the textarea's own room across is taken off the
    /// frame's. Saving and leaving are the field's events; Esc cancels.
    pub(in crate::ui) fn editing(
        self,
        field: &Entity<TextareaState>,
        heights: &ReadHeights,
        cx: &mut Context<LivtkApp>,
    ) -> AnyElement {
        let style = self.role.style();
        let id = self.edit_id.clone();
        style
            .frame(div(), cx)
            .pb(style.pad_y + self.below)
            .debug_selector(move || id.to_string())
            .when_some(heights.get(&self.read_id), |d, h| d.min_h(h))
            // A click anywhere else saves: not only on something that takes the keys.
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.save_edit(false, window, cx);
            }))
            .px(style.pad_x - TEXTAREA_PX)
            .key_context(FORM)
            .on_action(cx.listener(|this, _: &Submit, window, cx| {
                this.save_edit(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.cancel_edit(window, cx)))
            .on_action(
                cx.listener(|this, _: &input::Escape, window, cx| this.cancel_edit(window, cx)),
            )
            // ⌘Enter saves before the field sees it: the field would add a line break first.
            .capture_action(cx.listener(|this, e: &input::Enter, window, cx| {
                if e.secondary {
                    cx.stop_propagation();
                    this.save_edit(true, window, cx);
                }
            }))
            .when(self.walks, |d| {
                let (up, down) = (field.clone(), field.clone());
                // Taken before the field moves the caret: past its edge, the walk leaves it.
                d.capture_action(cx.listener(move |this, _: &input::MoveUp, window, cx| {
                    if at_edge(&up, Edge::Above, cx) {
                        cx.stop_propagation();
                        this.leave_edit(Edge::Above, window, cx);
                    }
                }))
                .capture_action(cx.listener(
                    move |this, _: &input::MoveDown, window, cx| {
                        if at_edge(&down, Edge::Below, cx) {
                            cx.stop_propagation();
                            this.leave_edit(Edge::Below, window, cx);
                        }
                    },
                ))
            })
            .map(|d| {
                // Wider by the caret's room on the right, so it wraps where the text read.
                let text = div().mr(-FIELD_CARET_ROOM).child(
                    // Extra small: `TEXTAREA_PX` across, nothing down, so only the frame's room
                    // counts.
                    Textarea::new(field)
                        .appearance(false)
                        .xsmall()
                        .text_size(style.size)
                        .line_height(style.line)
                        .font_weight(style.weight)
                        .text_color(color(style.color)),
                );
                // Several lines take Tab to indent; one line leaves it to move on.
                d.child(match self.lines {
                    Lines::One => text,
                    Lines::Many => indented(text, field),
                })
            })
            .into_any_element()
    }
}

/// Whether the caret in `field` is on its first line (`Above`) or its last (`Below`), as laid
/// out, wrapped lines counted: one more Up or Down would leave it.
fn at_edge(field: &Entity<TextareaState>, edge: Edge, cx: &App) -> bool {
    let field = field.read(cx);
    let (Some((caret, line)), Some(text)) = (field.cursor_layout(), field.text_bounds()) else {
        return true;
    };
    match edge {
        Edge::Above => caret.top() < text.top() + line,
        Edge::Below => caret.bottom() > text.bottom() - line,
    }
}

/// The word around byte `index` of `text` (letters, digits and `_`, `-`, `'`), or the empty
/// range at it between words.
fn word_at(text: &str, index: usize) -> Range<usize> {
    let index = index.min(text.len());
    let is_word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '\'');
    let start = text[..index]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_word(c))
        .last()
        .map_or(index, |(i, _)| i);
    let end = text[index..]
        .char_indices()
        .find(|&(_, c)| !is_word(c))
        .map_or(text.len(), |(i, _)| index + i);
    start..end
}
