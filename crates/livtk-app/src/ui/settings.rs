//! Settings, as claude.ai and Linear keep them: opened, the sidebar becomes the list of sections
//! (and the way back to the app), the middle the section's settings. Two kinds, kept apart:
//! yours (this app's, private, in app.toml) and this project's (.livtk/config.toml, committed and
//! shared with the team), which are written through the core, checked, comments and all.

use gpui_component::input::{InputEvent, InputState};
use gpui_kit::*;

use super::form::{button, choice, field, segmented, toggle};
use super::icons::{Icon, icon, icon_hover};
use super::theme::*;
use super::{Close, LivtkApp, NAV_ROW, NAV_TEXT, hint, nav_item, section_label};
use crate::model::prefs::{Place, SWITCHES};
use crate::model::{DetailMode, ReadingSize};
use livtk_core::{HOOK_NAMES, Kind, PROMPT_SECTIONS, SESSION_SECTIONS};

/// The key context of the settings page: Esc leaves it.
pub(super) const SETTINGS: &str = "Settings";

/// A section of the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    Appearance,
    Detail,
    Updates,
    Shortcuts,
    General,
    Claude,
    Kinds,
    Gates,
    Runs,
    Export,
}

impl Section {
    /// Yours, then this project's.
    const YOU: [Section; 4] = [
        Section::Appearance,
        Section::Detail,
        Section::Updates,
        Section::Shortcuts,
    ];
    const PROJECT: [Section; 6] = [
        Section::General,
        Section::Claude,
        Section::Kinds,
        Section::Gates,
        Section::Runs,
        Section::Export,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Detail => "Opening work",
            Section::Updates => "Updates",
            Section::Shortcuts => "Shortcuts",
            Section::General => "General",
            Section::Claude => "What Claude is told",
            Section::Kinds => "Kinds and relations",
            Section::Gates => "Your gates",
            Section::Runs => "Runs",
            Section::Export => "Export",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Section::Appearance => Icon::ViewOptions,
            Section::Detail => Icon::PanelRight,
            Section::Updates => Icon::Meteor,
            Section::Shortcuts => Icon::Shortcuts,
            Section::General => Icon::Project,
            Section::Claude => Icon::Claude,
            Section::Kinds => Icon::Grouping,
            Section::Gates => Icon::You,
            Section::Runs => Icon::Agents,
            Section::Export => Icon::Article,
        }
    }

    fn id(self) -> String {
        format!("settings-{}", self.title().to_lowercase().replace(' ', "-"))
    }
}

/// What the settings keep between frames: the section open, the fields, what went wrong.
pub(super) struct Settings {
    pub open: Option<Section>,
    pub error: Option<String>,
    pub done: Option<String>,
    pub gate: Entity<InputState>,
    pub docs: Entity<InputState>,
}

impl Settings {
    pub fn new(window: &mut Window, cx: &mut Context<LivtkApp>) -> Settings {
        Settings {
            open: None,
            error: None,
            done: None,
            gate: cx.new(|cx| InputState::new(window, cx).placeholder("task: review -> done")),
            docs: cx.new(|cx| InputState::new(window, cx).placeholder("docs")),
        }
    }
}

impl LivtkApp {
    /// Open the settings on `section`, with the keys.
    pub(super) fn open_settings(
        &mut self,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_overlays(window, cx);
        self.settings.open = Some(section);
        self.settings.error = None;
        self.settings.done = None;
        let docs = self
            .model
            .config()
            .and_then(|c| c.docs.clone())
            .unwrap_or_default();
        self.settings
            .docs
            .update(cx, |s, cx| s.set_value(docs, window, cx));
        self.nav.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.open = None;
        self.focus_main(window, cx);
        cx.notify();
    }

    /// A project setting written, or why it was not.
    fn project_edit(
        &mut self,
        edit: impl FnOnce(&mut livtk_core::Workspace) -> livtk_core::Result<()>,
        cx: &mut Context<Self>,
    ) -> bool {
        let done = self.model.edit_project(edit);
        self.settings.done = None;
        self.settings.error = done.as_ref().err().cloned();
        cx.notify();
        done.is_ok()
    }

    /// The sidebar while the settings are open: the way back, then the sections, yours and
    /// this project's.
    pub(super) fn settings_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.settings.open;
        let project = self
            .model
            .project()
            .map_or("This project".to_owned(), |p| p.name.clone());
        let mut side = div()
            .debug_selector(|| "settings-sidebar".into())
            .w(px(self.sidebar_width))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .px_2()
            .pb_3()
            .pt_1()
            .gap(px(2.))
            .child(
                div()
                    .id("settings-back")
                    .debug_selector(|| "settings-back".into())
                    .group("settings-back")
                    .px_2()
                    .h(px(NAV_ROW))
                    .text_size(px(NAV_TEXT))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_lg()
                    .text_color(color(TEXT_3))
                    .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT)))
                    .child(icon_hover(
                        Icon::ArrowLeft,
                        16.,
                        TEXT_4,
                        "settings-back",
                        TEXT,
                    ))
                    .child(div().flex_1().child("Back to app"))
                    .children(hint("Esc", cx))
                    .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx))),
            )
            .child(div().h(px(14.)))
            .child(section_label("You"));
        for (i, s) in Section::YOU.into_iter().chain(Section::PROJECT).enumerate() {
            if i == Section::YOU.len() {
                side = side.child(div().h(px(14.))).child(section_label(&project));
            }
            side = side.child(
                nav_item(
                    SharedString::from(s.id()),
                    s.icon(),
                    s.title(),
                    open == Some(s),
                    0,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_settings(s, window, cx);
                })),
            );
        }
        side.into_any_element()
    }

    /// The section open, in the middle: a bar with its name, then its settings, each a line of
    /// what it is and what it does, its control at the right.
    pub(super) fn settings_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(section) = self.settings.open else {
            return div().into_any_element();
        };
        let shared = Section::PROJECT.contains(&section);
        let body = match section {
            Section::Appearance => self.appearance(cx),
            Section::Detail => self.detail_section(cx),
            Section::Updates => self.updates(cx),
            Section::Shortcuts => self.shortcuts_section(cx),
            Section::General => self.general(cx),
            Section::Claude => self.claude(cx),
            Section::Kinds => self.kinds(cx),
            Section::Gates => self.gates(cx),
            Section::Runs => self.runs(cx),
            Section::Export => self.export_section(cx),
        };
        let note = if shared {
            "Kept in .livtk/config.toml: committed with the project and shared with the team."
        } else {
            "Yours alone: kept on this machine."
        };
        div()
            .debug_selector(|| "settings-page".into())
            .key_context(SETTINGS)
            .track_focus(&self.nav)
            .on_action(cx.listener(|this, _: &Close, window, cx| this.close_settings(window, cx)))
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
                    .child(icon(section.icon(), 15., TEXT_4))
                    .child(
                        div()
                            .flex_1()
                            .text_color(color(TEXT_2))
                            .child(section.title()),
                    ),
            )
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(720.))
                            .px_6()
                            .py_6()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_xs().text_color(color(TEXT_4)).pb_2().child(note))
                            .children(self.settings.error.clone().map(|e| {
                                div()
                                    .debug_selector(|| "settings-error".into())
                                    .p_3()
                                    .rounded_lg()
                                    .bg(with_alpha(RED, 0.08))
                                    .border_1()
                                    .border_color(with_alpha(RED, 0.3))
                                    .text_color(color(TEXT_2))
                                    .child(e)
                            }))
                            .children(self.settings.done.clone().map(|d| {
                                div()
                                    .debug_selector(|| "settings-done".into())
                                    .text_color(color(TEXT_3))
                                    .child(d)
                            }))
                            .child(body),
                    ),
            )
            .into_any_element()
    }

    fn appearance(&self, cx: &mut Context<Self>) -> Div {
        let reading = self.model.reading;
        let mut sizes = segmented();
        for size in ReadingSize::ALL {
            sizes = sizes.child(
                choice(
                    format!("reading-size-{}", size.name().to_lowercase()),
                    size.name(),
                    reading.size == size,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.model.reading.size = size;
                    cx.notify();
                })),
            );
        }
        let mut density = segmented();
        for (compact, label) in [(false, "Comfortable"), (true, "Compact")] {
            density = density.child(
                choice(
                    format!("reading-density-{}", label.to_lowercase()),
                    label,
                    reading.compact == compact,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.model.reading.compact = compact;
                    cx.notify();
                })),
            );
        }
        group()
            .child(row("Text size", "How large a doc's text reads.", sizes))
            .child(row("Density", "How close a doc's lines sit.", density))
            .child(row(
                "Motion",
                "Follows macOS: with Reduce Motion on, nothing slides or fades.",
                div(),
            ))
            .children(self.switches(Place::Appearance, cx))
    }

    fn detail_section(&self, cx: &mut Context<Self>) -> Div {
        let mut modes = segmented();
        for mode in DetailMode::ALL {
            modes = modes.child(
                choice(
                    format!(
                        "settings-detail-{}",
                        mode.name().to_lowercase().replace(' ', "-")
                    ),
                    mode.name(),
                    self.model.detail_mode == mode,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.set_detail_mode(mode, window, cx);
                    this.nav.focus(window, cx);
                })),
            );
        }
        group()
            .child(row(
                "Open a task as",
                "Beside the list, in a card over the window, or in the list's place.",
                modes,
            ))
            .children(self.switches(Place::Detail, cx))
    }

    fn updates(&self, cx: &mut Context<Self>) -> Div {
        let staged = self.update.as_ref().map(|u| u.manifest.build);
        let action = match staged {
            Some(build) => button(
                "settings-restart",
                &format!("Restart to build {build}"),
                true,
            )
            .on_click(cx.listener(|this, _, _, cx| this.restart_to_update(cx))),
            None => button("settings-check", "Check now", false)
                .on_click(cx.listener(|this, _, _, cx| this.check_updates(true, cx))),
        };
        group().child(row("This build", livtk_build::DESCRIBE, action))
    }

    fn shortcuts_section(&self, cx: &mut Context<Self>) -> Div {
        group().child(row(
            "Keyboard shortcuts",
            "Every key the app answers to, by what it does.",
            button("settings-shortcuts", "Show them", false).on_click(cx.listener(
                |this, _, window, cx| {
                    this.close_settings(window, cx);
                    this.set_help(true, window, cx);
                },
            )),
        ))
    }

    fn general(&self, cx: &mut Context<Self>) -> Div {
        let prefix = self
            .model
            .config()
            .map_or(String::new(), |c| c.prefix.clone());
        group()
            .child(row(
                "Key prefix",
                "What every task's key starts with. Set when the project began; keys never change.",
                div().text_color(color(TEXT_2)).child(prefix),
            ))
            .child(row(
                "Docs folder",
                "Where the project's docs are, from its folder.",
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(field(&self.settings.docs, "settings-docs-field")),
                    )
                    .child(
                        button("settings-docs-save", "Save", false).on_click(cx.listener(
                            |this, _, _, cx| {
                                let dir = this.settings.docs.read(cx).value().to_string();
                                if this.project_edit(|ws| ws.set_docs_dir(Some(&dir)), cx) {
                                    this.settings.done = Some("Saved.".into());
                                }
                            },
                        )),
                    ),
            ))
    }

    /// The switches `SWITCHES` puts in `place`, each turning one of your choices.
    fn switches(&self, place: Place, cx: &mut Context<Self>) -> Vec<Div> {
        SWITCHES
            .iter()
            .filter(|w| w.place == place)
            .map(|w| {
                let flip = w.flip;
                row(
                    w.label,
                    w.what,
                    toggle(format!("settings-{}", w.id), (w.shown)(&self.model.prefs)).on_click(
                        cx.listener(move |this, _, _, cx| {
                            flip(&mut this.model.prefs);
                            cx.notify();
                        }),
                    ),
                )
            })
            .collect()
    }

    /// How the project's runs of Claude go: on their own or not, how many builds at once,
    /// each in its own worktree or in the checkout.
    fn runs(&self, cx: &mut Context<Self>) -> Div {
        let Some(runs) = self.model.config().map(|c| c.runs.clone()) else {
            return group();
        };
        let set = |change: fn(&mut livtk_core::Runs)| {
            let mut next = runs.clone();
            change(&mut next);
            next
        };
        let auto = set(|r| r.auto = !r.auto);
        let worktree = set(|r| r.worktree = !r.worktree);
        let mut at_once = segmented();
        for n in 1..=livtk_core::BUILDS_MAX {
            let mut next = runs.clone();
            next.builds_at_once = n;
            at_once = at_once.child(
                choice(
                    format!("settings-builds-{n}"),
                    COUNTS[usize::from(n - 1)],
                    runs.builds_at_once == n,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.project_edit(|ws| ws.set_runs(&next), cx);
                })),
            );
        }
        group()
            .child(row(
                "Start on their own",
                "An acknowledged task given to an agent starts building, and a task in review \
                 starts its review, without pressing Start.",
                toggle("settings-runs-auto".into(), runs.auto).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.project_edit(|ws| ws.set_runs(&auto), cx);
                    },
                )),
            ))
            .child(row(
                "Builds at once",
                "How many build together when they start on their own; the rest wait their turn.",
                at_once,
            ))
            .child(row(
                "Each build in its own worktree",
                "On its own branch, beside the checkout, merged when you mark the task done. \
                 Off, builds work in the checkout itself.",
                toggle("settings-runs-worktree".into(), runs.worktree).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.project_edit(|ws| ws.set_runs(&worktree), cx);
                    },
                )),
            ))
    }

    fn claude(&self, cx: &mut Context<Self>) -> Div {
        let Some(config) = self.model.config().cloned() else {
            return group();
        };
        let mut hooks = group().child(sub("Hooks livtk runs in Claude Code"));
        for name in HOOK_NAMES {
            let on = config.hook_on(name);
            let (label, what) = match name {
                "session-start" => (
                    "At session start",
                    "A brief of the work, the gates, the rules and how to work.",
                ),
                "user-prompt-submit" => (
                    "Before each prompt",
                    "The memory recalled for it, and a nudge to propose a lesson.",
                ),
                "subagent-start" => (
                    "When a subagent starts",
                    "A shorter brief of the work in progress and the rules.",
                ),
                "stop" => (
                    "When a run as the implementer stops",
                    "The same, for a session started as the implementer: a build run.",
                ),
                _ => (
                    "When the implementer stops",
                    "Refused while work it did is unrecorded.",
                ),
            };
            let id = format!("settings-hook-{name}");
            hooks = hooks.child(row(
                label,
                what,
                toggle(id, on).on_click(cx.listener(move |this, _, _, cx| {
                    this.project_edit(|ws| ws.set_hook(name, !on), cx);
                })),
            ));
        }
        let mut brief = group().child(sub("What the session-start brief says"));
        for section in SESSION_SECTIONS {
            let on = config.session_start.has(section);
            let what = match section {
                "intro" => "That the project uses livtk, and its skills.",
                "tasks" => "The work in progress, in review, waiting for you, and next.",
                "gates" => "The moves only you may make.",
                "protocol" => "How to work: plan, wait for your acknowledgement, record.",
                "projects" => "The active projects: how far each has come, and what waits on you.",
                _ => "The standing rules you ratified.",
            };
            let id = format!("settings-session-{section}");
            brief = brief.child(row(
                &capital(section),
                what,
                toggle(id, on).on_click(cx.listener(move |this, _, _, cx| {
                    this.project_edit(|ws| ws.set_section("session_start", section, !on), cx);
                })),
            ));
        }
        let mut prompt = group().child(sub("What is added before each prompt"));
        for section in PROMPT_SECTIONS {
            let on = config.user_prompt_submit.has(section);
            let what = if section == "memory" {
                "The rules, decisions and findings relevant to the prompt."
            } else {
                "When a prompt reads as a correction, a nudge to propose a lesson."
            };
            let id = format!("settings-prompt-{section}");
            prompt = prompt.child(row(
                &capital(section),
                what,
                toggle(id, on).on_click(cx.listener(move |this, _, _, cx| {
                    this.project_edit(|ws| ws.set_section("user_prompt_submit", section, !on), cx);
                })),
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(hooks)
            .child(brief)
            .child(prompt)
    }

    fn kinds(&self, cx: &mut Context<Self>) -> Div {
        let Some(config) = self.model.config().cloned() else {
            return group();
        };
        let line = |name: String, states: String, extra: String| {
            div()
                .py_2()
                .flex()
                .flex_col()
                .gap_1()
                .border_b_1()
                .border_color(color(BORDER))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(div().text_color(color(TEXT)).child(name))
                        .child(div().text_xs().text_color(color(TEXT_4)).child(extra)),
                )
                .child(div().text_xs().text_color(color(TEXT_3)).child(states))
        };
        let mut kinds = group().child(sub("Kinds"));
        for k in Kind::ALL {
            let states: Vec<&str> = k.states().iter().map(|s| s.as_str()).collect();
            kinds = kinds.child(line(
                capital(k.as_str()),
                states.join(" · "),
                "built in".into(),
            ));
        }
        for k in &config.kinds {
            kinds = kinds.child(line(
                capital(&k.name),
                k.states.join(" · "),
                format!("{}-…, declared", k.key),
            ));
        }
        let mut rels = group().child(sub("Relations"));
        for r in [
            "waits for · blocks",
            "part of · has part",
            "relates",
            "supersedes · replaced by",
            "learned from · taught",
        ] {
            rels = rels.child(line(capital(r), String::new(), "built in".into()));
        }
        for r in &config.relations {
            let name = match &r.inverse {
                Some(i) => format!("{} · {i}", r.name),
                None => r.name.clone(),
            };
            rels = rels.child(line(
                name.replace('_', " "),
                String::new(),
                "declared".into(),
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(row(
                "Declare your own",
                "A project's own kinds and relations are written in its config ([[kinds]], [[relations]]); the README shows how.",
                button("settings-open-config", "Open config.toml", false).on_click(cx.listener(
                    |this, _, _, cx| this.open_in_editor(".livtk/config.toml", cx),
                )),
            ))
            .child(kinds)
            .child(rels)
    }

    fn gates(&self, cx: &mut Context<Self>) -> Div {
        let gates = self
            .model
            .config()
            .map(|c| c.gates.human.clone())
            .unwrap_or_default();
        let mut list = group().child(sub(
            "Acknowledging a plan and deciding what binds are always yours. These moves are too.",
        ));
        for (i, g) in gates.iter().enumerate() {
            let id = format!("settings-gate-{i}");
            let rest: Vec<String> = gates
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, g)| g.clone())
                .collect();
            list = list.child(
                div()
                    .h(px(36.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(color(BORDER))
                    .child(div().flex_1().text_color(color(TEXT_2)).child(g.clone()))
                    .child(
                        div()
                            .id(SharedString::from(id.clone()))
                            .debug_selector(move || id.clone())
                            .group("gate-remove")
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .hover(|h| h.bg(color(HOVER)))
                            .child(icon_hover(Icon::Close, 13., TEXT_4, "gate-remove", TEXT))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.project_edit(|ws| ws.set_gates(&rest), cx);
                            })),
                    ),
            );
        }
        list.child(
            div()
                .pt_3()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(260.))
                        .child(field(&self.settings.gate, "settings-gate-field")),
                )
                .child(
                    button("settings-gate-add", "Add", false)
                        .on_click(cx.listener(|this, _, window, cx| this.add_gate(window, cx))),
                ),
        )
    }

    /// The gate typed, added to the project's, checked.
    pub(super) fn add_gate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entry = self.settings.gate.read(cx).value().trim().to_owned();
        if entry.is_empty() {
            return;
        }
        let mut gates = self
            .model
            .config()
            .map(|c| c.gates.human.clone())
            .unwrap_or_default();
        gates.push(entry);
        if self.project_edit(|ws| ws.set_gates(&gates), cx) {
            self.settings
                .gate
                .update(cx, |s, cx| s.set_value("", window, cx));
        }
    }

    /// Type `entry` in the gates' field and add it, as Enter does: for tests.
    #[doc(hidden)]
    pub fn type_gate(&mut self, entry: &str, cx: &mut Context<Self>) {
        let entry = entry.to_owned();
        let mut gates = self
            .model
            .config()
            .map(|c| c.gates.human.clone())
            .unwrap_or_default();
        gates.push(entry);
        self.project_edit(|ws| ws.set_gates(&gates), cx);
    }

    pub(super) fn on_gate_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let InputEvent::PressEnter { .. } = event {
            self.add_gate(window, cx);
        }
    }

    fn export_section(&self, cx: &mut Context<Self>) -> Div {
        group().child(row(
            "Markdown mirror",
            "Every node as a markdown file in .livtk/export, kept out of git: a copy for grep and diff, never read back.",
            button("settings-export", "Export now", false).on_click(cx.listener(|this, _, _, cx| {
                let done = this.model.export();
                match done {
                    Some(Ok(r)) => {
                        this.settings.error = None;
                        this.settings.done = Some(format!(
                            "Exported {} nodes to {} ({} files written).",
                            r.nodes,
                            r.dir.display(),
                            r.stale
                        ));
                    }
                    Some(Err(e)) => this.settings.error = Some(e.to_string()),
                    None => {}
                }
                cx.notify();
            })),
        ))
    }
}

/// A group of settings.
fn group() -> Div {
    div().flex().flex_col()
}

/// A group's heading.
fn sub(text: &str) -> Div {
    div()
        .pt_2()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color(TEXT_4))
        .child(text.to_owned())
}

/// A setting: what it is and what it does, its control at the right.
fn row(label: &str, what: &str, control: impl IntoElement) -> Div {
    div()
        .min_h(px(56.))
        .py_2()
        .flex()
        .items_center()
        .gap_4()
        .border_b_1()
        .border_color(color(BORDER))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_color(color(TEXT)).child(label.to_owned()))
                .child(
                    div()
                        .text_xs()
                        .text_color(color(TEXT_3))
                        .child(what.to_owned()),
                ),
        )
        .child(div().flex_none().child(control))
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// The counts a choice of how many offers, as its chips say them: one for each a project may
/// build at once.
const COUNTS: [&str; 8] = ["1", "2", "3", "4", "5", "6", "7", "8"];
const _: () = assert!(COUNTS.len() == livtk_core::BUILDS_MAX as usize);
