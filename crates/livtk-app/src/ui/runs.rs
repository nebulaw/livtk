//! Runs of Claude, as the window keeps up with them: surveyed as it ticks, off its thread; asked
//! about before a quit while any is writing (keep them working, stop them all, or cancel); and
//! the ones the last quit stopped, offered back once on the next start.

use std::time::Instant;

use livtk_core::run::Runner;

use super::form::button;
use super::*;
use crate::model::runs::{self, RunRow};

/// What the runs modal asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Asking {
    /// Quitting, with runs writing: then quit, or restart to update.
    Quit(Then),
    /// Starting again, with runs the last quit stopped.
    Continue,
}

/// What a quit is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Then {
    Quit,
    Update,
}

/// An answer to the runs modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Cancel,
    StopAll,
    Keep,
    Leave,
    Continue,
}

impl Choice {
    const fn label(self) -> &'static str {
        match self {
            Choice::Cancel => "Cancel",
            Choice::StopAll => "Stop them all",
            Choice::Keep => "Keep them working",
            Choice::Leave => "Leave them stopped",
            Choice::Continue => "Continue them",
        }
    }

    const fn selector(self) -> &'static str {
        match self {
            Choice::Cancel => "runs-cancel",
            Choice::StopAll => "runs-stop",
            Choice::Keep => "runs-keep",
            Choice::Leave => "runs-leave",
            Choice::Continue => "runs-continue",
        }
    }
}

impl Asking {
    /// Its answers, the main one last.
    const fn choices(self) -> &'static [Choice] {
        match self {
            Asking::Quit(_) => &[Choice::Cancel, Choice::StopAll, Choice::Keep],
            Asking::Continue => &[Choice::Leave, Choice::Continue],
        }
    }
}

fn runner() -> Option<Runner> {
    crate::agents::claude().map(|claude| Runner { claude })
}

impl LivtkApp {
    /// Settle and look at the open projects' runs, off the window's thread, when due.
    pub(super) fn survey_runs(&mut self, cx: &mut Context<Self>) {
        if !self.model.runs_due(Instant::now()) {
            return;
        }
        self.model.runs.surveying = true;
        let roots = self.model.project_roots();
        let found = cx
            .background_executor()
            .spawn(async move { runs::survey(&roots, runner().as_ref()) });
        cx.spawn(async move |this, cx| {
            let (going, quit) = found.await;
            let _ = this.update(cx, |app, cx| {
                app.model.runs_surveyed(going, quit, Instant::now());
                // Once a launch: what the last quit stopped, offered back.
                if !app.runs_offered && !app.model.runs.quit.is_empty() && app.asking.is_none() {
                    app.runs_offered = true;
                    app.asking = Some(Asking::Continue);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Quit (or restart to update), asking first while runs are writing: as they are now,
    /// not as the last survey found them.
    pub(super) fn ask_quit(&mut self, then: Then, cx: &mut Context<Self>) {
        let (going, _) = runs::survey(&self.model.project_roots(), None);
        self.model.runs.going = going;
        if self.model.runs.going.is_empty() {
            self.quit_for(then, cx);
            return;
        }
        self.asking = Some(Asking::Quit(then));
        cx.notify();
    }

    /// Quit now, for what the quit is for.
    fn quit_for(&mut self, then: Then, cx: &mut Context<Self>) {
        match then {
            Then::Quit => {
                self.quitting = true;
                cx.quit();
            }
            Then::Update => self.install_update(cx),
        }
    }

    /// Whether the app has quit, or is quitting.
    pub fn quitting(&self) -> bool {
        self.quitting
    }

    fn answer_runs(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        let Some(asking) = self.asking.take() else {
            return;
        };
        let then = match asking {
            Asking::Quit(then) => then,
            Asking::Continue => Then::Quit,
        };
        let failed = match (choice, runner()) {
            (Choice::Cancel, _) => Vec::new(),
            (Choice::Keep, _) => {
                self.quit_for(then, cx);
                return;
            }
            (_, None) => vec!["the claude command is not where livtk looks".to_owned()],
            (Choice::StopAll, Some(r)) => {
                let failed = runs::stop_all(&self.model.runs.going, &r);
                if failed.is_empty() {
                    self.quit_for(then, cx);
                    return;
                }
                failed
            }
            (Choice::Leave | Choice::Continue, Some(r)) => {
                let rows = std::mem::take(&mut self.model.runs.quit);
                runs::take_back(&rows, &r, choice == Choice::Continue)
            }
        };
        debug_assert!(asking.choices().contains(&choice));
        if !failed.is_empty() {
            self.model.warn(format!(
                "Not every run could be told: {}",
                failed.join("; ")
            ));
        }
        self.restore_focus(window, cx);
        cx.notify();
    }

    /// The runs modal: what is asked, the runs it is about, and the answers.
    pub(super) fn runs_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let asking = self.asking?;
        // Asked from a menu or the Dock: the keys come here, so Esc cancels.
        if !self.modal_focus.is_focused(window) {
            self.modal_focus.focus(window, cx);
        }
        let (title, said, rows): (&str, String, &[RunRow]) = match asking {
            Asking::Quit(_) => (
                "Agents are working",
                format!(
                    "{} working. They can go on without the app, and it finds them again \
                         when it starts; or they stop, their work kept, and it offers to \
                         continue them.",
                    count(self.model.runs.going.len(), "is", "are")
                ),
                &self.model.runs.going,
            ),
            Asking::Continue => (
                "Agents were stopped",
                format!(
                    "{} stopped when livtk last quit, the work kept.",
                    count(self.model.runs.quit.len(), "was", "were")
                ),
                &self.model.runs.quit,
            ),
        };
        let mut list = div().flex().flex_col().gap_1();
        for r in rows {
            let key = r.key.clone();
            list =
                list.child(
                    div()
                        .debug_selector(move || format!("asked-run-{key}"))
                        .flex()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .flex_none()
                                .text_color(color(TEXT_4))
                                .child(r.key.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(color(TEXT_2))
                                .child(r.title.clone()),
                        )
                        .child(div().flex_none().text_xs().text_color(color(TEXT_4)).child(
                            format!(
                                "{} · {}",
                                r.stage.as_str(),
                                r.agent.as_deref().unwrap_or("session")
                            ),
                        )),
                );
        }
        let mut buttons = div().flex().justify_end().gap_2();
        let choices = asking.choices();
        for (i, &choice) in choices.iter().enumerate() {
            let main = i + 1 == choices.len();
            buttons = buttons.child(button(choice.selector(), choice.label(), main).on_click(
                cx.listener(move |this, _, window, cx| this.answer_runs(choice, window, cx)),
            ));
        }
        let card = div()
            .debug_selector(move || match asking {
                Asking::Quit(_) => "quit-runs".into(),
                Asking::Continue => "continue-runs".into(),
            })
            .w(px(460.))
            .p_6()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color(TEXT))
                    .child(title),
            )
            .child(div().text_sm().text_color(color(TEXT_2)).child(said))
            .child(list)
            .child(buttons);
        Some(self.modal("runs-modal", true, true, card, cx))
    }
}

/// "One agent is", "3 agents are": `one` and `many` the verb for each.
fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("One agent {one}")
    } else {
        format!("{n} agents {many}")
    }
}
