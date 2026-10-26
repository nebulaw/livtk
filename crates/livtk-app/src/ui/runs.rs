//! Runs of Claude, as the window keeps up with them: surveyed as it ticks, off its thread; asked
//! about before a quit while any is writing (keep them working, stop them all, or cancel); and
//! the ones the last quit stopped, offered back once on the next start.

use std::time::Instant;

use livtk_core::run::Runner;

use super::form::{button, icon_button};
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

    /// The icon its button leads with, if any: Stop them all and Continue them do as a
    /// task's Stop and Start do.
    const fn icon(self) -> Option<Icon> {
        match self {
            Choice::StopAll => Some(Icon::Stop),
            Choice::Continue => Some(Icon::Continue),
            _ => None,
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

/// What no agent picked is called: livtk's own for the stage runs.
const DEFAULT_AGENT: &str = "Default";

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
        let found = cx.background_executor().spawn(async move {
            let runner = runner();
            (runs::survey(&roots, runner.as_ref()), runner.is_some())
        });
        cx.spawn(async move |this, cx| {
            let (found, claude) = found.await;
            let _ = this.update(cx, |app, cx| {
                app.model.runs.claude = claude;
                app.model.runs_surveyed(found, Instant::now());
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
        self.model.runs.going = runs::survey(&self.model.project_roots(), None).going;
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
            let b = match choice.icon() {
                Some(g) => icon_button(choice.selector(), g, choice.label(), main),
                None => button(choice.selector(), choice.label(), main),
            };
            buttons = buttons.child(b.on_click(
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

/// `text` with its first letter a capital: "Waits for LIV-2".
fn upper_first(text: &str) -> String {
    let mut c = text.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// "One agent is", "3 agents are": `one` and `many` the verb for each.
fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("One agent {one}")
    } else {
        format!("{n} agents {many}")
    }
}

impl LivtkApp {
    /// A task row's mark of its run: waiting for you, or Claude writing on it.
    pub(super) fn run_tag(&self, key: &str) -> Option<Div> {
        if self.model.run_needs_you(key) {
            Some(
                icon_tag(Icon::Inbox, "Needs you", ACCENT)
                    .debug_selector(|| "row-needs-you".into()),
            )
        } else if self.model.writing_on(key) {
            Some(icon_tag(Icon::Agents, "Writing…", TEXT_3).debug_selector(|| "row-writing".into()))
        } else {
            None
        }
    }
}

// ---- a task's run, in its panel -------------------------------------------------------------

/// What a run is asked to do from a task's panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ask {
    Start,
    Stop,
    Allow,
    /// Allow, and the project's runs of its stage from now on: never asked again.
    AllowAlways,
    Deny,
}

impl Ask {
    /// The icon its button leads with.
    const fn icon(self) -> Icon {
        match self {
            Ask::Start | Ask::Allow | Ask::AllowAlways => Icon::Start,
            Ask::Stop | Ask::Deny => Icon::Stop,
        }
    }
}

impl LivtkApp {
    /// Where the task's builds work, picked: their own git worktree (a branch of their own,
    /// merged when done), the project's checkout, or as the project says.
    pub(super) fn worktree_button(&self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let n = &d.node;
        let own = n.props.get("worktree").and_then(serde_json::Value::as_bool);
        let project = self.model.project_worktree();
        let said = |w: bool| {
            if w {
                "Its own worktree"
            } else {
                "The project's checkout"
            }
        };
        let label = match own {
            Some(w) => said(w).to_owned(),
            None => format!("{} (project)", said(project)),
        };
        let open = self.menu == Some(Menu::DetailWorktree);
        let button = div()
            .id("detail-worktree")
            .debug_selector(|| "detail-worktree".into())
            .h(px(22.))
            .px(px(4.))
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .rounded_lg()
            .when(open, |b| b.bg(color(SELECTED)))
            .when(!open, |b| b.hover(|h| h.bg(color(HOVER))))
            .child(icon(Icon::Project, 14., TEXT_3))
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_menu(Menu::DetailWorktree, window, cx);
                }),
            );
        let id = n.id;
        div().relative().flex_none().child(button).when(open, |d| {
            let mut panel = menu_panel(260.).debug_selector(|| "set-worktree-menu".into());
            let choices = [
                (
                    None,
                    format!("As the project: {}", said(project).to_lowercase()),
                ),
                (Some(true), said(true).to_owned()),
                (Some(false), said(false).to_owned()),
            ];
            for (choice, label) in choices {
                let row_id = format!(
                    "set-worktree-{}",
                    choice.map_or("default", |w| if w { "on" } else { "off" })
                );
                panel = panel.child(
                    option_row(SharedString::from(row_id.clone()))
                        .debug_selector(move || row_id.clone())
                        .child(div().flex_1().truncate().child(label))
                        .child(check(choice == own))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.model.set_worktree(id, choice);
                            this.close_menu(window, cx);
                        })),
                );
            }
            d.child(self.popover(Menu::DetailWorktree, false, 26., panel, cx))
        })
    }

    /// The task's agent, picked from every agent there is; none, the Default: livtk's own for
    /// the stage.
    pub(super) fn agent_button(&self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let n = &d.node;
        let current = n
            .props
            .get("agent")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let open = self.menu == Some(Menu::DetailAgent);
        let button = div()
            .id("detail-agent")
            .debug_selector(|| "detail-agent".into())
            .h(px(22.))
            .px(px(4.))
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .rounded_lg()
            .when(open, |b| b.bg(color(SELECTED)))
            .when(!open, |b| b.hover(|h| h.bg(color(HOVER))))
            .child(icon(Icon::Agents, 14., TEXT_3))
            .child(current.clone().unwrap_or_else(|| DEFAULT_AGENT.to_owned()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.model.load_agents();
                    this.toggle_menu(Menu::DetailAgent, window, cx);
                }),
            );
        let id = n.id;
        div().relative().flex_none().child(button).when(open, |d| {
            let mut panel = menu_panel(240.).debug_selector(|| "set-agent-menu".into());
            let choices = std::iter::once(None)
                .chain(self.model.agents.files.iter().map(|f| Some(f.run_name())));
            for choice in choices {
                let label = choice.clone().unwrap_or_else(|| DEFAULT_AGENT.to_owned());
                let row_id = format!("set-agent-{}", choice.as_deref().unwrap_or("default"));
                let on = choice == current;
                panel = panel.child(
                    option_row(SharedString::from(row_id.clone()))
                        .debug_selector(move || row_id.clone())
                        .child(div().flex_1().truncate().child(label))
                        .child(check(on))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.model.set_agent(id, choice.clone());
                            this.close_menu(window, cx);
                        })),
                );
            }
            d.child(self.popover(Menu::DetailAgent, false, 26., panel, cx))
        })
    }

    /// The task's run: where it stands, and what can be done to it now (Start when the task's
    /// state is ready for a stage and nothing runs; Stop while one runs).
    pub(super) fn run_row(&self, d: &Detail, cx: &mut Context<Self>) -> Option<Div> {
        let n = &d.node;
        let going = d.run.as_ref().filter(|r| r.ended().is_none());
        let stage = livtk_core::run::Stage::for_state(n.kind, n.state);
        let starting = self.asked.get(&n.key) == Some(&Ask::Start);
        // Waiting for unfinished work: the status says so with the blocked mark.
        let mut waiting = false;
        let (status, ask): (String, Option<(Ask, String)>) = match going {
            _ if starting => ("Starting…".into(), None),
            Some(r) if self.model.writing_on(&n.key) => (
                format!("Writing… ({})", r.stage.as_str()),
                Some((Ask::Stop, "Stop".into())),
            ),
            Some(r) if r.needs_you() => ("Needs you".into(), Some((Ask::Stop, "Stop".into()))),
            Some(r) => (
                format!("Going on ({})", r.stage.as_str()),
                Some((Ask::Stop, "Stop".into())),
            ),
            None if self.model.queued_at(&n.key).is_some() => (
                format!(
                    "Waiting its turn (#{})",
                    self.model.queued_at(&n.key).unwrap_or(1)
                ),
                Some((Ask::Start, "Start now".into())),
            ),
            None => match (stage, &d.run) {
                (Some(_), _) if !self.model.runs.claude => ("Claude Code not found".into(), None),
                (Some(s), _)
                    if let Some(waits) =
                        livtk_core::run::waits_for(s, n.blocked, &d.links.depends_on) =>
                {
                    waiting = true;
                    (upper_first(&waits), None)
                }
                (Some(s), last) => (
                    last.as_ref()
                        .and_then(livtk_core::run::Run::ended)
                        .map_or_else(String::new, |how| format!("Last run {}", how.said())),
                    Some((Ask::Start, format!("Start to {}", s.as_str()))),
                ),
                (None, Some(last)) => (
                    last.ended()
                        .map_or_else(String::new, |how| format!("Last run {}", how.said())),
                    None,
                ),
                (None, None) => return None,
            },
        };
        // In the title bar beside the key: the status gives way first, never the buttons.
        let mut row = div()
            .debug_selector(|| "detail-run".into())
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .when(waiting, |r| {
                r.child(icon(Icon::Blocked, 14., TEXT_3).debug_selector(|| "run-waits".into()))
            })
            .child(
                div()
                    .debug_selector(|| "detail-run-status".into())
                    .min_w_0()
                    .truncate()
                    .text_color(color(TEXT_2))
                    .child(status),
            );
        // A build about to start says where it will work; a click switches it, for this task.
        let builds = going.is_none() && stage == Some(livtk_core::run::Stage::Build);
        if builds && matches!(ask, Some((Ask::Start, _))) {
            let own = n
                .props
                .get("worktree")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or_else(|| self.model.project_worktree());
            let id = n.id;
            row = row.child(
                div()
                    .id("run-worktree")
                    .debug_selector(|| "run-worktree".into())
                    .flex_none()
                    .h(px(22.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded_lg()
                    .text_xs()
                    .text_color(color(TEXT_3))
                    .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT_2)))
                    .child(icon(Icon::Project, 12., TEXT_4))
                    .child(if own {
                        "In its own worktree"
                    } else {
                        "In the checkout"
                    })
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(
                            "Where this build works; click to switch",
                        )
                        .build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.set_worktree(id, Some(!own));
                        cx.notify();
                    })),
            );
        }
        if let Some((ask, label)) = ask {
            let sel: &'static str = match ask {
                Ask::Start => "run-start",
                _ => "run-stop",
            };
            let key = n.key.clone();
            row = row.child(
                icon_button(sel, ask.icon(), &label, ask == Ask::Start)
                    .flex_none()
                    .on_click(cx.listener(move |this, _, _, cx| this.ask_run(&key, ask, cx))),
            );
        }
        Some(row)
    }

    /// Under the note a run is waiting on: that it needs you, and for a permission, Allow and
    /// Deny; a question is answered with a note below.
    pub(super) fn needs_you(
        &self,
        d: &Detail,
        e: &livtk_core::Event,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let r = d
            .run
            .as_ref()
            .filter(|r| r.needs_you() && r.last_id == e.id)?;
        let mut block = div()
            .debug_selector(|| "needs-you".into())
            .ml(px(20.))
            .flex()
            .items_center()
            .gap_2()
            .child(icon_tag(Icon::Inbox, "Needs you", ACCENT));
        if r.asks_to_run().is_empty() {
            block = block.child(
                div()
                    .text_xs()
                    .text_color(color(TEXT_3))
                    .child("Answer with a note below."),
            );
        } else {
            for (ask, sel, label, main) in [
                (Ask::Deny, "run-deny", "Deny", false),
                (Ask::Allow, "run-allow", "Allow once", false),
                (Ask::AllowAlways, "run-allow-always", "Allow always", true),
            ] {
                let key = d.node.key.clone();
                block = block.child(
                    button(sel, label, main)
                        .on_click(cx.listener(move |this, _, _, cx| this.ask_run(&key, ask, cx))),
                );
            }
        }
        Some(block)
    }

    /// Answer the permission the run of the task acted on waits on with `ask`, as its button
    /// does: whether there was one to answer. A run waiting on a question is left to a note.
    pub(super) fn answer_acted_on(&mut self, ask: Ask, cx: &mut Context<Self>) -> bool {
        let Some(key) = self
            .model
            .acted_on()
            .map(|r| r.key.clone())
            .filter(|k| self.model.run_asks_permission(k))
        else {
            return false;
        };
        self.ask_run(&key, ask, cx);
        true
    }

    /// Do `ask` to task `key`'s run, off the window's thread: the run's own process starts or
    /// stops there, and its notes come back through the log.
    fn ask_run(&mut self, key: &str, ask: Ask, cx: &mut Context<Self>) {
        // One thing asked of a task's run at a time: a second click waits for the first.
        if self.asked.contains_key(key) {
            return;
        }
        let (Some(root), Some(r)) = (self.model.project_root(), runner()) else {
            self.model
                .warn("The claude command is not installed, or not where livtk looks.");
            cx.notify();
            return;
        };
        self.asked.insert(key.to_owned(), ask);
        let files = crate::agents::agents(&root);
        let job_key = key.to_owned();
        let done = cx.background_executor().spawn(async move {
            let mut ws = livtk_core::Workspace::open(&root).map_err(|e| e.to_string())?;
            let human = livtk_core::Actor::Human;
            match ask {
                Ask::Start => r
                    .start(&mut ws, &job_key, None, None, &files, &human)
                    .map(|_| ()),
                Ask::Stop => r.stop(
                    &mut ws,
                    &job_key,
                    livtk_core::event::Ending::Stopped,
                    &human,
                ),
                Ask::Allow => r.answer(&mut ws, &job_key, true, false, &human),
                Ask::AllowAlways => r.answer(&mut ws, &job_key, true, true, &human),
                Ask::Deny => r.answer(&mut ws, &job_key, false, false, &human),
            }
            .map_err(|e| e.to_string())
        });
        let key = key.to_owned();
        cx.spawn(async move |this, cx| {
            let done = done.await;
            let _ = this.update(cx, |app, cx| {
                app.asked.remove(&key);
                match done {
                    Ok(()) => app.model.run_answered(&key),
                    Err(e) => app.model.warn(format!("{key}: {e}")),
                }
                app.model.survey_runs_soon();
                app.model.tick();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
