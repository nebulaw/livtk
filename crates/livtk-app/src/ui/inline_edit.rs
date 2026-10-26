//! A piece of text that reads as itself and is edited in place: the same style, the same room
//! around it, no background of its own while edited, so only the caret says it is. Configured by
//! its role (its look, from `TextRole`), what opens it, one line or several, whether it grows
//! with its text or scrolls, and its selectors; a title, a body, and later a
//! block of a body or a doc's line, all edit through it.

use super::text_style::TextRole;
use super::*;

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

    /// The field that edits `value`, made to its settings.
    pub(super) fn field(
        self,
        value: String,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TextareaState> {
        let rows = match self.fit {
            Fit::Grow => GROW_ROWS,
            Fit::Scroll { rows } => rows,
        };
        cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(self.placeholder)
                .auto_grow(1, rows)
                .submit_on_enter(self.lines == Lines::One)
                .default_value(value)
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

    /// Reading: `content` in its style; `open` runs on the click or double-click that opens it,
    /// as `opens` says.
    pub(super) fn reading(
        self,
        content: impl IntoElement,
        open: impl Fn(&mut LivtkApp, &mut Window, &mut Context<LivtkApp>) + 'static,
        cx: &mut Context<LivtkApp>,
    ) -> AnyElement {
        let clicks = self.opens.clicks();
        let id = self.read_id;
        self.role
            .style()
            .frame(div())
            .id(id)
            .debug_selector(move || id.into())
            .child(content)
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                if e.click_count() >= clicks {
                    open(this, window, cx);
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
        cx: &mut Context<LivtkApp>,
    ) -> AnyElement {
        let style = self.role.style();
        let id = self.edit_id;
        style
            .frame(div())
            .debug_selector(move || id.into())
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
