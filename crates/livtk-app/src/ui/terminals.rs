//! A project's terminals, the way VS Code keeps them: several shells, one shown. With more than
//! one, a list at the terminal's right names each (by what its shell calls its window), the one
//! shown lit; a click shows another, its × ends it. ⌘T starts another. A shell that exits leaves
//! the list while others remain; the last stays, showing how it ended.

use std::path::PathBuf;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::icons::{Icon, icon, icon_hover};
use super::terminal::TerminalPane;
use super::theme::*;
use super::{LivtkApp, View};

/// The list's width, and a line's height.
const LIST_W: f32 = 184.;
const LINE: f32 = 28.;

/// A project's terminals and the one shown.
#[derive(Default)]
pub(super) struct Terminals {
    pub list: Vec<Entity<TerminalPane>>,
    pub shown: usize,
}

impl Terminals {
    pub fn current(&self) -> Option<&Entity<TerminalPane>> {
        self.list.get(self.shown)
    }
}

impl LivtkApp {
    /// The open project's terminals, made (with one terminal) the first time.
    fn project_terminals(&mut self, cx: &mut Context<Self>) -> Option<&mut Terminals> {
        let dir = self.model.project()?.dir.clone();
        let t = self.terminals.entry(dir.clone()).or_default();
        if t.list.is_empty() {
            t.list.push(new_pane(dir, cx));
            t.shown = 0;
        }
        Some(t)
    }

    /// In the Terminal view, the terminal shown, made the first time; a shell that exited
    /// leaves while others remain.
    pub(super) fn terminal_view(&mut self, cx: &mut Context<Self>) -> Option<Entity<TerminalPane>> {
        if self.model.view() != View::Terminal || self.model.placeholder() {
            return None;
        }
        let t = self.project_terminals(cx)?;
        if t.list.len() > 1 {
            let exited = |p: &Entity<TerminalPane>| p.read(cx).shell().is_some_and(|s| s.exited);
            let before = t.list.len();
            let keep: Vec<_> = t.list.iter().filter(|p| !exited(p)).cloned().collect();
            if keep.len() != before {
                t.list = if keep.is_empty() {
                    vec![t.list[t.list.len() - 1].clone()]
                } else {
                    keep
                };
                t.shown = t.shown.min(t.list.len() - 1);
            }
        }
        t.current().cloned()
    }

    /// ⌘T: another terminal, shown; from another view, the terminals (another only when there
    /// is one already).
    pub(super) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let had = self
            .model
            .project()
            .and_then(|p| self.terminals.get(&p.dir))
            .is_some_and(|t| !t.list.is_empty());
        self.go(View::Terminal, window, cx);
        if self.model.view() != View::Terminal || !had {
            return;
        }
        let Some(dir) = self.model.project().map(|p| p.dir.clone()) else {
            return;
        };
        let pane = new_pane(dir.clone(), cx);
        if let Some(t) = self.terminals.get_mut(&dir) {
            t.list.push(pane);
            t.shown = t.list.len() - 1;
        }
        // Its keys once it is drawn.
        self.last_place = None;
        cx.notify();
    }

    /// Show terminal `i` of the open project's, with the keys.
    fn show_terminal(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = self
            .model
            .project()
            .map(|p| p.dir.clone())
            .and_then(|d| self.terminals.get_mut(&d))
            && i < t.list.len()
        {
            t.shown = i;
            self.focus_main(window, cx);
            cx.notify();
        }
    }

    /// End terminal `i` of the open project's (its shell with it); never the last.
    pub(super) fn kill_terminal(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = self
            .model
            .project()
            .map(|p| p.dir.clone())
            .and_then(|d| self.terminals.get_mut(&d))
            && t.list.len() > 1
            && i < t.list.len()
        {
            t.list.remove(i);
            if t.shown >= i && t.shown > 0 {
                t.shown -= 1;
            }
            self.focus_main(window, cx);
            cx.notify();
        }
    }

    /// The list of the open project's terminals, at the terminal's right, while there is more
    /// than one.
    pub(super) fn terminal_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = self.terminals.get(&self.model.project()?.dir)?;
        if t.list.len() < 2 {
            return None;
        }
        let shell = std::env::var("SHELL")
            .ok()
            .and_then(|s| s.rsplit('/').next().map(str::to_owned))
            .unwrap_or_else(|| "zsh".to_owned());
        let mut list = div()
            .debug_selector(|| "terminal-list".into())
            .w(px(LIST_W))
            .flex_none()
            .h_full()
            .p_1()
            .flex()
            .flex_col()
            .gap(px(2.))
            .border_l_1()
            .border_color(color(BORDER));
        for (i, pane) in t.list.iter().enumerate() {
            let shown = i == t.shown;
            let title = pane
                .read(cx)
                .shell()
                .and_then(|s| s.title.clone())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| shell.clone());
            let id = format!("terminal-{i}");
            let kill = format!("terminal-kill-{i}");
            list = list.child(
                div()
                    .id(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
                    .group("terminal-line")
                    .h(px(LINE))
                    .flex_none()
                    .pl_2()
                    .pr_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_lg()
                    .text_sm()
                    .text_color(color(if shown { TEXT } else { TEXT_3 }))
                    .when(shown, |d| d.bg(color(SELECTED)))
                    .when(!shown, |d| d.hover(|h| h.bg(color(HOVER))))
                    .child(icon(
                        Icon::Terminal,
                        14.,
                        if shown { TEXT_2 } else { TEXT_4 },
                    ))
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .child(
                        div()
                            .id(SharedString::from(kill.clone()))
                            .debug_selector(move || kill.clone())
                            .group("terminal-kill")
                            .size(px(20.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .invisible()
                            .group_hover("terminal-line", |s| s.visible())
                            .hover(|h| h.bg(color(HOVER)))
                            .child(icon_hover(Icon::Close, 13., TEXT_4, "terminal-kill", TEXT))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Kill the terminal")
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.kill_terminal(i, window, cx);
                            })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show_terminal(i, window, cx);
                    })),
            );
        }
        Some(list.into_any_element())
    }

    /// The terminal shown, and the list beside it while there are several.
    pub(super) fn terminal_element(
        &self,
        pane: Entity<TerminalPane>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(div().flex_1().min_w_0().h_full().child(pane))
            .children(self.terminal_list(cx))
            .into_any_element()
    }
}

fn new_pane(dir: PathBuf, cx: &mut Context<LivtkApp>) -> Entity<TerminalPane> {
    cx.new(|cx| TerminalPane::new(dir, cx))
}
