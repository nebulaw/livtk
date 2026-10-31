//! The Terminal view: a project's shell, drawn. Each project has one, started the first time
//! its view is laid out (so it starts at the size it is shown at) and kept running while the app
//! runs; leaving the view or the project leaves it as it was. The screen is Alacritty's
//! (`crate::terminal`); here it is painted on one canvas, row by row, each row's text shaped
//! once with every glyph held to a cell's width, so the grid stays a grid.
//!
//! While it has focus every key goes to the shell (Esc and Control keys too), but the app's
//! Command keys, which the window binds outside it, stay the app's. ⌘C copies a selection, ⌘V
//! pastes (bracketed, when the program asks). The mouse selects (a word on a double-click, a
//! line on a triple), or, when a full-screen program asks for the mouse, is the program's (Shift
//! held, it selects all the same). The wheel scrolls back, or, in a full-screen program, is
//! sent to it.

use std::cell::Cell as Shared;
use std::path::PathBuf;
use std::rc::Rc;

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::CursorShape;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::theme::*;
use crate::terminal::colors::{self, BACKGROUND, CURSOR};
use crate::terminal::{Shell, Size, keys};

/// The terminal's text, and its room inside the content box.
const FONT_SIZE: f32 = 13.;
const LINE_HEIGHT: f32 = 1.3;
const PAD: f32 = 10.;

/// Set, no shell starts: the terminal is drawn without one. For the app's window tests, whose
/// scheduler runs everything on one thread and cannot have a shell's own waking it.
pub struct Headless;

impl Global for Headless {}

/// One project's terminal: its shell once started, or why it could not be.
pub struct TerminalPane {
    dir: PathBuf,
    /// What it runs instead of a shell, if anything: a program and its arguments.
    program: Option<(String, Vec<String>)>,
    shell: Option<Shell>,
    error: Option<String>,
    focus: FocusHandle,
    /// Reads what the shell says, for as long as it runs.
    _events: Option<Task<()>>,
    /// Where the grid was painted and a cell's size, for the mouse.
    origin: Rc<Shared<Point<Pixels>>>,
    cell: Rc<Shared<(f32, f32)>>,
    /// The wheel's movement not yet a whole line.
    wheel: f32,
    /// The mouse button pressed in the terminal and not yet let go: followed anywhere in the
    /// window, so a drag out of it, and its release, still reach the selection or the program.
    held: Option<MouseButton>,
}

/// What a frame paints: each row's text with its runs, the backgrounds unlike the screen's,
/// wide characters on their own, and the cursor's outline when it is drawn hollow.
struct Frame {
    rows: Vec<(String, Vec<TextRun>)>,
    fills: Vec<(usize, usize, usize, Hsla)>,
    wide: Vec<(usize, usize, char, Hsla)>,
    hollow: Option<(usize, usize)>,
    beam: Option<(usize, usize, bool)>,
}

impl TerminalPane {
    pub fn new(dir: PathBuf, cx: &mut Context<Self>) -> TerminalPane {
        TerminalPane {
            dir,
            program: None,
            shell: None,
            error: None,
            focus: cx.focus_handle(),
            _events: None,
            origin: Rc::default(),
            cell: Rc::new(Shared::new((8., 17.))),
            wheel: 0.,
            held: None,
        }
    }

    /// A terminal that runs `program` with `args` instead of a shell.
    pub fn running(
        dir: PathBuf,
        program: String,
        args: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        TerminalPane {
            program: Some((program, args)),
            ..TerminalPane::new(dir, cx)
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The shell, if it runs.
    pub fn shell(&self) -> Option<&Shell> {
        self.shell.as_ref()
    }

    /// Laid out at `size`: the shell started at that size, or told of it.
    fn fit(&mut self, size: Size, cx: &mut Context<Self>) {
        if cx.has_global::<Headless>() {
            return;
        }
        match &mut self.shell {
            Some(shell) => shell.resize(size),
            None if self.error.is_none() => match match &self.program {
                Some((program, args)) => Shell::spawn_program(&self.dir, size, program, args),
                None => Shell::spawn(&self.dir, size),
            } {
                Ok((shell, events)) => {
                    self.shell = Some(shell);
                    self._events = Some(cx.spawn(async move |this, cx| {
                        while let Ok(event) = events.recv().await {
                            let alive = this.update(cx, |pane, cx| {
                                if let Some(shell) = &mut pane.shell
                                    && shell.take(&event)
                                {
                                    cx.notify();
                                }
                            });
                            if alive.is_err() {
                                break;
                            }
                        }
                    }));
                }
                Err(e) => self.error = Some(format!("The shell would not start: {e}")),
            },
            None => {}
        }
    }

    /// A grid point from a point in the window, and which half of its cell it is in.
    fn grid_point(
        &self,
        at: Point<Pixels>,
        display_offset: usize,
        size: Size,
    ) -> (GridPoint, Side) {
        let o = self.origin.get();
        let (w, h) = self.cell.get();
        let x = f32::from(at.x - o.x).max(0.);
        let y = f32::from(at.y - o.y).max(0.);
        let col = ((x / w) as usize).min(size.columns.saturating_sub(1));
        let row = ((y / h) as usize).min(size.lines.saturating_sub(1));
        let side = if x - col as f32 * w < w / 2. {
            Side::Left
        } else {
            Side::Right
        };
        let line = i32::try_from(row).unwrap_or(0) - i32::try_from(display_offset).unwrap_or(0);
        (GridPoint::new(Line(line), Column(col)), side)
    }

    fn key_down(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let Some(shell) = &mut self.shell else {
            return;
        };
        // A shell that exited: Enter starts another.
        if shell.exited {
            if k.key == "enter" && !k.modifiers.modified() {
                self.shell = None;
                self._events = None;
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        let m = &k.modifiers;
        if m.platform && k.key == "c" {
            if let Some(text) = shell.with_term(|t| t.selection_to_string()) {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                cx.stop_propagation();
            }
            return;
        }
        if m.platform && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                let bracketed = shell.with_term(|t| t.mode().contains(TermMode::BRACKETED_PASTE));
                let text = if bracketed {
                    format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', ""))
                } else {
                    text.replace("\r\n", "\r").replace('\n', "\r")
                };
                shell.with_term(|t| t.scroll_display(Scroll::Bottom));
                shell.write(text.into_bytes());
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        // Shift with Page Up and Down scrolls back through what went by.
        if m.shift && !m.control && !m.alt && !m.platform {
            let page = match k.key.as_str() {
                "pageup" => Some(Scroll::PageUp),
                "pagedown" => Some(Scroll::PageDown),
                _ => None,
            };
            if let Some(page) = page {
                shell.with_term(|t| t.scroll_display(page));
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        let app_cursor = shell.with_term(|t| t.mode().contains(TermMode::APP_CURSOR));
        if let Some(bytes) = keys::to_bytes(k, app_cursor) {
            shell.with_term(|t| {
                t.selection = None;
                t.scroll_display(Scroll::Bottom);
            });
            shell.write(bytes);
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// The mouse pressed, moved with a button held, or let go, at `at`: the program's, when it
    /// asks for the mouse (and Shift is not held), else a selection.
    fn mouse(
        &mut self,
        at: Point<Pixels>,
        button: Option<MouseButton>,
        motion: Motion,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let pressed = !matches!(motion, Motion::Up);
        let moving = matches!(motion, Motion::Drag);
        let Some(shell) = &self.shell else {
            return;
        };
        let size = shell.size();
        let (mode, offset) = shell.with_term(|t| (*t.mode(), t.grid().display_offset()));
        let (point, side) = self.grid_point(at, offset, size);
        if mode.intersects(TermMode::MOUSE_MODE) && !modifiers.shift {
            let code = match button {
                Some(MouseButton::Left) => 0,
                Some(MouseButton::Middle) => 1,
                Some(MouseButton::Right) => 2,
                _ => 3,
            };
            if moving && !mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) {
                return;
            }
            let column = point.column.0;
            let line =
                usize::try_from(point.line.0 + i32::try_from(offset).unwrap_or(0)).unwrap_or(0);
            let code = code + if moving { 32 } else { 0 } + modifier_bits(modifiers);
            if let Some(bytes) = keys::mouse(code, column, line, pressed, mode) {
                shell.write(bytes);
            }
            return;
        }
        if button != Some(MouseButton::Left) {
            return;
        }
        shell.with_term(|t| {
            if pressed && !moving {
                let clicks = match motion {
                    Motion::Down(n) => n,
                    _ => 1,
                };
                let ty = match clicks {
                    2 => SelectionType::Semantic,
                    n if n >= 3 => SelectionType::Lines,
                    _ => SelectionType::Simple,
                };
                t.selection = Some(Selection::new(ty, point, side));
            } else if moving && let Some(s) = &mut t.selection {
                s.update(point, side);
            }
        });
        cx.notify();
    }

    fn scroll(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(shell) = &self.shell else {
            return;
        };
        let (_, h) = self.cell.get();
        let lines = match e.delta {
            ScrollDelta::Lines(p) => p.y,
            ScrollDelta::Pixels(p) => f32::from(p.y) / h,
        };
        self.wheel += lines;
        let whole = self.wheel.trunc();
        self.wheel -= whole;
        if whole == 0. {
            return;
        }
        let n = whole as i32;
        let mode = shell.with_term(|t| *t.mode());
        if mode.intersects(TermMode::MOUSE_MODE) && !e.modifiers.shift {
            let size = shell.size();
            let (point, _) = self.grid_point(e.position, 0, size);
            let code = if n > 0 { 64 } else { 65 };
            for _ in 0..n.unsigned_abs().min(10) {
                if let Some(bytes) = keys::mouse(
                    code,
                    point.column.0,
                    usize::try_from(point.line.0).unwrap_or(0),
                    true,
                    mode,
                ) {
                    shell.write(bytes);
                }
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            // A full-screen program without the mouse: the wheel is its arrow keys.
            let key = if n > 0 { b'A' } else { b'B' };
            let app = mode.contains(TermMode::APP_CURSOR);
            let one = [0x1b, if app { b'O' } else { b'[' }, key];
            let bytes: Vec<u8> = one
                .iter()
                .copied()
                .cycle()
                .take(3 * n.unsigned_abs() as usize)
                .collect();
            shell.write(bytes);
        } else {
            shell.with_term(|t| t.scroll_display(Scroll::Delta(n)));
            cx.notify();
        }
    }

    /// What to paint, read from the screen while it is held.
    fn frame(&self, focused: bool, font: &Font) -> Option<Frame> {
        let shell = self.shell.as_ref()?;
        Some(shell.with_term(|t| {
            let content = t.renderable_content();
            let colors = content.colors;
            let offset = content.display_offset;
            let lines = t.grid().screen_lines();
            let selection = content.selection;
            let cursor = content.cursor;
            let mut rows: Vec<(String, Vec<TextRun>)> = vec![(String::new(), Vec::new()); lines];
            let mut fills = Vec::new();
            let mut wide = Vec::new();
            let mut hollow = None;
            let mut beam = None;
            let selected = with_alpha(ACCENT, 0.35);
            for indexed in content.display_iter {
                let row =
                    usize::try_from(indexed.point.line.0 + i32::try_from(offset).unwrap_or(0))
                        .unwrap_or(0);
                if row >= lines {
                    continue;
                }
                let col = indexed.point.column.0;
                let cell = &indexed.cell;
                let flags = cell.flags;
                let mut fg = colors::rgb(cell.fg, colors);
                let mut bg = colors::rgb(cell.bg, colors);
                if flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if flags.intersects(Flags::DIM) {
                    fg = colors::dim(fg);
                }
                if flags.contains(Flags::HIDDEN) {
                    fg = bg;
                }
                let at_cursor = indexed.point == cursor.point;
                let mut bg_color = (bg != BACKGROUND).then(|| color(bg));
                if selection.is_some_and(|s| s.contains(indexed.point)) {
                    bg_color = Some(selected);
                }
                if at_cursor {
                    match cursor.shape {
                        CursorShape::Block if focused => {
                            bg_color = Some(color(CURSOR));
                            fg = BACKGROUND;
                        }
                        CursorShape::Block | CursorShape::HollowBlock => hollow = Some((row, col)),
                        CursorShape::Beam => beam = Some((row, col, true)),
                        CursorShape::Underline => beam = Some((row, col, false)),
                        CursorShape::Hidden => {}
                    }
                }
                if let Some(bg) = bg_color {
                    let width = if flags.contains(Flags::WIDE_CHAR) {
                        2
                    } else {
                        1
                    };
                    fills.push((row, col, width, bg));
                }
                let (text, runs) = &mut rows[row];
                // A wide character is drawn on its own across its two cells; in the row, its
                // cells hold spaces, so what follows stays in its columns.
                let c = if flags.contains(Flags::WIDE_CHAR) {
                    wide.push((row, col, cell.c, color(fg)));
                    ' '
                } else if flags.contains(Flags::WIDE_CHAR_SPACER) || cell.c == '\0' {
                    ' '
                } else {
                    cell.c
                };
                let run_font = Font {
                    weight: if flags.contains(Flags::BOLD) {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    },
                    style: if flags.contains(Flags::ITALIC) {
                        FontStyle::Italic
                    } else {
                        FontStyle::Normal
                    },
                    ..font.clone()
                };
                let underline = flags
                    .intersects(Flags::ALL_UNDERLINES)
                    .then(|| UnderlineStyle {
                        thickness: px(1.),
                        color: Some(color(fg)),
                        wavy: flags.contains(Flags::UNDERCURL),
                    });
                let strikethrough = flags
                    .contains(Flags::STRIKEOUT)
                    .then(|| StrikethroughStyle {
                        thickness: px(1.),
                        color: Some(color(fg)),
                    });
                let len = c.len_utf8();
                text.push(c);
                match runs.last_mut() {
                    Some(r)
                        if r.color == color(fg)
                            && r.font == run_font
                            && r.underline == underline
                            && r.strikethrough == strikethrough =>
                    {
                        r.len += len;
                    }
                    _ => runs.push(TextRun {
                        len,
                        font: run_font,
                        color: color(fg),
                        background_color: None,
                        underline,
                        strikethrough,
                    }),
                }
            }
            Frame {
                rows,
                fills,
                wide,
                hollow,
                beam,
            }
        }))
    }
}

/// What the mouse did: pressed (with how many clicks), moved with a button held, let go.
#[derive(Clone, Copy)]
enum Motion {
    Down(usize),
    Drag,
    Up,
}

/// The modifiers as xterm's mouse reports carry them.
fn modifier_bits(m: Modifiers) -> u8 {
    4 * u8::from(m.shift) + 8 * u8::from(m.alt) + 16 * u8::from(m.control)
}

impl Render for TerminalPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = gpui_component::Theme::global(cx).mono_font_family.clone();
        let font = Font {
            family: mono,
            ..font(".SystemUIFont")
        };
        let focused = self.focus.is_focused(window);
        let exited = self.shell.as_ref().is_some_and(|s| s.exited);
        let this = cx.entity().downgrade();
        let frame = self.frame(focused, &font).map(Rc::new);
        let (origin, cell) = (self.origin.clone(), self.cell.clone());
        let held = self.held;
        let follow = this.clone();
        let paint_font = font.clone();
        let grid = canvas(
            {
                let this = this.clone();
                let font = font.clone();
                move |bounds, window, cx| {
                    // A cell: the font's advance across, its line down.
                    let size = px(FONT_SIZE);
                    let shaped = window.text_system().shape_line(
                        "M".into(),
                        size,
                        &[TextRun {
                            len: 1,
                            font: font.clone(),
                            color: color(TEXT),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        None,
                    );
                    let w = f32::from(shaped.width).max(1.);
                    let h = (FONT_SIZE * LINE_HEIGHT).round();
                    let columns = ((f32::from(bounds.size.width) - 2. * PAD) / w)
                        .floor()
                        .max(2.) as usize;
                    let lines = ((f32::from(bounds.size.height) - 2. * PAD) / h)
                        .floor()
                        .max(1.) as usize;
                    let fit = Size {
                        columns,
                        lines,
                        cell_width: w.round() as u16,
                        cell_height: h as u16,
                    };
                    let _ = this.update(cx, |pane, cx| {
                        pane.cell.set((w, h));
                        pane.fit(fit, cx);
                    });
                }
            },
            move |bounds, _, window, cx| {
                // A button held: its moves and its release, wherever in the window they are.
                if let Some(button) = held {
                    let moved = follow.clone();
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture && e.pressed_button.is_some() {
                            let _ = moved.update(cx, |pane, cx| {
                                pane.mouse(e.position, Some(button), Motion::Drag, e.modifiers, cx);
                            });
                        }
                    });
                    let released = follow.clone();
                    window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture && e.button == button {
                            let _ = released.update(cx, |pane, cx| {
                                pane.held = None;
                                pane.mouse(e.position, Some(button), Motion::Up, e.modifiers, cx);
                            });
                        }
                    });
                }
                let Some(frame) = &frame else { return };
                let wide_font = paint_font.clone();
                let o = bounds.origin + point(px(PAD), px(PAD));
                origin.set(o);
                let (w, h) = cell.get();
                let at = |row: usize, col: usize| o + point(px(col as f32 * w), px(row as f32 * h));
                for &(row, col, width, bg) in &frame.fills {
                    window.paint_quad(gpui_kit::fill(
                        Bounds::new(at(row, col), size(px(w * width as f32), px(h))),
                        bg,
                    ));
                }
                if let Some((row, col, beam)) = frame.beam {
                    let b = if beam {
                        Bounds::new(at(row, col), size(px(2.), px(h)))
                    } else {
                        Bounds::new(
                            at(row, col) + point(px(0.), px(h - 2.)),
                            size(px(w), px(2.)),
                        )
                    };
                    window.paint_quad(gpui_kit::fill(b, color(CURSOR)));
                }
                if let Some((row, col)) = frame.hollow {
                    window.paint_quad(outline(
                        Bounds::new(at(row, col), size(px(w), px(h))),
                        color(CURSOR),
                        BorderStyle::Solid,
                    ));
                }
                let font_size = px(FONT_SIZE);
                for (row, (text, runs)) in frame.rows.iter().enumerate() {
                    if text.trim_end().is_empty() {
                        continue;
                    }
                    let line = window.text_system().shape_line(
                        SharedString::from(text.clone()),
                        font_size,
                        runs,
                        Some(px(w)),
                    );
                    let _ = line.paint(at(row, 0), px(h), TextAlign::Left, None, window, cx);
                }
                for &(row, col, c, fg) in &frame.wide {
                    let text = c.to_string();
                    let line = window.text_system().shape_line(
                        SharedString::from(text.clone()),
                        font_size,
                        &[TextRun {
                            len: text.len(),
                            font: wide_font.clone(),
                            color: fg,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        Some(px(2. * w)),
                    );
                    let _ = line.paint(at(row, col), px(h), TextAlign::Left, None, window, cx);
                }
            },
        )
        .size_full();
        div()
            .id("terminal")
            .debug_selector(|| "terminal".into())
            .key_context("Terminal")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .bg(color(BACKGROUND))
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| this.key_down(e, cx)))
            // Any button: a program that asked for the mouse hears the right and the middle too.
            .on_any_mouse_down(cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.focus.focus(window, cx);
                this.held = Some(e.button);
                // Drawn again, so the window follows the button from now on.
                cx.notify();
                this.mouse(
                    e.position,
                    Some(e.button),
                    Motion::Down(e.click_count),
                    e.modifiers,
                    cx,
                );
            }))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.scroll(e, cx)))
            .child(grid)
            .when_some(self.error.clone(), |d, error| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(color(TEXT_3))
                        .child(error),
                )
            })
            .when(exited, |d| {
                d.child(
                    div()
                        .debug_selector(|| "terminal-exited".into())
                        .absolute()
                        .bottom(px(16.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .px_3()
                                .py_1()
                                .rounded_lg()
                                .bg(color(SURFACE))
                                .border_1()
                                .border_color(color(BORDER_STRONG))
                                .text_sm()
                                .text_color(color(TEXT_3))
                                .child("The shell exited · Enter starts a new one"),
                        ),
                )
            })
    }
}

impl Focusable for TerminalPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
