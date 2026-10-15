//! The window: a sidebar, the current view (inbox, list or board), the detail panel and the
//! new-task form. It renders the model and forwards input to it; no behaviour lives here.

mod theme;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{Input, InputEvent, InputState, TextareaState};
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use livtk_core::event::{Event, Op};
use livtk_core::{Brief, Kind, State, Summary};

use crate::model::{Choice, Layout, Model, View};
use theme::*;

actions!(
    livtk,
    [
        Down,
        Up,
        Open,
        Close,
        Decide,
        Standing,
        Reject,
        NewTask,
        ToggleLayout,
        ToggleAll,
        GoInbox,
        GoTasks,
        GoLessons,
        GoDecisions,
        GoFindings,
        PrevProject,
        NextProject,
        Note,
        OpenFolder,
        Priority1,
        Priority2,
        Priority3,
        Priority4,
        Submit,
        Cancel
    ]
);

/// Keys act on the list only; text fields live outside it, so typing never triggers a command.
const NAV: &str = "LivtkNav";
const FORM: &str = "LivtkForm";

const POLL: Duration = Duration::from_millis(200);
const FADE: Duration = Duration::from_millis(180);
const FLASH: Duration = Duration::from_millis(900);
const SLIDE: Duration = Duration::from_millis(160);

pub fn run(dirs: Vec<PathBuf>) {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        bind_keys(cx);
        let options = WindowOptions {
            app_id: Some("livtk".into()),
            titlebar: Some(TitlebarOptions {
                title: Some("livtk".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        if let Err(e) = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| LivtkApp::new(Model::open(&dirs), window, cx))
        }) {
            eprintln!("livtk-app: cannot open a window: {e}");
            cx.quit();
        }
    });
}

pub fn bind_keys(cx: &mut App) {
    let nav = Some(NAV);
    cx.bind_keys([
        KeyBinding::new("j", Down, nav),
        KeyBinding::new("down", Down, nav),
        KeyBinding::new("k", Up, nav),
        KeyBinding::new("up", Up, nav),
        KeyBinding::new("enter", Open, nav),
        KeyBinding::new("escape", Close, nav),
        KeyBinding::new("a", Decide, nav),
        KeyBinding::new("s", Standing, nav),
        KeyBinding::new("x", Reject, nav),
        KeyBinding::new("n", NewTask, nav),
        KeyBinding::new("b", ToggleLayout, nav),
        KeyBinding::new("h", ToggleAll, nav),
        KeyBinding::new("c", Note, nav),
        KeyBinding::new("o", OpenFolder, nav),
        KeyBinding::new("g i", GoInbox, nav),
        KeyBinding::new("g t", GoTasks, nav),
        KeyBinding::new("g l", GoLessons, nav),
        KeyBinding::new("g d", GoDecisions, nav),
        KeyBinding::new("g f", GoFindings, nav),
        KeyBinding::new("[", PrevProject, nav),
        KeyBinding::new("]", NextProject, nav),
        KeyBinding::new("1", Priority1, nav),
        KeyBinding::new("2", Priority2, nav),
        KeyBinding::new("3", Priority3, nav),
        KeyBinding::new("4", Priority4, nav),
        KeyBinding::new("escape", Cancel, Some(FORM)),
        KeyBinding::new("ctrl-enter", Submit, Some(FORM)),
    ]);
}

struct NewTaskForm {
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    priority: u8,
}

pub struct LivtkApp {
    model: Model,
    nav: FocusHandle,
    list_scroll: UniformListScrollHandle,
    note: Entity<InputState>,
    form: Option<NewTaskForm>,
    _subscriptions: Vec<Subscription>,
    _poll: Task<()>,
}

impl LivtkApp {
    pub fn new(model: Model, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let nav = cx.focus_handle();
        let note = cx.new(|cx| InputState::new(window, cx).placeholder("Add a note, then Enter"));
        let subscriptions = vec![cx.subscribe_in(&note, window, Self::on_note)];
        // Claude, the CLI and git change the log from outside; look every 200 ms.
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let alive = this.update(cx, |app, cx| {
                    let changed = app.model.tick();
                    let busy = !app.model.flashed.is_empty() || !app.model.leaving.is_empty();
                    app.model.settle(Instant::now(), FLASH.max(FADE) + POLL);
                    if changed || busy {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        nav.focus(window, cx);
        LivtkApp {
            model,
            nav,
            list_scroll: UniformListScrollHandle::new(),
            note,
            form: None,
            _subscriptions: subscriptions,
            _poll: poll,
        }
    }

    fn on_note(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let InputEvent::PressEnter { .. } = event {
            let text = state.read(cx).value().to_string();
            self.model.add_note(&text);
            state.update(cx, |s, cx| s.set_value("", window, cx));
            cx.notify();
        }
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    // ---- actions ------------------------------------------------------------------------------

    fn after_move(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.display_index() {
            self.list_scroll.scroll_to_item(i, ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn choose(&mut self, choice: &Choice, cx: &mut Context<Self>) {
        self.model.choose(choice);
        cx.notify();
    }

    fn choice_where(&mut self, pick: impl Fn(&Choice) -> bool, cx: &mut Context<Self>) {
        let found = self
            .model
            .selected()
            .map(|r| crate::model::choices(r.kind, r.state))
            .and_then(|c| c.into_iter().find(|c| pick(c)));
        if let Some(c) = found {
            self.choose(&c, cx);
        }
    }

    fn open_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Task title"));
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Goal, approach, acceptance check (markdown)")
                .auto_grow(4, 12)
        });
        let sub = cx.subscribe_in(&title, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.submit_form(window, cx);
            }
        });
        self._subscriptions.push(sub);
        title.read(cx).focus_handle(cx).focus(window, cx);
        self.form = Some(NewTaskForm {
            title,
            body,
            priority: 3,
        });
        cx.notify();
    }

    fn submit_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let title = form.title.read(cx).value().to_string();
        let body = form.body.read(cx).value().to_string();
        if self.model.create_task(&title, &body, Some(form.priority)) {
            self.form = None;
            self.nav.focus(window, cx);
            self.after_move(cx);
        } else {
            cx.notify();
        }
    }

    fn close_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        self.nav.focus(window, cx);
        cx.notify();
    }

    fn open_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open a livtk project".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(dir) = paths.into_iter().next()
            {
                let _ = this.update(cx, |app, cx| {
                    if app.model.add_project(&dir)
                        && let (Some(config), Some(p)) =
                            (livtk_core::registry::config_dir(), app.model.project())
                    {
                        let _ = livtk_core::registry::add(&config, &p.dir);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    // ---- what the list shows ------------------------------------------------------------------

    /// The task list with a header row before each state group.
    fn items(&self) -> Vec<Item> {
        let mut items = Vec::with_capacity(self.model.rows.len() + 8);
        let mut current = None;
        for (i, r) in self.model.rows.iter().enumerate() {
            if current != Some(r.state) {
                let count = self
                    .model
                    .rows
                    .iter()
                    .filter(|x| x.state == r.state)
                    .count();
                items.push(Item::Header(r.kind, r.state, count));
                current = Some(r.state);
            }
            items.push(Item::Row(i));
        }
        items
    }

    fn display_index(&self) -> Option<usize> {
        let sel = self.model.selected_index()?;
        self.items()
            .iter()
            .position(|it| matches!(it, Item::Row(i) if *i == sel))
    }

    // ---- rendering ----------------------------------------------------------------------------

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut side = div()
            .w(px(220.))
            .h_full()
            .flex()
            .flex_col()
            .bg(color(SIDEBAR))
            .border_r_1()
            .border_color(color(BORDER))
            .px_2()
            .py_3()
            .gap_1()
            .child(
                div()
                    .px_2()
                    .pb_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(14.)).rounded(px(4.)).bg(color(ACCENT)))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("livtk")),
            )
            .child(section_label("Projects"));
        for (i, p) in self.model.projects.iter().enumerate() {
            let current = i == self.model.current;
            side = side.child(
                nav_item(
                    SharedString::from(format!("project-{i}")),
                    &p.name,
                    current,
                    p.inbox,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.model.select_project(i);
                    cx.notify();
                })),
            );
        }
        side = side.child(
            div()
                .id("open-folder")
                .px_2()
                .py_1()
                .rounded_md()
                .text_color(color(TEXT_3))
                .cursor_pointer()
                .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_2)))
                .child("+ Open project  o")
                .on_click(cx.listener(|this, _, _, cx| this.open_folder(cx))),
        );
        side = side.child(div().h(px(12.))).child(section_label("Views"));
        let inbox = self.model.project().map(|p| p.inbox).unwrap_or(0);
        for view in View::ALL {
            let count = if view == View::Inbox { inbox } else { 0 };
            side = side.child(
                nav_item(
                    SharedString::from(format!("view-{}", view.title())),
                    view.title(),
                    view == self.model.view,
                    count,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.model.set_view(view);
                    cx.notify();
                })),
            );
        }
        side.child(div().flex_1()).child(
            div()
                .px_2()
                .text_xs()
                .text_color(color(TEXT_3))
                .flex()
                .flex_col()
                .gap_1()
                .child("j k  move     Enter  open")
                .child("a  decide     x  reject")
                .child("s  standing rule")
                .child("n  new task   c  note")
                .child("g i / t / l / d / f  views")
                .child("b  board      h  show all"),
        )
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let m = &self.model;
        let title = m.view.title();
        let mut bar = div()
            .h(px(44.))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .border_b_1()
            .border_color(color(BORDER))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(
                div()
                    .text_color(color(TEXT_3))
                    .child(format!("{}", m.rows.len())),
            )
            .child(div().flex_1());
        if m.view == View::Tasks {
            let board = m.layout == Layout::Board;
            bar = bar.child(
                div()
                    .flex()
                    .rounded_md()
                    .border_1()
                    .border_color(color(BORDER))
                    .child(segment("layout-list", "List", !board).on_click(cx.listener(
                        |this, _, _, cx| {
                            if this.model.layout == Layout::Board {
                                this.model.toggle_layout();
                                cx.notify();
                            }
                        },
                    )))
                    .child(
                        segment("layout-board", "Board", board).on_click(cx.listener(
                            |this, _, _, cx| {
                                if this.model.layout == Layout::List {
                                    this.model.toggle_layout();
                                    cx.notify();
                                }
                            },
                        )),
                    ),
            );
        }
        if m.view != View::Inbox {
            bar = bar.child(
                segment(
                    "show-all",
                    if m.show_all { "All" } else { "Active" },
                    m.show_all,
                )
                .rounded_md()
                .border_1()
                .border_color(color(BORDER))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.model.toggle_show_all();
                    cx.notify();
                })),
            );
        }
        bar.child(
            div()
                .id("new-task")
                .px_3()
                .py_1()
                .rounded_md()
                .bg(color(ACCENT))
                .text_color(color(TEXT))
                .cursor_pointer()
                .hover(|s| s.opacity(0.9))
                .child("New task")
                .on_click(cx.listener(|this, _, window, cx| this.open_form(window, cx))),
        )
    }

    fn content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.model.projects.is_empty() {
            return empty(
                "No livtk projects yet",
                "Run `livtk init` in a project, then open it here with o, or start `livtk-app DIR`.",
            )
            .into_any_element();
        }
        match (self.model.view, self.model.layout) {
            (View::Inbox, _) => self.inbox(cx).into_any_element(),
            (View::Tasks, Layout::Board) => self.board(cx).into_any_element(),
            _ => self.list(window, cx).into_any_element(),
        }
    }

    /// The inbox: a short list, so rows can fade and collapse when the human acts on them.
    fn inbox(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.model.rows.is_empty() && self.model.leaving.is_empty() {
            return div()
                .size_full()
                .child(empty(
                    "Nothing waits for you",
                    "Plans, lessons and decisions to approve appear here.",
                ))
                .into_any_element();
        }
        let mut shown: Vec<(Summary, Option<Instant>)> =
            self.model.rows.iter().map(|r| (r.clone(), None)).collect();
        for (r, at, when) in &self.model.leaving {
            let i = (*at).min(shown.len());
            shown.insert(i, (r.clone(), Some(*when)));
        }
        let selected = self.model.selected().map(|r| r.id);
        let mut list = div()
            .id("inbox")
            .size_full()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap(px(1.));
        for (r, leaving) in shown {
            list = list.child(self.row(
                &r,
                Some(r.id) == selected && leaving.is_none(),
                leaving,
                true,
                cx,
            ));
        }
        list.into_any_element()
    }

    /// Tasks, lessons, decisions or findings: a virtualised list grouped by state.
    fn list(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.model.rows.is_empty() {
            let what = self.model.view.title().to_lowercase();
            return div()
                .size_full()
                .child(empty(
                    &format!("No {what} here"),
                    "h shows what is over too; n creates a task.",
                ))
                .into_any_element();
        }
        let count = self.items().len();
        uniform_list(
            "rows",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                let items = this.items();
                let selected = this.model.selected().map(|r| r.id);
                let mut out = Vec::with_capacity(range.len());
                for i in range {
                    match items.get(i) {
                        Some(Item::Header(kind, state, n)) => {
                            out.push(group_header(*kind, *state, *n).into_any_element())
                        }
                        Some(Item::Row(ri)) => {
                            let r = this.model.rows[*ri].clone();
                            let sel = Some(r.id) == selected;
                            out.push(this.row(&r, sel, None, false, cx));
                        }
                        None => out.push(div().into_any_element()),
                    }
                }
                out
            }),
        )
        .track_scroll(&self.list_scroll)
        .size_full()
        .px_2()
        .into_any_element()
    }

    fn board(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.model.selected().map(|r| r.id);
        let mut cols = div()
            .id("board")
            .size_full()
            .overflow_x_scroll()
            .flex()
            .gap_3()
            .p_3();
        for (state, rows) in self.model.columns() {
            let mut col = div()
                .id(SharedString::from(format!("col-{state}")))
                .w(px(272.))
                .flex_none()
                .h_full()
                .flex()
                .flex_col()
                .gap_2()
                .overflow_y_scroll()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_1()
                        .h(px(28.))
                        .child(state_mark(Kind::Task, state))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(state_title(state)),
                        )
                        .child(
                            div()
                                .text_color(color(TEXT_3))
                                .child(format!("{}", rows.len())),
                        ),
                );
            for r in rows {
                let id = r.id;
                let sel = Some(id) == selected;
                let flash = self.model.flashed.get(&id).copied();
                let card = div()
                    .id(SharedString::from(format!("card-{}", r.key)))
                    .p_3()
                    .rounded_lg()
                    .bg(color(if sel { SELECTED } else { RAISED }))
                    .border_1()
                    .border_color(color(if sel { ACCENT } else { BORDER }))
                    .cursor_pointer()
                    .hover(|s| s.border_color(color(TEXT_3)))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when(r.blocked, |d| d.opacity(0.6))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(color(TEXT_3))
                            .child(r.key.clone())
                            .child(div().flex_1())
                            .when(r.blocked, |d| d.child("blocked"))
                            .child(priority_mark(r.priority)),
                    )
                    .child(div().child(r.title.clone()))
                    .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                        this.model.select(id);
                        if e.click_count() >= 2 {
                            this.model.open_detail();
                        }
                        cx.notify();
                    }));
                col = col.child(flashing(card, &r.key, flash));
            }
            cols = cols.child(col);
        }
        cols
    }

    fn row(
        &self,
        r: &Summary,
        selected: bool,
        leaving: Option<Instant>,
        inbox: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = r.id;
        let flash = self.model.flashed.get(&id).copied();
        let mut row = div()
            .id(SharedString::from(format!("row-{}", r.key)))
            .w_full()
            .h(px(ROW))
            .flex_none()
            .px_3()
            .flex()
            .items_center()
            .gap_3()
            .rounded_md()
            .cursor_pointer()
            .when(selected, |d| d.bg(color(SELECTED)))
            .when(!selected, |d| d.hover(|s| s.bg(color(HOVER))))
            .when(r.blocked && !r.state.is_closed(), |d| {
                d.text_color(color(TEXT_2))
            });
        if r.kind == Kind::Task {
            row = row.child(priority_mark(r.priority));
        }
        row = row
            .child(
                div()
                    .w(px(64.))
                    .flex_none()
                    .text_color(color(TEXT_3))
                    .child(r.key.clone()),
            )
            .child(state_mark(r.kind, r.state))
            .child(div().flex_1().min_w_0().truncate().child(r.title.clone()));
        if r.blocked && !r.state.is_closed() {
            row = row.child(tag("blocked", TEXT_3));
        }
        if r.standing {
            row = row.child(tag("standing rule", GREEN));
        }
        if inbox {
            let (label, c) = match r.kind {
                Kind::Task => ("acknowledge", ACCENT),
                Kind::Lesson => ("ratify", GREEN),
                Kind::Decision => ("accept", GREEN),
                Kind::Finding => ("", TEXT_3),
            };
            row = row.child(tag(label, c));
            if selected {
                row = row.child(div().text_xs().text_color(color(TEXT_3)).child("a"));
            }
        }
        let row = row.on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
            this.model.select(id);
            if e.click_count() >= 2 {
                this.model.open_detail();
            }
            cx.notify();
        }));
        match leaving {
            Some(at) => {
                let started = at.elapsed().as_secs_f32() / FADE.as_secs_f32();
                row.with_animation(
                    SharedString::from(format!("leave-{}", r.key)),
                    Animation::new(FADE).with_easing(ease_in_out),
                    move |el, t| {
                        let t = (started + t).min(1.0);
                        el.opacity(1.0 - t).h(px(ROW * (1.0 - t)))
                    },
                )
                .into_any_element()
            }
            None if selected => row
                .with_animation(
                    SharedString::from(format!("select-{}", r.key)),
                    Animation::new(Duration::from_millis(120)).with_easing(ease_out_quint()),
                    |el, t| el.bg(with_alpha(SELECTED, t)),
                )
                .into_any_element(),
            None => flashing(row, &r.key, flash),
        }
    }

    fn detail(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let d = self.model.detail.clone()?;
        let n = &d.node;
        let mut choices = div().flex().flex_wrap().gap_2();
        for (i, c) in d.choices.iter().enumerate() {
            let c2 = c.clone();
            let primary = c.decides && !matches!(c.to, State::Rejected | State::Retired);
            let hint = match (i, c.always, c.to) {
                (_, true, _) => " s",
                (_, _, State::Rejected) => " x",
                (0, _, _) if primary => " a",
                _ => "",
            };
            choices = choices.child(
                div()
                    .id(SharedString::from(format!("choice-{i}")))
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .when(primary, |d| d.bg(color(ACCENT)).text_color(color(TEXT)))
                    .when(!primary, |d| {
                        d.bg(color(RAISED))
                            .border_1()
                            .border_color(color(BORDER))
                            .text_color(color(TEXT_2))
                    })
                    .hover(|s| s.opacity(0.85))
                    .child(format!("{}{hint}", c.label))
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(&c2, cx))),
            );
        }
        let mut meta = div()
            .flex()
            .items_center()
            .gap_2()
            .text_color(color(TEXT_2))
            .child(state_mark(n.kind, n.state))
            .child(n.state.as_str().to_owned());
        if n.kind == Kind::Task {
            meta = meta
                .child(priority_mark(n.priority))
                .child(format!("P{}", n.priority));
        } else {
            meta = meta.child(tag(n.kind.as_str(), TEXT_3));
        }
        if n.props.get("scope").and_then(|v| v.as_str()) == Some("always") {
            meta = meta.child(tag("standing rule", GREEN));
        }
        let mut links = div().flex().flex_col().gap_1();
        for (label, briefs) in [
            ("Waits for", &d.links.depends_on),
            ("Blocks", &d.links.blocks),
            ("Replaces", &d.replaces),
            ("Replaced by", &d.replaced_by),
            ("Learned from", &d.learned_from),
        ] {
            if !briefs.is_empty() {
                links = links.child(link_group(label, briefs));
            }
        }
        let mut timeline = div().flex().flex_col().gap_2();
        for e in d.history.iter().rev() {
            timeline = timeline.child(event_line(e));
        }
        let panel = div()
            .id(SharedString::from(format!("detail-{}", n.key)))
            .w(px(460.))
            .flex_none()
            .h_full()
            .bg(color(PANEL))
            .border_l_1()
            .border_color(color(BORDER))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(44.))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(color(BORDER))
                    .child(div().text_color(color(TEXT_3)).child(n.key.clone()))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("close-detail")
                            .px_2()
                            .rounded_md()
                            .text_color(color(TEXT_3))
                            .cursor_pointer()
                            .hover(|s| s.bg(color(HOVER)))
                            .child("Esc")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.model.close_detail();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("detail-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(n.title.clone()),
                    )
                    .child(meta)
                    .child(choices)
                    .when(!n.body.is_empty(), |d| {
                        d.child(div().text_color(color(TEXT_2)).child(TextView::markdown(
                            SharedString::from(format!("body-{}", n.key)),
                            n.body.clone(),
                        )))
                    })
                    .child(links)
                    .child(section_label("Activity"))
                    .child(timeline),
            )
            .child(
                div()
                    .flex_none()
                    .p_3()
                    .border_t_1()
                    .border_color(color(BORDER))
                    .child(Input::new(&self.note)),
            );
        Some(
            panel
                .with_animation(
                    SharedString::from(format!("slide-{}", n.key)),
                    Animation::new(SLIDE).with_easing(ease_out_quint()),
                    |el, t| el.opacity(t).ml(px(24.0 * (1.0 - t))),
                )
                .into_any_element(),
        )
    }

    fn form_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.form.as_ref()?;
        let mut priorities = div().flex().gap_1();
        for p in 1..=4u8 {
            let on = form.priority == p;
            priorities = priorities.child(
                div()
                    .id(SharedString::from(format!("prio-{p}")))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .bg(color(if on { SELECTED } else { RAISED }))
                    .border_1()
                    .border_color(color(if on { ACCENT } else { BORDER }))
                    .child(priority_mark(p))
                    .child(["Urgent", "High", "Medium", "Low"][usize::from(p - 1)])
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(f) = &mut this.form {
                            f.priority = p;
                        }
                        cx.notify();
                    })),
            );
        }
        let card = div()
            .w(px(600.))
            .p_4()
            .rounded_lg()
            .bg(color(PANEL))
            .border_1()
            .border_color(color(BORDER))
            .shadow_lg()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().font_weight(FontWeight::SEMIBOLD).child("New task"))
            .child(Input::new(&form.title))
            .child(gpui_kit::component::input::Textarea::new(&form.body))
            .child(priorities)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .text_color(color(TEXT_3))
                    .child("Esc to cancel · Ctrl+Enter to create · it starts in the backlog")
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("create-task")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(color(ACCENT))
                            .text_color(color(TEXT))
                            .text_sm()
                            .cursor_pointer()
                            .child("Create")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.submit_form(window, cx)),
                            ),
                    ),
            );
        Some(
            div()
                .absolute()
                .inset_0()
                .key_context(FORM)
                .on_action(cx.listener(|this, _: &Cancel, window, cx| this.close_form(window, cx)))
                .on_action(cx.listener(|this, _: &Submit, window, cx| this.submit_form(window, cx)))
                .bg(with_alpha(0x000000, 0.55))
                .flex()
                .justify_center()
                .pt(px(120.))
                .child(card)
                .with_animation(
                    "form-in",
                    Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
                    |el, t| el.opacity(t),
                )
                .into_any_element(),
        )
    }

    fn notice(&self) -> Option<AnyElement> {
        let text = self.model.notice.clone()?;
        Some(
            div()
                .absolute()
                .bottom(px(16.))
                .right(px(16.))
                .max_w(px(520.))
                .p_3()
                .rounded_lg()
                .bg(color(RAISED))
                .border_1()
                .border_color(with_alpha(RED, 0.6))
                .text_color(color(TEXT))
                .shadow_lg()
                .child(text)
                .into_any_element(),
        )
    }
}

impl Render for LivtkApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.content(window, cx);
        let detail = self.detail(cx);
        let form = self.form_overlay(cx);
        let notice = self.notice();
        div()
            .size_full()
            .relative()
            .flex()
            .bg(color(BG))
            .text_color(color(TEXT))
            .text_sm()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .key_context(NAV)
                    .track_focus(&self.nav)
                    .on_action(cx.listener(|this, _: &Down, _, cx| {
                        this.model.select_by(1);
                        this.after_move(cx);
                    }))
                    .on_action(cx.listener(|this, _: &Up, _, cx| {
                        this.model.select_by(-1);
                        this.after_move(cx);
                    }))
                    .on_action(cx.listener(|this, _: &Open, _, cx| {
                        this.model.open_detail();
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &Close, _, cx| {
                        this.model.close_detail();
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &Decide, _, cx| {
                        this.model.decide();
                        cx.notify();
                    }))
                    .on_action(
                        cx.listener(|this, _: &Standing, _, cx| {
                            this.choice_where(|c| c.always, cx)
                        }),
                    )
                    .on_action(cx.listener(|this, _: &Reject, _, cx| {
                        this.choice_where(|c| c.to == State::Rejected, cx)
                    }))
                    .on_action(
                        cx.listener(|this, _: &NewTask, window, cx| this.open_form(window, cx)),
                    )
                    .on_action(cx.listener(|this, _: &OpenFolder, _, cx| this.open_folder(cx)))
                    .on_action(cx.listener(|this, _: &Note, window, cx| {
                        if this.model.detail.is_none() {
                            this.model.open_detail();
                        }
                        this.note.read(cx).focus_handle(cx).focus(window, cx);
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &ToggleLayout, _, cx| {
                        if this.model.view == View::Tasks {
                            this.model.toggle_layout();
                            cx.notify();
                        }
                    }))
                    .on_action(cx.listener(|this, _: &ToggleAll, _, cx| {
                        this.model.toggle_show_all();
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &GoInbox, _, cx| go(this, View::Inbox, cx)))
                    .on_action(cx.listener(|this, _: &GoTasks, _, cx| go(this, View::Tasks, cx)))
                    .on_action(
                        cx.listener(|this, _: &GoLessons, _, cx| go(this, View::Lessons, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &GoDecisions, _, cx| go(this, View::Decisions, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &GoFindings, _, cx| go(this, View::Findings, cx)),
                    )
                    .on_action(cx.listener(|this, _: &PrevProject, _, cx| {
                        let i = this.model.current.saturating_sub(1);
                        this.model.select_project(i);
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &NextProject, _, cx| {
                        let i = this.model.current + 1;
                        this.model.select_project(i);
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &Priority1, _, cx| prio(this, 1, cx)))
                    .on_action(cx.listener(|this, _: &Priority2, _, cx| prio(this, 2, cx)))
                    .on_action(cx.listener(|this, _: &Priority3, _, cx| prio(this, 3, cx)))
                    .on_action(cx.listener(|this, _: &Priority4, _, cx| prio(this, 4, cx)))
                    .child(self.sidebar(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(self.header(cx))
                            .child(div().flex_1().min_h_0().child(content)),
                    ),
            )
            .children(detail)
            .children(form)
            .children(notice)
    }
}

fn go(app: &mut LivtkApp, view: View, cx: &mut Context<LivtkApp>) {
    app.model.set_view(view);
    cx.notify();
}

fn prio(app: &mut LivtkApp, p: u8, cx: &mut Context<LivtkApp>) {
    app.model.set_priority(p);
    cx.notify();
}

enum Item {
    Header(Kind, State, usize),
    Row(usize),
}

// ---- small pieces ---------------------------------------------------------------------------------

fn section_label(text: &str) -> Div {
    div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(color(TEXT_3))
        .child(text.to_owned())
}

fn nav_item(id: SharedString, label: &str, active: bool, count: usize) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .flex()
        .items_center()
        .cursor_pointer()
        .when(active, |d| d.bg(color(SELECTED)).text_color(color(TEXT)))
        .when(!active, |d| {
            d.text_color(color(TEXT_2)).hover(|s| s.bg(color(HOVER)))
        })
        .child(div().flex_1().truncate().child(label.to_owned()))
        .when(count > 0, |d| {
            d.child(
                div()
                    .px(px(6.))
                    .rounded_full()
                    .bg(with_alpha(ACCENT, 0.25))
                    .text_color(color(ACCENT))
                    .text_xs()
                    .child(format!("{count}")),
            )
        })
}

fn segment(id: &'static str, label: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py(px(3.))
        .cursor_pointer()
        .when(on, |d| d.bg(color(SELECTED)).text_color(color(TEXT)))
        .when(!on, |d| {
            d.text_color(color(TEXT_3))
                .hover(|s| s.text_color(color(TEXT_2)))
        })
        .child(label)
}

fn tag(label: &str, c: u32) -> Div {
    div()
        .px(px(6.))
        .rounded(px(4.))
        .text_xs()
        .border_1()
        .border_color(with_alpha(c, 0.4))
        .text_color(color(c))
        .child(label.to_owned())
}

fn group_header(kind: Kind, state: State, count: usize) -> Div {
    div()
        .w_full()
        .h(px(ROW))
        .flex_none()
        .px_3()
        .flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(color(TEXT_2))
        .child(state_mark(kind, state))
        .child(state_title(state))
        .child(div().text_color(color(TEXT_3)).child(format!("{count}")))
}

fn state_title(state: State) -> String {
    let s = state.as_str();
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn empty(title: &str, hint: &str) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .child(div().text_color(color(TEXT_2)).child(title.to_owned()))
        .child(
            div()
                .text_xs()
                .text_color(color(TEXT_3))
                .child(hint.to_owned()),
        )
}

fn link_group(label: &str, briefs: &[Brief]) -> Div {
    let mut g = div().flex().flex_col().gap_1().child(
        div()
            .text_xs()
            .text_color(color(TEXT_3))
            .child(label.to_owned()),
    );
    for b in briefs {
        g = g.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(state_mark(Kind::Task, b.state))
                .child(div().text_color(color(TEXT_3)).child(b.key.clone()))
                .child(div().truncate().child(b.title.clone())),
        );
    }
    g
}

/// A row that flashes the accent once when someone else changed it.
fn flashing<E: Styled + IntoElement + 'static>(
    el: E,
    key: &str,
    at: Option<Instant>,
) -> AnyElement {
    match at {
        Some(at) => {
            let started = at.elapsed().as_secs_f32() / FLASH.as_secs_f32();
            el.with_animation(
                SharedString::from(format!("flash-{key}")),
                Animation::new(FLASH).with_easing(ease_in_out),
                move |el, t| {
                    let t = (started + t).min(1.0);
                    el.bg(with_alpha(ACCENT, 0.22 * (1.0 - t)))
                },
            )
            .into_any_element()
        }
        None => el.into_any_element(),
    }
}

fn event_line(e: &Event) -> Div {
    let when = ago(&e.at);
    let what = match &e.op {
        Op::NodeCreate { data, .. } => format!("created \"{}\"", data.title),
        Op::NodeSet { data, .. } => {
            let mut parts = Vec::new();
            if let Some(s) = data.state {
                parts.push(format!("moved to {s}"));
            }
            if data.title.is_some() {
                parts.push("renamed".into());
            }
            if data.body.is_some() {
                parts.push("edited the body".into());
            }
            if let Some(p) = data.priority {
                parts.push(format!("set priority P{}", p.get()));
            }
            if data.props.is_some() {
                parts.push("changed its scope".into());
            }
            parts.join(", ")
        }
        Op::NodeNote { data, .. } => data.text.clone(),
        Op::EdgeAdd { data } => format!("linked: {}", data.rel),
        Op::EdgeRemove { data } => format!("unlinked: {}", data.rel),
    };
    let human = e.by == "human";
    let note = matches!(e.op, Op::NodeNote { .. });
    div()
        .flex()
        .gap_2()
        .child(
            div()
                .mt(px(5.))
                .size(px(6.))
                .flex_none()
                .rounded_full()
                .bg(color(if human { ACCENT } else { TEXT_3 })),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_xs().text_color(color(TEXT_3)).child(format!(
                    "{} · {when}",
                    if human { "you" } else { e.by.as_str() }
                )))
                .child(
                    div()
                        .when(note, |d| {
                            d.p_2()
                                .rounded_md()
                                .bg(color(RAISED))
                                .border_1()
                                .border_color(color(BORDER))
                        })
                        .text_color(color(if note { TEXT } else { TEXT_2 }))
                        .child(what),
                ),
        )
}

/// "just now", "5m ago", "3h ago", "2d ago", or the date.
fn ago(at: &str) -> String {
    let Ok(t) = at.parse::<jiff::Timestamp>() else {
        return at.to_owned();
    };
    let secs = jiff::Timestamp::now().as_second() - t.as_second();
    match secs {
        ..60 => "just now".into(),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        86400..604800 => format!("{}d ago", secs / 86400),
        _ => at.get(..10).unwrap_or(at).to_owned(),
    }
}
