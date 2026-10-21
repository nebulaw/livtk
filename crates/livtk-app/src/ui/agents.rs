//! The Agents place: in the middle, the Claude sessions working in the project (Needs you
//! first) and the agents they can run as; in the panel, the one picked: a session with what it
//! did, or an agent's page to change, copy or remove it.

use std::path::PathBuf;
use std::time::Duration;

use gpui_component::Sizable;
use gpui_component::input::{Input, InputState};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::icons::{Icon, icon};
use super::theme::*;
use super::{LivtkApp, ago, event_line, section_label};
use crate::agents::{self, Agent, MODELS, Place, Session, Standing, TOOLS};
use crate::model::View;
use crate::model::agents::Pick;

/// How often the sessions are asked for while the place is open.
const POLL: Duration = Duration::from_secs(3);

/// The agent page's fields, and which pick they were filled from.
pub(super) struct AgentForm {
    name: Entity<InputState>,
    description: Entity<InputState>,
    body: Entity<TextareaState>,
    tools: Vec<String>,
    model: Option<String>,
    /// The file's other keys, kept as they were.
    extra: String,
    place: Place,
    loaded: Option<Pick>,
    /// Sessions are being asked for.
    pub polling: bool,
}

impl AgentForm {
    pub fn new(window: &mut Window, cx: &mut Context<LivtkApp>) -> AgentForm {
        AgentForm {
            name: cx.new(|cx| InputState::new(window, cx).placeholder("code-reviewer")),
            description: cx.new(|cx| {
                InputState::new(window, cx).placeholder("When Claude should use it, in a sentence")
            }),
            body: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("Its instructions…")
                    .auto_grow(8, 30)
            }),
            tools: Vec::new(),
            model: None,
            extra: String::new(),
            place: Place::Project,
            loaded: None,
            polling: false,
        }
    }
}

impl LivtkApp {
    /// Whether the panel shows the Agents place's pick.
    pub(super) fn agents_panel_open(&self) -> bool {
        self.model.view() == View::Agents && self.model.agents.open.is_some()
    }

    /// Ask Claude Code for the sessions, off the window's thread, again every few seconds while
    /// the place is shown.
    fn poll_sessions(&mut self, cx: &mut Context<Self>) {
        if self.agent_form.polling {
            return;
        }
        let Some(root) = self.model.project_root() else {
            return;
        };
        self.agent_form.polling = true;
        let read = cx.background_spawn(async move { agents::sessions(&root) });
        cx.spawn(async move |this, cx| {
            let found = read.await;
            let _ = this.update(cx, |app, cx| {
                app.model.set_sessions(found);
                cx.notify();
            });
            cx.background_executor().timer(POLL).await;
            let _ = this.update(cx, |app, cx| {
                app.agent_form.polling = false;
                if app.model.view() == View::Agents {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Fill the agent page from what was picked, once per pick.
    fn sync_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.model.agents.open.clone();
        if open == self.agent_form.loaded {
            return;
        }
        let (agent, place) = match &open {
            Some(Pick::New(a)) => (a.clone(), Place::Project),
            Some(Pick::Agent(_)) => match self.model.picked_agent() {
                Some(f) => (f.agent.clone(), f.place),
                None => return,
            },
            _ => {
                self.agent_form.loaded = open;
                return;
            }
        };
        let f = &mut self.agent_form;
        f.name
            .update(cx, |s, cx| s.set_value(agent.name.clone(), window, cx));
        f.description.update(cx, |s, cx| {
            s.set_value(agent.description.clone(), window, cx)
        });
        f.body
            .update(cx, |s, cx| s.set_value(agent.body.clone(), window, cx));
        f.tools = agent.tools;
        f.model = agent.model;
        f.extra = agent.extra;
        f.place = place;
        f.loaded = open;
    }

    /// The agent the page's fields say.
    fn form_agent(&self, cx: &App) -> Agent {
        let f = &self.agent_form;
        Agent {
            name: f.name.read(cx).value().trim().to_owned(),
            description: f.description.read(cx).value().trim().to_owned(),
            tools: f.tools.clone(),
            model: f.model.clone(),
            body: f.body.read(cx).value().to_string(),
            extra: f.extra.clone(),
        }
    }

    /// The middle: the sessions, then the agents by place.
    pub(super) fn agents_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.poll_sessions(cx);
        self.sync_form(window, cx);
        let open = self.model.agents.open.clone();
        let mut list = div()
            .id("agents-place")
            .debug_selector(|| "agents-place".into())
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        // Sessions.
        let sessions = self.model.agents.sessions.clone();
        list = list.child(band(
            "Sessions",
            sessions.as_ref().map_or(0, Vec::len),
            None,
        ));
        if let Some(e) = &self.model.agents.sessions_error {
            list = list.child(quiet_line("sessions-error", e));
        }
        match &sessions {
            None if self.model.agents.sessions_error.is_none() => {
                list = list.child(quiet_line("sessions-reading", "Asking Claude Code…"));
            }
            Some(s) if s.is_empty() => {
                list = list.child(quiet_line(
                    "sessions-none",
                    "No Claude session is working in this project.",
                ));
            }
            _ => {}
        }
        for s in sessions.iter().flatten() {
            let pick = Pick::Session(s.key());
            let on = open.as_ref() == Some(&pick);
            list = list.child(self.session_row(s, on, pick, cx));
        }
        // Agents, by place.
        let new = div()
            .id("agent-new")
            .debug_selector(|| "agent-new".into())
            .h(px(24.))
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .rounded_lg()
            .text_xs()
            .text_color(color(TEXT_3))
            .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
            .child(icon(Icon::ZoomIn, 12., TEXT_3))
            .child("New agent")
            .on_click(cx.listener(|this, _, _, cx| {
                this.model.pick_agent(Some(Pick::New(Agent::default())));
                cx.notify();
            }));
        list = list.child(band(
            "Agents",
            self.model.agents.files.len(),
            Some(new.into_any_element()),
        ));
        for place in Place::ALL {
            let files: Vec<_> = self
                .model
                .agents
                .files
                .iter()
                .filter(|f| f.place == place)
                .cloned()
                .collect();
            if files.is_empty() {
                continue;
            }
            list = list.child(
                div()
                    .px_4()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(place.label()),
            );
            for f in files {
                let pick = Pick::Agent(f.path.clone());
                let on = open.as_ref() == Some(&pick);
                let id = format!("agent-{}-{:?}", f.agent.name, f.place).to_lowercase();
                list = list.child(
                    div()
                        .id(SharedString::from(id.clone()))
                        .debug_selector(move || id.clone())
                        .h(px(ROW))
                        .flex_none()
                        .px_4()
                        .flex()
                        .items_center()
                        .gap_3()
                        .border_b_1()
                        .border_color(color(CONTENT))
                        .when(on, |d| d.bg(color(SELECTED)))
                        .when(!on, |d| d.hover(|h| h.bg(color(HOVER))))
                        .child(icon(Icon::Agents, 14., TEXT_4))
                        .child(
                            div()
                                .w(px(160.))
                                .flex_none()
                                .truncate()
                                .text_color(color(TEXT))
                                .child(f.agent.name.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(color(TEXT_3))
                                .child(f.agent.description.clone()),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.model.pick_agent(Some(pick.clone()));
                            cx.notify();
                        })),
                );
            }
        }
        list.into_any_element()
    }

    fn session_row(&self, s: &Session, on: bool, pick: Pick, cx: &mut Context<Self>) -> AnyElement {
        let key = s.key();
        let standing = s.standing();
        let started = jiff::Timestamp::from_millisecond(s.started_at)
            .map(|t| ago(&t.to_string()))
            .unwrap_or_default();
        div()
            .id(SharedString::from(format!("session-{key}")))
            .debug_selector(move || format!("session-{key}"))
            .h(px(ROW))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(color(CONTENT))
            .when(on, |d| d.bg(color(SELECTED)))
            .when(!on, |d| d.hover(|h| h.bg(color(HOVER))))
            .child(standing_tag(standing))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(color(TEXT))
                    .child(s.title()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(if s.is_background() {
                        "background"
                    } else {
                        "interactive"
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
                    .child(started),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.pick_agent(Some(pick.clone()));
                cx.notify();
            }))
            .into_any_element()
    }

    /// The panel: the session or the agent picked.
    pub(super) fn agents_panel(
        &mut self,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.agents_panel_open() {
            return None;
        }
        let body = match self.model.agents.open.clone()? {
            Pick::Session(_) => self.session_page(cx)?,
            Pick::Agent(_) | Pick::New(_) => self.agent_page(cx),
        };
        Some(
            div()
                .debug_selector(|| "detail-panel".into())
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
                        .border_b_1()
                        .border_color(color(BORDER))
                        .child(div().flex_1().text_color(color(TEXT_3)).child("Agents"))
                        .child(
                            div()
                                .id("close-agent")
                                .debug_selector(|| "close-agent".into())
                                .size(px(26.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_lg()
                                .hover(|s| s.bg(color(HOVER)))
                                .child(icon(Icon::Close, 16., TEXT_4))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.model.pick_agent(None);
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .id("agent-scroll")
                        .flex_1()
                        .overflow_y_scroll()
                        .p_5()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(body),
                )
                .into_any_element(),
        )
    }

    fn session_page(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let s = self.model.picked_session()?.clone();
        let mut page = div()
            .debug_selector(|| "session-page".into())
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_lg().text_color(color(TEXT)).child(s.title()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(standing_tag(s.standing()))
                    .child(div().text_xs().text_color(color(TEXT_3)).child(format!(
                        "{} · {}",
                        if s.is_background() {
                            "background"
                        } else {
                            "interactive"
                        },
                        s.cwd.display()
                    ))),
            );
        let mut actions = div().flex().gap_2();
        if let Some(id) = s.session_id.clone() {
            actions = actions.child(
                action_button("session-open", "Open in the terminal", true).on_click(cx.listener(
                    move |this, _, window, cx| {
                        // The one livtk found: the Finder's PATH is short.
                        let claude = agents::claude()
                            .map_or_else(|| "claude".to_owned(), |p| p.display().to_string());
                        this.open_program(
                            &claude,
                            vec!["--resume".to_owned(), id.clone()],
                            window,
                            cx,
                        );
                    },
                )),
            );
        }
        if s.is_background()
            && let Some(id) = s.id.clone()
        {
            actions = actions.child(action_button("session-stop", "Stop", false).on_click(
                cx.listener(move |this, _, _, cx| {
                    match agents::stop(&id) {
                        Ok(()) => this.model.inform("Stopped."),
                        Err(e) => this.model.warn(e),
                    }
                    cx.notify();
                }),
            ));
        }
        page = page.child(actions);
        let seen = self.model.session_activity(&s);
        page = page.child(section_label("What it did in livtk"));
        if seen.is_empty() {
            page = page.child(
                div()
                    .text_sm()
                    .text_color(color(TEXT_3))
                    .child("Nothing yet since it started."),
            );
        }
        for a in &seen {
            let key = a.node.as_ref().map(|n| n.key.clone()).unwrap_or_default();
            page = page.child(event_line(&a.event, &key));
        }
        Some(page)
    }

    fn agent_page(&mut self, cx: &mut Context<Self>) -> Div {
        let picked = self.model.picked_agent().cloned();
        let new = picked.is_none();
        let editable = picked.as_ref().is_none_or(|f| f.place.editable());
        let f = &self.agent_form;
        let mut page = div()
            .debug_selector(|| "agent-page".into())
            .flex()
            .flex_col()
            .gap_3();
        page = page.child(
            div()
                .text_lg()
                .text_color(color(TEXT))
                .child(match &picked {
                    Some(p) => p.agent.name.clone(),
                    None => "New agent".to_owned(),
                }),
        );
        if !editable {
            page = page.child(
                div()
                    .text_sm()
                    .text_color(color(TEXT_3))
                    .child("livtk's own agent: copy it to make one of your own."),
            );
        }
        // Where it lives: chosen for a new one.
        if new {
            let mut places = segmented();
            for place in [Place::Project, Place::Yours] {
                let on = f.place == place;
                let id = format!("agent-place-{:?}", place).to_lowercase();
                places = places.child(choice(id, place.label(), on).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.agent_form.place = place;
                        cx.notify();
                    },
                )));
            }
            page = page.child(labelled("Where", places));
        }
        page = page
            .child(labelled("Name", field(&f.name, "agent-name")))
            .child(labelled(
                "Description",
                field(&f.description, "agent-description"),
            ));
        // Tools: every tool, or only these.
        let all = f.tools.is_empty();
        let mut tools = div().flex().flex_wrap().gap_1().child(
            chip("agent-tools-all", "Every tool", all).on_click(cx.listener(|this, _, _, cx| {
                this.agent_form.tools.clear();
                cx.notify();
            })),
        );
        for t in TOOLS {
            let on = f.tools.iter().any(|x| x == t);
            let id = format!("agent-tool-{}", t.to_lowercase());
            tools = tools.child(
                chip(&id, t, on).on_click(cx.listener(move |this, _, _, cx| {
                    let tools = &mut this.agent_form.tools;
                    match tools.iter().position(|x| x == t) {
                        Some(i) => {
                            tools.remove(i);
                        }
                        None => tools.push(t.to_owned()),
                    }
                    cx.notify();
                })),
            );
        }
        page = page.child(labelled("Tools", tools));
        let mut models = segmented().child(
            choice(
                "agent-model-session".into(),
                "The session's",
                f.model.is_none(),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.agent_form.model = None;
                cx.notify();
            })),
        );
        for m in MODELS {
            let on = f.model.as_deref() == Some(m);
            models = models.child(
                choice(format!("agent-model-{m}"), m, on).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.agent_form.model = Some(m.to_owned());
                        cx.notify();
                    },
                )),
            );
        }
        page = page.child(labelled("Model", models));
        page = page.child(labelled(
            "Instructions",
            div()
                .debug_selector(|| "agent-body".into())
                .w_full()
                .rounded_lg()
                .bg(color(CONTENT))
                .border_1()
                .border_color(color(BORDER_STRONG))
                .p_2()
                .child(Textarea::new(&f.body).xsmall()),
        ));
        // What can be done with it.
        let mut actions = div().flex().gap_2().pt_2();
        if editable {
            let replacing: Option<PathBuf> = picked.as_ref().map(|p| p.path.clone());
            let place = picked.as_ref().map_or(f.place, |p| p.place);
            actions = actions.child(action_button("agent-save", "Save", true).on_click(
                cx.listener(move |this, _, _, cx| {
                    let agent = this.form_agent(cx);
                    this.model.save_agent(place, &agent, replacing.clone());
                    cx.notify();
                }),
            ));
        }
        if let Some(p) = &picked {
            let mut copy = p.agent.clone();
            copy.name = format!("{}-copy", copy.name);
            actions = actions.child(action_button("agent-copy", "Copy", false).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.model.pick_agent(Some(Pick::New(copy.clone())));
                    cx.notify();
                }),
            ));
            if p.place.editable() {
                let label = if self.model.agents.confirm_remove {
                    "Remove: press again"
                } else {
                    "Remove"
                };
                actions = actions.child(action_button("agent-remove", label, false).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.model.remove_agent();
                        cx.notify();
                    }),
                ));
            }
        }
        page.child(actions)
    }
}

/// A Sessions or Agents band: its name, its count, and what it offers at the right.
fn band(name: &'static str, count: usize, end: Option<AnyElement>) -> Div {
    div()
        .debug_selector(move || format!("band-{}", name.to_lowercase()))
        .h(px(ROW))
        .flex_none()
        .px_4()
        .flex()
        .items_center()
        .gap_2()
        .bg(color(SURFACE))
        .text_sm()
        .text_color(color(TEXT_2))
        .child(name)
        .child(div().text_color(color(TEXT_4)).child(count.to_string()))
        .child(div().flex_1())
        .children(end)
}

fn quiet_line(id: &'static str, text: &str) -> Div {
    div()
        .debug_selector(move || id.into())
        .px_4()
        .py_2()
        .text_sm()
        .text_color(color(TEXT_3))
        .child(text.to_owned())
}

/// A session's standing as a tag, Needs you in the accent.
fn standing_tag(s: Standing) -> Div {
    let c = match s {
        Standing::NeedsYou => ACCENT,
        Standing::Working => PROGRESS,
        Standing::Idle => TEXT_4,
        Standing::Finished => DONE,
    };
    div()
        .w(px(84.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_xs()
        .text_color(color(TEXT_2))
        .child(div().size(px(8.)).rounded_full().bg(color(c)))
        .child(s.label())
}

fn labelled(label: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().text_color(color(TEXT_4)).child(label))
        .child(control)
}

fn field(state: &Entity<InputState>, id: &'static str) -> Div {
    div()
        .debug_selector(move || id.into())
        .h(px(28.))
        .px_2()
        .flex()
        .items_center()
        .rounded_lg()
        .bg(color(CONTENT))
        .border_1()
        .border_color(color(BORDER_STRONG))
        .child(
            div()
                .flex_1()
                .child(Input::new(state).appearance(false).small()),
        )
}

fn segmented() -> Div {
    div()
        .flex()
        .p(px(2.))
        .gap(px(2.))
        .rounded_lg()
        .bg(color(SURFACE))
        .border_1()
        .border_color(color(BORDER))
}

fn choice(id: String, label: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || id.clone())
        .h(px(24.))
        .px_2()
        .flex()
        .items_center()
        .rounded_md()
        .text_xs()
        .when(on, |d| d.bg(color(SELECTED)).text_color(color(TEXT)))
        .when(!on, |d| {
            d.text_color(color(TEXT_3))
                .hover(|h| h.text_color(color(TEXT_2)))
        })
        .child(label)
}

/// A tool's chip: lit while chosen.
fn chip(id: &str, label: &'static str, on: bool) -> Stateful<Div> {
    let sel = format!("{id}-{}", if on { "on" } else { "off" });
    div()
        .id(SharedString::from(id.to_owned()))
        .debug_selector(move || sel.clone())
        .h(px(22.))
        .px_2()
        .flex()
        .items_center()
        .rounded_full()
        .text_xs()
        .border_1()
        .when(on, |d| {
            d.bg(color(SELECTED))
                .border_color(color(ACCENT))
                .text_color(color(TEXT))
        })
        .when(!on, |d| {
            d.border_color(color(BORDER_STRONG))
                .text_color(color(TEXT_3))
                .hover(|h| h.text_color(color(TEXT_2)))
        })
        .child(label)
}

fn action_button(id: &'static str, label: &str, main: bool) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .h(px(28.))
        .px_3()
        .flex()
        .items_center()
        .rounded_lg()
        .text_sm()
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
        .child(label.to_owned())
}
