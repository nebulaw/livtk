//! Agents and Sessions, two views side by side in the sidebar. Agents: the agents Claude
//! sessions can run as, each with a page to change, copy or remove it. Sessions: the project's
//! Claude sessions, past and running (Needs you first), each with what it did, to read.

use std::path::PathBuf;
use std::time::Duration;

use gpui_component::Sizable;
use gpui_component::input::InputState;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::form::{button, chip, choice, field, segmented};
use super::icons::{Icon, icon};
use super::node_ref::OpenIn;
use super::theme::*;
use super::{LivtkApp, ago, event_line, section_label};
use crate::agents::{self, Agent, MODELS, Place, Standing, TOOLS};
use crate::model::View;
use crate::model::agents::Pick;
use crate::model::agents::{PAGE, SessionRow};
use livtk_core::claude::Entry;

/// How many records before a moment a session opened at it begins: the turn that led to it.
const READ_BEFORE: usize = 6;

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
    /// The stage it suits, kept as it was.
    stage: Option<livtk_core::run::Stage>,
    place: Place,
    loaded: Option<Pick>,
    /// Sessions are being asked for.
    pub polling: bool,
    /// Where the session being read is scrolled.
    pub reader_scroll: ScrollHandle,
    /// The project whose transcripts were last asked for, outside the Sessions view's poll.
    pub transcripts_for: Option<std::path::PathBuf>,
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
            stage: None,
            place: Place::Project,
            loaded: None,
            polling: false,
            reader_scroll: ScrollHandle::new(),
            transcripts_for: None,
        }
    }
}

impl LivtkApp {
    /// Whether the panel shows this view's pick: an agent in Agents, a session in Sessions.
    pub(super) fn agents_panel_open(&self) -> bool {
        // Each view's own: a session in Sessions, an agent in Agents.
        self.model.agents_pick_in(self.model.view()).is_some()
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
        let read = cx.background_spawn(async move {
            let live = agents::sessions(&root);
            let transcripts = agents::claude_home()
                .map(|h| {
                    livtk_core::claude::transcripts(&livtk_core::claude::transcripts_in(&h, &root))
                })
                .unwrap_or_default();
            (live, transcripts)
        });
        cx.spawn(async move |this, cx| {
            let (live, transcripts) = read.await;
            let _ = this.update(cx, |app, cx| {
                app.model.set_sessions(live);
                app.model.set_transcripts(transcripts);
                cx.notify();
            });
            cx.background_executor().timer(POLL).await;
            let _ = this.update(cx, |app, cx| {
                app.agent_form.polling = false;
                if app.model.view() == View::Sessions {
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
        f.stage = agent.stage;
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
            stage: f.stage,
        }
    }

    /// The middle of Sessions: the project's sessions, past and running, Needs you first; or the
    /// one being read.
    pub(super) fn sessions_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.poll_sessions(cx);
        let open = self.model.agents.open.clone();
        let mut list = div()
            .id("sessions-place")
            .debug_selector(|| "sessions-place".into())
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        // A session being read takes the list's place.
        if self.model.agents.reader.is_some() {
            return self.reader_view(cx);
        }
        // Sessions: past and running, Needs you first.
        let rows = self.model.session_rows();
        list = list.child(band("Sessions", rows.len(), None));
        if let Some(e) = &self.model.agents.sessions_error {
            list = list.child(quiet_line("sessions-error", e));
        }
        if rows.is_empty() {
            list = list.child(quiet_line(
                "sessions-none",
                "No Claude session has worked in this project yet.",
            ));
        }
        for row in &rows {
            let pick = Pick::Session(row.id.clone());
            let on = open.as_ref() == Some(&pick);
            list = list.child(self.session_row(row, on, pick, cx));
        }
        list.into_any_element()
    }

    /// The middle of Agents: the agents by where they live.
    pub(super) fn agents_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_form(window, cx);
        let open = self.model.agents.open.clone();
        let mut list = div()
            .id("agents-place")
            .debug_selector(|| "agents-place".into())
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        // The builds waiting their turn to start on their own, first first.
        let queued = self.model.queued_here();
        if !queued.is_empty() {
            list = list.child(band("Waiting their turn", queued.len(), None));
            for r in &queued {
                list = list.child(div().px_4().py_1().child(self.node_ref(
                    format!("queued-{}", r.key),
                    &r.key,
                    livtk_core::Kind::Task,
                    livtk_core::State::Acked,
                    &r.title,
                    OpenIn::View,
                    None,
                    cx,
                )));
            }
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

    /// A session in the list: where it stands (while it runs), its name, its agents, when.
    fn session_row(
        &self,
        row: &SessionRow,
        on: bool,
        pick: Pick,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = row.id.clone();
        let when = row.ended.as_deref().map(ago).unwrap_or_default();
        let read = pick.clone();
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
            .child(match row.live {
                Some(s) => standing_tag(s),
                None => div().w(px(84.)).flex_none(),
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(color(TEXT))
                    .child(row.name.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(row_detail(row)),
            )
            .child(
                div()
                    .w(px(60.))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(when),
            )
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                // A double click reads it.
                if e.click_count() >= 2
                    && let Pick::Session(id) = &read
                {
                    this.model.read_session(id, None);
                } else {
                    this.model.pick_agent(Some(pick.clone()));
                }
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

    /// A session's panel: its name, where it stands, the tasks it worked on, and what it did in
    /// livtk; Read, and while it runs, Open and Stop.
    fn session_page(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let row = self.model.picked_row()?;
        let live = self.model.picked_session().cloned();
        let mut page = div()
            .debug_selector(|| "session-page".into())
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .text_color(color(TEXT))
                    .child(row.name.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(row.live.map(standing_tag))
                    .child(div().text_xs().text_color(color(TEXT_3)).child(format!(
                        "{} · {}{}",
                        livtk_core::format::session(&row.id),
                        row.started.as_deref().map(ago).unwrap_or_default(),
                        if row.by.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", row.by.join(", "))
                        }
                    ))),
            );
        let mut actions = div().flex().gap_2();
        if row.transcript.is_some() {
            let id = row.id.clone();
            actions = actions.child(button("session-read", "Read", true).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.model.read_session(&id, None);
                    cx.notify();
                },
            )));
        } else {
            page = page.child(
                div()
                    .debug_selector(|| "session-gone".into())
                    .text_sm()
                    .text_color(color(TEXT_3))
                    .child("Claude Code no longer keeps this session's transcript."),
            );
        }
        if let Some(id) = live.as_ref().and_then(|s| s.session_id.clone()) {
            actions = actions.child(
                button("session-open", "Open in the terminal", false).on_click(cx.listener(
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
        if let Some(id) = live.filter(|s| s.is_background()).and_then(|s| s.id) {
            actions = actions.child(button("session-stop", "Stop", false).on_click(cx.listener(
                move |this, _, _, cx| {
                    let Some(root) = this.model.project_root() else {
                        return;
                    };
                    match agents::stop(&root, &id) {
                        Ok(()) => this.model.inform("Stopped."),
                        Err(e) => this.model.warn(e),
                    }
                    cx.notify();
                },
            )));
        }
        page = page.child(actions);
        let nodes = self.model.session_nodes(&row.id);
        if !nodes.is_empty() {
            page = page.child(section_label("Worked on"));
            for b in &nodes {
                page = page.child(self.node_ref(
                    format!("session-node-{}", b.key),
                    &b.key,
                    b.kind,
                    b.state,
                    &b.title,
                    OpenIn::View,
                    None,
                    cx,
                ));
            }
        }
        let seen = self.model.session_activity(&row.id);
        page = page.child(section_label("What it did in livtk"));
        if seen.is_empty() {
            page = page.child(
                div()
                    .text_sm()
                    .text_color(color(TEXT_3))
                    .child("Nothing on record."),
            );
        }
        for (i, a) in seen.iter().enumerate() {
            let key = a.node.as_ref().map(|n| n.key.clone()).unwrap_or_default();
            let line = event_line(&a.event, &key);
            page = page.child(if key.is_empty() {
                line.into_any_element()
            } else {
                let el = div()
                    .id(SharedString::from(format!("session-did-{i}")))
                    .rounded_md()
                    .hover(|h| h.bg(color(HOVER)))
                    .child(line);
                self.goes_to(el, &key, OpenIn::View, cx).into_any_element()
            });
        }
        Some(page)
    }

    /// The session being read, in the middle: what was asked, what Claude said, each call one
    /// line until opened, its result under it; read a page at a time, from a little before the
    /// moment it was opened at, if any.
    fn reader_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.read_pages(cx);
        let Some(r) = self.model.agents.reader.as_ref() else {
            return div().into_any_element();
        };
        let name = self
            .model
            .session_rows()
            .into_iter()
            .find(|row| row.id == r.session)
            .map_or_else(|| r.session.clone(), |row| row.name);
        let results = livtk_core::claude::results(&r.entries);
        let focus = r.focus_index();
        let mut body = div()
            .id("session-reader-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.agent_form.reader_scroll)
            .px_6()
            .py_4()
            .flex()
            .flex_col()
            .gap_3();
        // Begun past the start: what came before is a press away.
        let earlier = r.begun > 0;
        if earlier {
            body = body.child(button("reader-start", "From the start", false).on_click(
                cx.listener(|this, _, _, cx| {
                    this.model.read_from_start();
                    cx.notify();
                }),
            ));
        }
        for (i, e) in r.drawn().enumerate() {
            let el = match e {
                Entry::Prompt { text, at } => said("You", text, at, ACCENT),
                Entry::Reply { text, at } => said("Claude", text, at, TEXT_3),
                Entry::Compacted { .. } => div()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child("The conversation was compacted here."),
                Entry::Result { .. } => continue,
                Entry::Tool {
                    id,
                    name,
                    summary,
                    input,
                    ..
                } => self.call(id, name, summary, input, r.open.contains(id), &results, cx),
            };
            body = body.child(
                div()
                    .when(focus == Some(i), |d| {
                        d.debug_selector(|| "reader-focus".into())
                            .border_l_2()
                            .border_color(color(ACCENT))
                            .pl_2()
                    })
                    .child(el),
            );
        }
        if r.next.is_some() {
            body = body.child(
                button(
                    "reader-more",
                    if r.loading { "Reading…" } else { "Read on" },
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.model.read_on();
                    cx.notify();
                })),
            );
        }
        // The rows drawn, the button above them counted.
        if let Some(i) = focus {
            self.agent_form
                .reader_scroll
                .scroll_to_item(i + usize::from(earlier));
        }
        div()
            .debug_selector(|| "session-reader".into())
            .size_full()
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
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(color(TEXT_2))
                            .child(name),
                    )
                    .child(button("reader-close", "Back to the list", false).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.model.close_reader();
                            cx.notify();
                        }),
                    )),
            )
            .child(body)
            .into_any_element()
    }

    /// A tool call in the reader: one line until opened; opened, its input and result, and for a
    /// call that started a subagent, its work to read.
    #[allow(clippy::too_many_arguments)]
    fn call(
        &self,
        id: &str,
        name: &str,
        summary: &str,
        input: &str,
        open: bool,
        results: &std::collections::HashMap<&str, (&str, bool)>,
        cx: &mut Context<Self>,
    ) -> Div {
        let call = id.to_owned();
        let sel = format!("call-{id}");
        let mut d = div().flex().flex_col().gap_1().child(
            div()
                .id(SharedString::from(sel.clone()))
                .debug_selector(move || sel.clone())
                .flex()
                .gap_2()
                .text_xs()
                .text_color(color(TEXT_3))
                .hover(|h| h.text_color(color(TEXT_2)))
                .child(div().text_color(color(TEXT_2)).child(name.to_owned()))
                .child(div().truncate().child(summary.to_owned()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.model.toggle_call(&call);
                    cx.notify();
                })),
        );
        if !open {
            return d;
        }
        if let Some(sub) = self.model.subagent_of(id) {
            let call = id.to_owned();
            let sel = format!("subagent-{id}");
            let label = format!(
                "Read {}'s work",
                sub.agent_type.as_deref().unwrap_or("the subagent")
            );
            d = d.child(
                div()
                    .id(SharedString::from(sel.clone()))
                    .debug_selector(move || sel.clone())
                    .text_xs()
                    .text_color(color(ACCENT))
                    .hover(|h| h.text_color(color(ACCENT_HOVER)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.read_subagent(&call);
                        cx.notify();
                    })),
            );
        }
        d = d.child(code(input));
        if let Some((text, error)) = results.get(id) {
            d = d.child(code(&clip(text)).when(*error, |c| c.text_color(color(RED))));
        }
        d
    }

    /// Read the next page of the session being read, off the window's thread, when it wants one:
    /// first finding where to begin, when it was opened at a moment.
    fn read_pages(&mut self, cx: &mut Context<Self>) {
        let Some((path, from, seek)) = self.model.wants_page() else {
            return;
        };
        let Some(session) = self.model.agents.reader.as_ref().map(|r| r.session.clone()) else {
            return;
        };
        self.model.page_asked();
        let read = cx.background_spawn(async move {
            let from = match seek {
                Some(at) => livtk_core::claude::offset_near(&path, &at, READ_BEFORE)?,
                None => from,
            };
            livtk_core::claude::read(&path, from, PAGE).map(|page| (from, page))
        });
        cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |app, cx| {
                match read {
                    Ok((from, page)) => app.model.add_page(&session, from, page),
                    Err(e) => {
                        app.model.close_reader();
                        app.model
                            .warn(format!("The transcript could not be read: {e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Have the project's transcripts read, once, off the window's thread: a task's panel names
    /// its sessions by them, before Sessions was ever opened.
    pub(super) fn load_transcripts(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.model.project_root() else {
            return;
        };
        if self.agent_form.transcripts_for.as_ref() == Some(&root) {
            return;
        }
        self.agent_form.transcripts_for = Some(root.clone());
        let read = cx.background_spawn(async move {
            agents::claude_home()
                .map(|h| {
                    livtk_core::claude::transcripts(&livtk_core::claude::transcripts_in(&h, &root))
                })
                .unwrap_or_default()
        });
        cx.spawn(async move |this, cx| {
            let transcripts = read.await;
            let _ = this.update(cx, |app, cx| {
                app.model.set_transcripts(transcripts);
                cx.notify();
            });
        })
        .detach();
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
            actions = actions.child(button("agent-save", "Save", true).on_click(cx.listener(
                move |this, _, _, cx| {
                    let agent = this.form_agent(cx);
                    this.model.save_agent(place, &agent, replacing.clone());
                    cx.notify();
                },
            )));
        }
        if let Some(p) = &picked {
            let mut copy = p.agent.clone();
            copy.name = format!("{}-copy", copy.name);
            actions = actions.child(button("agent-copy", "Copy", false).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.model.pick_agent(Some(Pick::New(copy.clone())));
                    cx.notify();
                },
            )));
            if p.place.editable() {
                let label = if self.model.agents.confirm_remove {
                    "Remove: press again"
                } else {
                    "Remove"
                };
                actions = actions.child(button("agent-remove", label, false).on_click(
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

/// What someone said in a session: who, when, and the text as written.
fn said(who: &'static str, text: &str, at: &str, c: u32) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .gap_2()
                .text_xs()
                .child(div().text_color(color(c)).child(who))
                .child(div().text_color(color(TEXT_4)).child(ago(at))),
        )
        .child(
            div()
                .text_sm()
                .text_color(color(TEXT))
                .child(text.to_owned()),
        )
}

/// A call's input or result, as written, in a quiet box.
fn code(text: &str) -> Div {
    div()
        .p_2()
        .rounded_lg()
        .bg(color(SURFACE))
        .text_xs()
        .text_color(color(TEXT_2))
        .child(text.to_owned())
}

/// A result cut to what reads in a box: the start, and how much more there was.
fn clip(text: &str) -> String {
    const MAX: usize = 4000;
    if text.len() <= MAX {
        return text.to_owned();
    }
    let cut = text
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|i| *i <= MAX)
        .last()
        .unwrap_or(0);
    format!("{}\n… {} more characters", &text[..cut], text.len() - cut)
}

/// A session row's quiet detail: who worked in it, and on how many nodes.
fn row_detail(row: &SessionRow) -> String {
    let by: Vec<&str> = row
        .by
        .iter()
        .map(|b| b.strip_prefix("claude:").unwrap_or(b))
        .collect();
    match (by.is_empty(), row.nodes) {
        (true, 0) => String::new(),
        (true, n) => format!("{n} node(s)"),
        (false, 0) => by.join(", "),
        (false, n) => format!("{} · {n} node(s)", by.join(", ")),
    }
}
