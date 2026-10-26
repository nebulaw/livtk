//! A piece of text that reads as itself and is edited in place: the same style, the same room
//! around it, no background of its own while edited, so only the caret says it is. Configured by
//! its role (its look, from `TextRole`), what opens it, one line or several, whether it grows
//! with its text or scrolls, and its selectors; a title, a body, and later a
//! block of a body or a doc's line, all edit through it.

use super::text_style::TextRole;
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

/// What opens a piece of text for editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Opens {
    Click,
    DoubleClick,
}

impl Opens {
    const fn clicks(self) -> usize {
        match self {
            Opens::Click => 1,
            Opens::DoubleClick => 2,
        }
    }
}

/// One line (Enter saves; a long one still wraps, as it reads) or several (Enter starts a new
/// line, ⌘Enter saves).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lines {
    One,
    Many,
}

/// How tall the editor is: as tall as its text, as it reads, or at most `rows`, scrolling past.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fit {
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
pub(super) struct ReadHeights(Rc<RefCell<HashMap<&'static str, Pixels>>>);

impl ReadHeights {
    fn get(&self, id: &'static str) -> Option<Pixels> {
        self.0.borrow().get(id).copied()
    }
}

/// A piece of text, read or edited in place.
#[derive(Clone, Copy, Debug)]
pub(super) struct InlineEdit {
    role: TextRole,
    opens: Opens,
    lines: Lines,
    fit: Fit,
    placeholder: &'static str,
    /// Its selectors reading, and editing.
    read_id: &'static str,
    edit_id: &'static str,
}

impl InlineEdit {
    /// One line, opened by a click, growing with its text.
    pub(super) const fn new(role: TextRole, read_id: &'static str, edit_id: &'static str) -> Self {
        InlineEdit {
            role,
            opens: Opens::Click,
            lines: Lines::One,
            fit: Fit::Grow,
            placeholder: "",
            read_id,
            edit_id,
        }
    }

    pub(super) const fn opens(mut self, opens: Opens) -> Self {
        self.opens = opens;
        self
    }

    pub(super) const fn lines(mut self, lines: Lines) -> Self {
        self.lines = lines;
        self
    }

    #[allow(dead_code, reason = "every field grows for now")]
    pub(super) const fn fit(mut self, fit: Fit) -> Self {
        self.fit = fit;
        self
    }

    pub(super) const fn placeholder(mut self, placeholder: &'static str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// The field that edits `value`, made to its settings, its caret at `at` (a byte offset
    /// in `value`), or at the end when where is not known.
    pub(super) fn field(
        self,
        value: String,
        at: Option<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TextareaState> {
        let at = at.unwrap_or(value.len()).min(value.len());
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
            field.set_selected_range(at..at, cx);
            field
        })
    }

    /// Whether an Enter (`secondary`: with ⌘ or Ctrl) saves.
    pub(super) const fn saves_on_enter(self, secondary: bool) -> bool {
        match self.lines {
            Lines::One => true,
            Lines::Many => secondary,
        }
    }

    /// What the field holds, to save: one line keeps no line breaks (a pasted one becomes a
    /// space); trailing blank lines carry nothing.
    pub(super) fn value(self, field: &Entity<TextareaState>, cx: &App) -> String {
        let text = field.read(cx).value();
        match self.lines {
            Lines::One => text.split_whitespace().collect::<Vec<_>>().join(" "),
            Lines::Many => text.trim_end().to_owned(),
        }
    }

    /// Reading plain `text`: as `reading`, `open` told where in the text the click fell, so the
    /// caret starts there.
    pub(super) fn reading_text(
        self,
        text: impl Into<SharedString>,
        open: impl Fn(&mut LivtkApp, Option<usize>, &mut Window, &mut Context<LivtkApp>) + 'static,
        heights: &ReadHeights,
        cx: &mut Context<LivtkApp>,
    ) -> AnyElement {
        let text = StyledText::new(text.into());
        let layout = text.layout().clone();
        self.read_frame(text, heights, cx, move |this, at, window, cx| {
            // Past a line's end, the nearest place in it.
            let index = layout
                .index_for_position(at)
                .unwrap_or_else(|nearest| nearest);
            open(this, Some(index), window, cx);
        })
    }

    /// Reading `content` (rich, so where a click falls in its source is not known): in its
    /// style; `open` runs on the click or double-click that opens it, as `opens` says.
    pub(super) fn reading(
        self,
        content: impl IntoElement,
        open: impl Fn(&mut LivtkApp, Option<usize>, &mut Window, &mut Context<LivtkApp>) + 'static,
        heights: &ReadHeights,
        cx: &mut Context<LivtkApp>,
    ) -> AnyElement {
        self.read_frame(content, heights, cx, move |this, _, window, cx| {
            open(this, None, window, cx);
        })
    }

    fn read_frame(
        self,
        content: impl IntoElement,
        heights: &ReadHeights,
        cx: &mut Context<LivtkApp>,
        open: impl Fn(&mut LivtkApp, Point<Pixels>, &mut Window, &mut Context<LivtkApp>) + 'static,
    ) -> AnyElement {
        let clicks = self.opens.clicks();
        let id = self.read_id;
        let store = heights.0.clone();
        self.role
            .style()
            .frame(div())
            .id(id)
            .debug_selector(move || id.into())
            .relative()
            .child(content)
            .child(
                canvas(
                    move |bounds, _, _| {
                        store.borrow_mut().insert(id, bounds.size.height);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                if e.click_count() >= clicks {
                    open(this, e.position(), window, cx);
                }
            }))
            .into_any_element()
    }

    /// Editing: the field in the role's style, with no frame, background or room of its own, so
    /// its text sits where the read text did; the textarea's own room across is taken off the
    /// frame's. Saving and leaving are the field's events; Esc cancels.
    pub(super) fn editing(
        self,
        field: &Entity<TextareaState>,
        heights: &ReadHeights,
        cx: &mut Context<LivtkApp>,
    ) -> AnyElement {
        let style = self.role.style();
        let id = self.edit_id;
        style
            .frame(div())
            .debug_selector(move || id.into())
            .when_some(heights.get(self.read_id), |d, h| d.min_h(h))
            // A click anywhere else saves: not only on something that takes the keys.
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.save_edit(false, window, cx)))
            .px(style.pad_x - TEXTAREA_PX)
            .key_context(FORM)
            .on_action(cx.listener(|this, _: &Submit, window, cx| this.save_edit(true, window, cx)))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.cancel_edit(window, cx)))
            .on_action(
                cx.listener(|this, _: &input::Escape, window, cx| this.cancel_edit(window, cx)),
            )
            .map(|d| {
                let text = div().child(
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
