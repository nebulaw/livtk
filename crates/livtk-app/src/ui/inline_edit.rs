//! A piece of text that reads as itself and is edited in place: the same style, the same room
//! around it, no background of its own while edited, so only the caret says it is. Configured by
//! its role (its look, from `TextRole`), what opens it, and its selectors; a title, a body, and
//! later a block of a body or a doc's line, all edit through it.

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

/// A piece of text, read or edited in place.
#[derive(Clone, Copy, Debug)]
pub(super) struct InlineEdit {
    role: TextRole,
    opens: Opens,
    /// Its selectors reading, and editing.
    read_id: &'static str,
    edit_id: &'static str,
}

impl InlineEdit {
    pub(super) const fn new(role: TextRole, read_id: &'static str, edit_id: &'static str) -> Self {
        InlineEdit {
            role,
            opens: Opens::DoubleClick,
            read_id,
            edit_id,
        }
    }

    pub(super) const fn opens(mut self, opens: Opens) -> Self {
        self.opens = opens;
        self
    }

    /// Reading: `content` in its style, a hover saying it can be edited; `open` runs on the
    /// click or double-click that opens it, as `opens` says.
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
            .rounded_xl()
            .hover(|s| s.bg(color(HOVER)))
            .child(content)
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                if e.click_count() >= clicks {
                    open(this, window, cx);
                }
            }))
            .into_any_element()
    }

    /// Editing on one line: Enter or leaving saves, Esc cancels (the field's own events).
    pub(super) fn editing_line(
        self,
        state: &Entity<InputState>,
        cx: &mut Context<LivtkApp>,
    ) -> AnyElement {
        let style = self.role.style();
        let input = flush_input(
            Input::new(state)
                .line_height(style.line)
                .text_size(style.size)
                .font_weight(style.weight),
            f32::from(style.line),
        );
        self.frame_editing(cx).child(input).into_any_element()
    }

    /// Editing on several lines: ⌘Enter or leaving saves, Esc cancels. The textarea's own room
    /// across is taken off the frame's, so the text stays where it was.
    pub(super) fn editing_area(
        self,
        state: &Entity<TextareaState>,
        cx: &mut Context<LivtkApp>,
    ) -> AnyElement {
        let style = self.role.style();
        self.frame_editing(cx)
            .px(style.pad_x - TEXTAREA_PX)
            .key_context(FORM)
            .on_action(cx.listener(|this, _: &Submit, window, cx| this.save_edit(true, window, cx)))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.cancel_edit(window, cx)))
            .child(indented(
                div().child(
                    Textarea::new(state)
                        .appearance(false)
                        .xsmall()
                        .text_size(style.size),
                ),
                state,
            ))
            .into_any_element()
    }

    /// The editing frame: the reading one, without a background.
    fn frame_editing(self, cx: &mut Context<LivtkApp>) -> Div {
        let id = self.edit_id;
        self.role
            .style()
            .frame(div())
            .debug_selector(move || id.into())
            .rounded_xl()
            .on_action(
                cx.listener(|this, _: &input::Escape, window, cx| this.cancel_edit(window, cx)),
            )
    }
}
