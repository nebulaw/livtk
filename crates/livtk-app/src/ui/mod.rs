//! The window: a sidebar on the black canvas, an inset content panel (header, display toolbar,
//! the list or board), the detail panel, and overlays (new task, shortcuts, notices). It renders
//! the model and forwards input to it; no behaviour lives here.
//!
//! Keys are bound only on the list area. Text fields (the filter, notes, the new-task form) live
//! outside it, so typing never triggers a command.

mod theme;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::component::Sizable;
use gpui_kit::component::input::{self, Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use livtk_core::event::{Event, Op};
use livtk_core::{Brief, Kind, State, Summary};
use uuid::Uuid;

use crate::model::{
    Choice, Display, GroupKey, Grouping, Layout, Model, Ordering, View, choices, drop_allowed,
};
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
        FocusFilter,
        ToggleHelp,
        Submit,
        Cancel
    ]
);

const NAV: &str = "LivtkNav";
const FORM: &str = "LivtkForm";

const POLL: Duration = Duration::from_millis(200);
const FADE: Duration = Duration::from_millis(180);
const FLASH: Duration = Duration::from_millis(900);
const SLIDE: Duration = Duration::from_millis(160);

/// Every shortcut, in the order the shortcuts modal shows them: (section, keys, what it does).
/// In `keys`, the words "then", "or" and "+" are separators; everything else is a key cap.
const SHORTCUTS: &[(&str, &[&str], &str)] = &[
    ("Navigate", &["J", "or", "↓"], "Next row"),
    ("Navigate", &["K", "or", "↑"], "Previous row"),
    ("Navigate", &["Enter"], "Open the detail panel"),
    ("Navigate", &["Esc"], "Close the panel or a modal"),
    ("Navigate", &["G", "then", "I"], "Inbox"),
    ("Navigate", &["G", "then", "T"], "Tasks"),
    ("Navigate", &["G", "then", "L"], "Lessons"),
    ("Navigate", &["G", "then", "D"], "Decisions"),
    ("Navigate", &["G", "then", "F"], "Findings"),
    ("Navigate", &["[", "or", "]"], "Previous or next project"),
    ("Decide", &["A"], "Acknowledge, ratify or accept"),
    ("Decide", &["S"], "Ratify as a standing rule"),
    ("Decide", &["X"], "Reject"),
    ("Work", &["N"], "New task"),
    ("Work", &["C"], "Add a note"),
    (
        "Work",
        &["1", "2", "3", "4"],
        "Priority: urgent, high, medium, low",
    ),
    ("Work", &["Drag"], "Move to another state or priority"),
    ("Display", &["/"], "Filter"),
    ("Display", &["B"], "List or board"),
    (
        "Display",
        &["H"],
        "Show dismissed: canceled, rejected, retired",
    ),
    ("Display", &["O"], "Open a project folder"),
    ("Display", &["Ctrl", "+", "/"], "These shortcuts"),
];

pub fn run(dirs: Vec<PathBuf>) {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        theme::apply(cx);
        bind_keys(cx);
        let options = WindowOptions {
            app_id: Some("livtk".into()),
            titlebar: Some(TitlebarOptions {
                title: Some("livtk".into()),
                ..Default::default()
            }),
            window_background: WindowBackgroundAppearance::Opaque,
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
        KeyBinding::new("/", FocusFilter, nav),
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
        // Everywhere, typing included: Ctrl+/ is never text.
        KeyBinding::new("ctrl-/", ToggleHelp, None),
        KeyBinding::new("escape", Cancel, Some(FORM)),
        KeyBinding::new("ctrl-enter", Submit, Some(FORM)),
    ]);
}

/// What is being dragged: a node, with what it takes to judge a drop target.
#[derive(Clone)]
struct Dragged {
    id: Uuid,
    key: String,
    title: String,
    kind: Kind,
    state: State,
    priority: u8,
}

impl Dragged {
    fn of(r: &Summary) -> Dragged {
        Dragged {
            id: r.id,
            key: r.key.clone(),
            title: r.title.clone(),
            kind: r.kind,
            state: r.state,
            priority: r.priority,
        }
    }

    fn fits(&self, target: GroupKey) -> bool {
        drop_allowed(self.kind, self.state, self.priority, target)
    }
}

impl Render for Dragged {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .w(px(320.))
            .rounded_md()
            .bg(color(LIFTED))
            .border_1()
            .border_color(color(BORDER_STRONG))
            .shadow_lg()
            .flex()
            .items_center()
            .gap_2()
            .text_sm()
            .text_color(color(TEXT))
            .child(state_mark(self.kind, self.state))
            .child(div().text_color(color(TEXT_3)).child(self.key.clone()))
            .child(div().truncate().child(self.title.clone()))
    }
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
    filter: Entity<InputState>,
    note: Entity<InputState>,
    form: Option<NewTaskForm>,
    help: bool,
    _subscriptions: Vec<Subscription>,
    _poll: Task<()>,
}

impl LivtkApp {
    pub fn new(model: Model, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let nav = cx.focus_handle();
        let note = cx.new(|cx| InputState::new(window, cx).placeholder("Leave a note…"));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter…"));
        let subscriptions = vec![
            cx.subscribe_in(&note, window, Self::on_note),
            cx.subscribe_in(&filter, window, Self::on_filter),
        ];
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
            filter,
            note,
            form: None,
            help: false,
            _subscriptions: subscriptions,
            _poll: poll,
        }
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn help_open(&self) -> bool {
        self.help
    }

    pub fn form_open(&self) -> bool {
        self.form.is_some()
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

    fn on_filter(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let text = state.read(cx).value().to_string();
                if text != self.model.display().filter {
                    self.model.set_filter(&text);
                    cx.notify();
                }
            }
            InputEvent::PressEnter { .. } => self.nav.focus(window, cx),
            _ => {}
        }
    }

    /// Show the current view's filter text in the filter field.
    fn sync_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = self.model.display().filter;
        if self.filter.read(cx).value().as_ref() != want {
            self.filter
                .update(cx, |s, cx| s.set_value(want.clone(), window, cx));
        }
    }

    fn go(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        self.model.set_view(view);
        self.sync_filter(window, cx);
        cx.notify();
    }

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
            .map(|r| choices(r.kind, r.state))
            .and_then(|c| c.into_iter().find(|c| pick(c)));
        if let Some(c) = found {
            self.choose(&c, cx);
        }
    }

    fn open_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.help = false;
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Task title"));
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Add a plan: goal, approach, acceptance check…")
                .auto_grow(5, 14)
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
            self.sync_filter(window, cx);
            self.nav.focus(window, cx);
            self.after_move(cx);
        } else {
            cx.notify();
        }
    }

    fn close_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        self.help = false;
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

    /// The list as rows of the virtualised list: a band before each group, then its rows.
    fn items(&self) -> Vec<Item> {
        let mut items = Vec::with_capacity(self.model.rows.len() + self.model.groups.len());
        for (gi, g) in self.model.groups.iter().enumerate() {
            if g.key != GroupKey::All {
                items.push(Item::Band(gi));
            }
            items.extend((g.start..g.start + g.shown).map(Item::Row));
        }
        items
    }

    fn display_index(&self) -> Option<usize> {
        let sel = self.model.selected_index()?;
        self.items()
            .iter()
            .position(|it| matches!(it, Item::Row(i) if *i == sel))
    }

    // ---- rendering: the frame -----------------------------------------------------------------

    fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        let mut side = div()
            .w(px(232.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .px_2()
            .py_3()
            .gap(px(2.))
            .child(
                div()
                    .px_2()
                    .pb_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(18.))
                            .rounded(px(5.))
                            .bg(color(ACCENT))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(11.))
                            .font_weight(FontWeight::BOLD)
                            .child("L"),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color(TEXT))
                            .child("livtk"),
                    ),
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
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.model.select_project(i);
                    this.sync_filter(window, cx);
                    cx.notify();
                })),
            );
        }
        side = side.child(
            div()
                .id("open-folder")
                .px_2()
                .h(px(28.))
                .flex()
                .items_center()
                .rounded_md()
                .text_color(color(TEXT_4))
                .cursor_pointer()
                .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_3)))
                .child("+ Open project")
                .on_click(cx.listener(|this, _, _, cx| this.open_folder(cx))),
        );
        side = side.child(div().h(px(14.))).child(section_label("Views"));
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
                .on_click(cx.listener(move |this, _, window, cx| this.go(view, window, cx))),
            );
        }
        side.child(div().flex_1()).child(
            div()
                .id("shortcuts")
                .px_2()
                .h(px(28.))
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .text_xs()
                .text_color(color(TEXT_4))
                .cursor_pointer()
                .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_3)))
                .child(kbd("Ctrl"))
                .child(kbd("/"))
                .child("Shortcuts")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.help = !this.help;
                    cx.notify();
                })),
        )
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let m = &self.model;
        let mut bar = div()
            .h(px(46.))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .border_b_1()
            .border_color(color(BORDER))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(TEXT))
                    .child(m.view.title()),
            )
            .child(
                div()
                    .text_color(color(TEXT_4))
                    .child(format!("{}", m.filtered.len())),
            )
            .child(div().flex_1());
        if m.view == View::Tasks {
            let board = m.layout == Layout::Board;
            bar = bar.child(
                segments()
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
        bar.child(
            div()
                .id("new-task")
                .h(px(28.))
                .px_3()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .bg(color(ACCENT))
                .text_color(color(TEXT))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .hover(|s| s.bg(color(ACCENT_HOVER)))
                .child("New task")
                .child(div().text_xs().text_color(with_alpha(TEXT, 0.7)).child("N"))
                .on_click(cx.listener(|this, _, window, cx| this.open_form(window, cx))),
        )
    }

    /// Filter, priority chips, grouping and ordering: Linear's Display menu, laid out flat.
    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let d: Display = self.model.display();
        let mut chips = div().flex().items_center().gap_1();
        for p in 1..=4u8 {
            let on = d.priorities.contains(&p);
            chips = chips.child(
                div()
                    .id(SharedString::from(format!("prio-filter-{p}")))
                    .h(px(26.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded_md()
                    .border_1()
                    .cursor_pointer()
                    .when(on, |s| {
                        s.bg(color(SELECTED))
                            .border_color(color(BORDER_STRONG))
                            .text_color(color(TEXT))
                    })
                    .when(!on, |s| {
                        s.border_color(color(BORDER))
                            .text_color(color(TEXT_3))
                            .hover(|h| h.bg(color(HOVER)))
                    })
                    .child(priority_mark(p))
                    .child(PRIORITY_NAMES[usize::from(p - 1)])
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.toggle_priority(p);
                        cx.notify();
                    })),
            );
        }
        let group = |id: &'static str, label: &'static str, g: Grouping| {
            segment(id, label, d.grouping == g).on_click(cx.listener(move |this, _, _, cx| {
                this.model.set_grouping(g);
                cx.notify();
            }))
        };
        let board = self.model.view == View::Tasks && self.model.layout == Layout::Board;
        let groups = segments()
            .child(group("group-state", "State", Grouping::State))
            .child(group("group-priority", "Priority", Grouping::Priority))
            // A board needs columns: as in Linear, it is never ungrouped.
            .when(!board, |s| {
                s.child(group("group-none", "None", Grouping::None))
            });
        let order = |id: &'static str, label: &'static str, o: Ordering| {
            segment(id, label, d.ordering == o).on_click(cx.listener(move |this, _, _, cx| {
                this.model.set_ordering(o);
                cx.notify();
            }))
        };
        let orders = segments()
            .child(order("order-priority", "Priority", Ordering::Priority))
            .child(order("order-updated", "Updated", Ordering::Updated))
            .child(order("order-created", "Created", Ordering::Created));
        let shown = segment(
            "show-all",
            if self.model.show_all {
                "Dismissed shown"
            } else {
                "Dismissed hidden"
            },
            self.model.show_all,
        )
        .rounded_md()
        .border_1()
        .border_color(color(BORDER))
        .on_click(cx.listener(|this, _, _, cx| {
            this.model.toggle_show_all();
            cx.notify();
        }));
        div()
            .flex_none()
            .h(px(44.))
            .px_4()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(color(BORDER))
            .on_action(cx.listener(|this, _: &input::Escape, window, cx| {
                this.nav.focus(window, cx);
                cx.notify();
            }))
            .child(div().w(px(220.)).child(Input::new(&self.filter).small()))
            .child(chips)
            .child(div().flex_1())
            .child(toolbar_label("Group"))
            .child(groups)
            .child(toolbar_label("Order"))
            .child(orders)
            .when(self.model.view != View::Inbox, |t| t.child(shown))
    }

    fn content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.model.projects.is_empty() {
            return empty(
                "No livtk projects yet",
                "Run `livtk init` in a project, then open it with + Open project, or start `livtk-app DIR`.",
            )
            .into_any_element();
        }
        match (self.model.view, self.model.layout) {
            (View::Inbox, _) => self.inbox(cx).into_any_element(),
            (View::Tasks, Layout::Board) => self.board(cx).into_any_element(),
            _ => self.list(window, cx).into_any_element(),
        }
    }

    // ---- rendering: lists ---------------------------------------------------------------------

    /// The inbox: a short list, so rows can fade and collapse when the human acts on them.
    fn inbox(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.model.rows.is_empty() && self.model.leaving.is_empty() {
            return div()
                .size_full()
                .child(empty(
                    "Nothing waits for you",
                    "Plans to acknowledge, lessons to ratify and decisions to accept appear here.",
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
            .py_1()
            .flex()
            .flex_col();
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

    /// Tasks, lessons, decisions or findings: a virtualised list in solid group bands.
    fn list(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.model.rows.is_empty() && self.model.groups.is_empty() {
            let what = self.model.view.title().to_lowercase();
            return div()
                .size_full()
                .child(empty(
                    &format!("No {what} here"),
                    "Clear the filter, press H to show dismissed work, or N for a new task.",
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
                        Some(Item::Band(gi)) => out.push(this.band(*gi, cx)),
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
        .into_any_element()
    }

    /// A group's solid header band: its mark, name and count. Click to collapse; drop a row on
    /// it to move the row into the group.
    fn band(&self, gi: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(g) = self.model.groups.get(gi).cloned() else {
            return div().into_any_element();
        };
        let (mark, label) = group_label(g.key, g.kind.unwrap_or(Kind::Task));
        let key = g.key;
        div()
            .id(SharedString::from(format!("band-{gi}")))
            .w_full()
            .h(px(ROW))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .bg(color(SURFACE))
            .border_b_1()
            .border_color(color(BORDER))
            .cursor_pointer()
            .hover(|s| s.bg(color(HOVER)))
            .child(
                div()
                    .w(px(10.))
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(if g.collapsed { "▸" } else { "▾" }),
            )
            .child(mark)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(TEXT))
                    .child(label),
            )
            .child(
                div()
                    .text_color(color(TEXT_4))
                    .child(format!("{}", g.count)),
            )
            .drag_over::<Dragged>(move |style, d, _, _| {
                if d.fits(key) {
                    style
                        .bg(with_alpha(ACCENT, 0.18))
                        .border_color(color(ACCENT))
                } else {
                    style
                }
            })
            .on_drop(cx.listener(move |this, d: &Dragged, _, cx| {
                this.model.drop_on(d.id, key);
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.toggle_collapsed(key);
                cx.notify();
            }))
            .into_any_element()
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
        let blocked = r.blocked && !r.state.is_closed();
        let mut row = div()
            .id(SharedString::from(format!("row-{}", r.key)))
            .w_full()
            .h(px(ROW))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(color(CONTENT))
            .cursor_pointer()
            .when(selected, |d| d.bg(color(SELECTED)))
            .when(!selected, |d| d.hover(|s| s.bg(color(HOVER))))
            .text_color(color(if blocked { TEXT_3 } else { TEXT_2 }));
        if r.kind == Kind::Task {
            row = row.child(priority_mark(r.priority));
        }
        row = row
            .child(
                div()
                    .w(px(62.))
                    .flex_none()
                    .text_color(color(TEXT_4))
                    .child(r.key.clone()),
            )
            .child(state_mark(r.kind, r.state))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(color(if blocked { TEXT_3 } else { TEXT }))
                    .child(r.title.clone()),
            );
        if blocked {
            row = row.child(tag("Blocked", TEXT_4));
        }
        if r.standing {
            row = row.child(tag("Standing rule", DONE));
        }
        if inbox {
            let label = match r.kind {
                Kind::Task => "Acknowledge",
                Kind::Lesson => "Ratify",
                Kind::Decision => "Accept",
                Kind::Finding => "",
            };
            row = row.child(tag(label, ACCENT));
            if selected {
                row = row.child(kbd("A"));
            }
        }
        row = row.child(
            div()
                .w(px(60.))
                .flex_none()
                .text_xs()
                .text_color(color(TEXT_4))
                .flex()
                .justify_end()
                .child(ago(&r.updated)),
        );
        let dragged = Dragged::of(r);
        let row = row
            .on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
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

    fn board(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.model.selected().map(|r| r.id);
        let mut cols = div()
            .id("board")
            .size_full()
            .overflow_x_scroll()
            .flex()
            .gap_2()
            .p_3();
        for (key, rows) in self.model.columns() {
            let (mark, label) = group_label(key, Kind::Task);
            let mut col = div()
                .id(SharedString::from(format!("col-{key:?}")))
                .w(px(288.))
                .flex_none()
                .h_full()
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .rounded_lg()
                .bg(color(SURFACE))
                .border_1()
                .border_color(color(BORDER))
                .overflow_y_scroll()
                .drag_over::<Dragged>(move |style, d, _, _| {
                    if d.fits(key) {
                        style
                            .bg(with_alpha(ACCENT, 0.12))
                            .border_color(color(ACCENT))
                    } else {
                        style
                    }
                })
                .on_drop(cx.listener(move |this, d: &Dragged, _, cx| {
                    this.model.drop_on(d.id, key);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_1()
                        .h(px(28.))
                        .child(mark)
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(color(TEXT))
                                .child(label),
                        )
                        .child(
                            div()
                                .text_color(color(TEXT_4))
                                .child(format!("{}", rows.len())),
                        ),
                );
            for r in rows {
                let id = r.id;
                let sel = Some(id) == selected;
                let blocked = r.blocked && !r.state.is_closed();
                let flash = self.model.flashed.get(&id).copied();
                let card = div()
                    .id(SharedString::from(format!("card-{}", r.key)))
                    .p_3()
                    .rounded_md()
                    .bg(color(if sel { SELECTED } else { CONTENT }))
                    .border_1()
                    .border_color(color(if sel { BORDER_STRONG } else { BORDER }))
                    .cursor_grab()
                    .hover(|s| s.border_color(color(BORDER_STRONG)))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(color(TEXT_4))
                            .child(r.key.clone())
                            .child(div().flex_1())
                            .when(blocked, |d| d.child("Blocked")),
                    )
                    .child(
                        div()
                            .text_color(color(if blocked { TEXT_3 } else { TEXT }))
                            .child(r.title.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(priority_mark(r.priority))
                            .child(state_mark(Kind::Task, r.state)),
                    )
                    .on_drag(Dragged::of(r), |d, _, _, cx| cx.new(|_| d.clone()))
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

    // ---- rendering: the detail panel ----------------------------------------------------------

    fn detail(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let d = self.model.detail.clone()?;
        let n = &d.node;
        let mut choice_row = div().flex().flex_wrap().gap_2();
        for (i, c) in d.choices.iter().enumerate() {
            let c2 = c.clone();
            let primary = c.decides && !matches!(c.to, State::Rejected | State::Retired);
            let hint = match (i, c.always, c.to) {
                (_, true, _) => Some("S"),
                (_, _, State::Rejected) => Some("X"),
                (0, _, _) if primary => Some("A"),
                _ => None,
            };
            choice_row = choice_row.child(
                div()
                    .id(SharedString::from(format!("choice-{i}")))
                    .h(px(28.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(primary, |d| {
                        d.bg(color(ACCENT))
                            .text_color(color(TEXT))
                            .hover(|s| s.bg(color(ACCENT_HOVER)))
                    })
                    .when(!primary, |d| {
                        d.bg(color(SURFACE))
                            .border_1()
                            .border_color(color(BORDER_STRONG))
                            .text_color(color(TEXT_2))
                            .hover(|s| s.bg(color(HOVER)))
                    })
                    .child(c.label)
                    .when_some(hint, |d, h| {
                        d.child(div().text_xs().text_color(with_alpha(TEXT, 0.6)).child(h))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(&c2, cx))),
            );
        }
        let mut props = div().flex().flex_col().gap_2().child(prop_row(
            "Status",
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(state_mark(n.kind, n.state))
                .child(title_case(n.state.as_str())),
        ));
        if n.kind == Kind::Task {
            props = props.child(prop_row(
                "Priority",
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(priority_mark(n.priority))
                    .child(PRIORITY_NAMES[usize::from(n.priority.clamp(1, 4) - 1)]),
            ));
        } else {
            props = props.child(prop_row("Kind", div().child(title_case(n.kind.as_str()))));
        }
        if n.props.get("scope").and_then(|v| v.as_str()) == Some("always") {
            props = props.child(prop_row("Scope", tag("Standing rule", DONE)));
        }
        props = props.child(prop_row(
            "Created",
            div().child(format!("{} by {}", ago(&n.created), who(&n.created_by))),
        ));
        let mut links = div().flex().flex_col().gap_3();
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
        let mut timeline = div().flex().flex_col().gap_3();
        for e in d.history.iter().rev() {
            timeline = timeline.child(event_line(e));
        }
        let panel = div()
            .id(SharedString::from(format!("detail-{}", n.key)))
            .w(px(480.))
            .flex_none()
            .h_full()
            .bg(color(SURFACE))
            .border_l_1()
            .border_color(color(BORDER))
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
                    .child(div().text_color(color(TEXT_3)).child(n.key.clone()))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("close-detail")
                            .h(px(24.))
                            .px_2()
                            .flex()
                            .items_center()
                            .rounded_md()
                            .text_color(color(TEXT_4))
                            .cursor_pointer()
                            .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_3)))
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
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color(TEXT))
                            .child(n.title.clone()),
                    )
                    .when(!d.choices.is_empty(), |d| d.child(choice_row))
                    .child(props)
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

    // ---- rendering: overlays ------------------------------------------------------------------

    /// A modal: a dim backdrop that closes it when clicked, around a card sized to its content.
    fn modal(&self, id: &'static str, card: Div, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id(id)
            .absolute()
            .inset_0()
            .key_context(FORM)
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.close_overlays(window, cx)))
            .on_action(
                cx.listener(|this, _: &input::Escape, window, cx| this.close_overlays(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Submit, window, cx| this.submit_form(window, cx)))
            .bg(with_alpha(0x000000, 0.6))
            .flex()
            .flex_col()
            .items_center()
            .pt(px(110.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_overlays(window, cx)),
            )
            .child(
                card.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .rounded_lg()
                    .bg(color(SURFACE))
                    .border_1()
                    .border_color(color(BORDER_STRONG))
                    .shadow_lg(),
            )
            .with_animation(
                SharedString::from(format!("{id}-in")),
                Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
                |el, t| el.opacity(t),
            )
            .into_any_element()
    }

    fn form_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.form.as_ref()?;
        let mut priorities = div().flex().gap_1();
        for p in 1..=4u8 {
            let on = form.priority == p;
            priorities = priorities.child(
                div()
                    .id(SharedString::from(format!("prio-{p}")))
                    .h(px(28.))
                    .px_2()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .border_1()
                    .when(on, |s| {
                        s.bg(color(SELECTED))
                            .border_color(color(BORDER_STRONG))
                            .text_color(color(TEXT))
                    })
                    .when(!on, |s| {
                        s.border_color(color(BORDER))
                            .text_color(color(TEXT_3))
                            .hover(|h| h.bg(color(HOVER)))
                    })
                    .child(priority_mark(p))
                    .child(PRIORITY_NAMES[usize::from(p - 1)])
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(f) = &mut this.form {
                            f.priority = p;
                        }
                        cx.notify();
                    })),
            );
        }
        let card = div()
            .w(px(640.))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(color(TEXT_3))
                    .child(
                        self.model
                            .project()
                            .map(|p| p.name.clone())
                            .unwrap_or_default(),
                    )
                    .child("›")
                    .child("New task"),
            )
            .child(Input::new(&form.title).appearance(false).text_lg())
            .child(Textarea::new(&form.body).appearance(false))
            .child(div().h(px(1.)).bg(color(BORDER)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(priorities)
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(color(TEXT_4))
                            .child("Starts in the backlog"),
                    )
                    .child(
                        div()
                            .id("create-task")
                            .h(px(28.))
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded_md()
                            .bg(color(ACCENT))
                            .text_color(color(TEXT))
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .hover(|s| s.bg(color(ACCENT_HOVER)))
                            .child("Create task")
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(with_alpha(TEXT, 0.7))
                                    .child("Ctrl ↵"),
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.submit_form(window, cx)),
                            ),
                    ),
            );
        Some(self.modal("new-task-modal", card, cx))
    }

    fn help_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.help {
            return None;
        }
        let mut sections = div().flex().flex_col().gap_4();
        let mut current = "";
        let mut block = div().flex().flex_col();
        for (section, keys, what) in SHORTCUTS {
            if *section != current {
                if !current.is_empty() {
                    sections = sections.child(block);
                }
                block = div().flex().flex_col().child(
                    div()
                        .pb_1()
                        .text_xs()
                        .text_color(color(TEXT_4))
                        .child(*section),
                );
                current = section;
            }
            let mut combo = div().flex().items_center().gap_1();
            for k in *keys {
                combo = if matches!(*k, "then" | "or" | "+") {
                    combo.child(div().text_xs().text_color(color(TEXT_4)).child(*k))
                } else {
                    combo.child(kbd(k))
                };
            }
            block = block.child(
                div()
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(color(BORDER))
                    .child(div().flex_1().text_color(color(TEXT_2)).child(*what))
                    .child(combo),
            );
        }
        sections = sections.child(block);
        let card = div()
            .w(px(520.))
            .max_h(px(620.))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(46.))
                    .px_4()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(color(BORDER))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(TEXT))
                            .child("Keyboard shortcuts"),
                    )
                    .child(kbd("Esc")),
            )
            .child(
                div()
                    .id("shortcuts-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_4()
                    .child(sections),
            );
        Some(self.modal("shortcuts-modal", card, cx))
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
                .bg(color(LIFTED))
                .border_1()
                .border_color(with_alpha(RED, 0.5))
                .text_color(color(TEXT))
                .shadow_lg()
                .child(text)
                .into_any_element(),
        )
    }
}

impl Render for LivtkApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = self.sidebar(window, cx).into_any_element();
        let header = self.header(cx).into_any_element();
        let toolbar = self.toolbar(cx).into_any_element();
        let content = self.content(window, cx);
        let detail = self.detail(cx);
        let form = self.form_overlay(cx);
        let help = self.help_overlay(cx);
        let notice = self.notice();
        let list_area = div()
            .flex_1()
            .min_h_0()
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
            .on_action(cx.listener(|this, _: &Close, window, cx| {
                if this.help || this.form.is_some() {
                    this.close_overlays(window, cx);
                } else {
                    this.model.close_detail();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &Decide, _, cx| {
                this.model.decide();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Standing, _, cx| this.choice_where(|c| c.always, cx)))
            .on_action(cx.listener(|this, _: &Reject, _, cx| {
                this.choice_where(|c| c.to == State::Rejected, cx)
            }))
            .on_action(cx.listener(|this, _: &NewTask, window, cx| this.open_form(window, cx)))
            .on_action(cx.listener(|this, _: &OpenFolder, _, cx| this.open_folder(cx)))
            .on_action(cx.listener(|this, _: &FocusFilter, window, cx| {
                this.filter.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
            }))
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
            .on_action(cx.listener(|this, _: &GoInbox, w, cx| this.go(View::Inbox, w, cx)))
            .on_action(cx.listener(|this, _: &GoTasks, w, cx| this.go(View::Tasks, w, cx)))
            .on_action(cx.listener(|this, _: &GoLessons, w, cx| this.go(View::Lessons, w, cx)))
            .on_action(cx.listener(|this, _: &GoDecisions, w, cx| this.go(View::Decisions, w, cx)))
            .on_action(cx.listener(|this, _: &GoFindings, w, cx| this.go(View::Findings, w, cx)))
            .on_action(cx.listener(|this, _: &PrevProject, window, cx| {
                let i = this.model.current.saturating_sub(1);
                this.model.select_project(i);
                this.sync_filter(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NextProject, window, cx| {
                let i = this.model.current + 1;
                this.model.select_project(i);
                this.sync_filter(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Priority1, _, cx| prio(this, 1, cx)))
            .on_action(cx.listener(|this, _: &Priority2, _, cx| prio(this, 2, cx)))
            .on_action(cx.listener(|this, _: &Priority3, _, cx| prio(this, 3, cx)))
            .on_action(cx.listener(|this, _: &Priority4, _, cx| prio(this, 4, cx)))
            .child(content);
        div()
            .size_full()
            .relative()
            .flex()
            .bg(color(WINDOW))
            .text_color(color(TEXT_2))
            .text_sm()
            .on_action(cx.listener(|this, _: &ToggleHelp, window, cx| {
                this.help = !this.help;
                if !this.help {
                    this.nav.focus(window, cx);
                }
                cx.notify();
            }))
            .child(sidebar)
            .child(
                div().flex_1().min_w_0().h_full().py_2().pr_2().child(
                    div()
                        .size_full()
                        .flex()
                        .rounded_lg()
                        .border_1()
                        .border_color(color(BORDER))
                        .bg(color(CONTENT))
                        .overflow_hidden()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .flex()
                                .flex_col()
                                .child(header)
                                .child(toolbar)
                                .child(list_area),
                        )
                        .children(detail),
                ),
            )
            .children(form)
            .children(help)
            .children(notice)
    }
}

fn prio(app: &mut LivtkApp, p: u8, cx: &mut Context<LivtkApp>) {
    app.model.set_priority(p);
    cx.notify();
}

enum Item {
    Band(usize),
    Row(usize),
}

// ---- small pieces ---------------------------------------------------------------------------------

fn section_label(text: &str) -> Div {
    div()
        .px_2()
        .h(px(24.))
        .flex()
        .items_center()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color(TEXT_4))
        .child(text.to_owned())
}

fn toolbar_label(text: &'static str) -> Div {
    div().text_xs().text_color(color(TEXT_4)).child(text)
}

fn nav_item(id: SharedString, label: &str, active: bool, count: usize) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .h(px(28.))
        .rounded_md()
        .flex()
        .items_center()
        .cursor_pointer()
        .when(active, |d| d.bg(color(SELECTED)).text_color(color(TEXT)))
        .when(!active, |d| {
            d.text_color(color(TEXT_3))
                .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_2)))
        })
        .child(div().flex_1().truncate().child(label.to_owned()))
        .when(count > 0, |d| {
            d.child(
                div()
                    .min_w(px(18.))
                    .h(px(18.))
                    .px(px(5.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(color(ACCENT))
                    .text_color(color(TEXT))
                    .text_xs()
                    .child(format!("{count}")),
            )
        })
}

fn segments() -> Div {
    div()
        .flex()
        .p(px(2.))
        .gap(px(2.))
        .rounded_md()
        .bg(color(WINDOW))
        .border_1()
        .border_color(color(BORDER))
}

fn segment(id: &'static str, label: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(22.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(4.))
        .text_xs()
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
        .h(px(20.))
        .px(px(7.))
        .flex()
        .flex_none()
        .items_center()
        .rounded_full()
        .text_xs()
        .bg(color(SURFACE))
        .border_1()
        .border_color(color(BORDER_STRONG))
        .text_color(color(TEXT_2))
        .child(div().size(px(6.)).mr(px(6.)).rounded_full().bg(color(c)))
        .child(label.to_owned())
}

fn kbd(k: &str) -> Div {
    div()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(color(LIFTED))
        .border_1()
        .border_color(color(BORDER_STRONG))
        .text_xs()
        .text_color(color(TEXT_3))
        .child(k.to_owned())
}

fn prop_row(label: &str, value: Div) -> Div {
    div()
        .flex()
        .items_center()
        .min_h(px(26.))
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .text_color(color(TEXT_4))
                .child(label.to_owned()),
        )
        .child(value.text_color(color(TEXT_2)))
}

/// A group's mark and name, for list bands and board columns alike.
fn group_label(key: GroupKey, kind: Kind) -> (Div, String) {
    match key {
        GroupKey::State(s) => (state_mark(kind, s), title_case(s.as_str())),
        GroupKey::Priority(p) => (
            priority_mark(p),
            PRIORITY_NAMES[usize::from(p.clamp(1, 4) - 1)].to_owned(),
        ),
        GroupKey::All => (div(), "All".to_owned()),
    }
}

fn title_case(s: &str) -> String {
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
                .text_color(color(TEXT_4))
                .child(hint.to_owned()),
        )
}

fn link_group(label: &str, briefs: &[Brief]) -> Div {
    let mut g = div().flex().flex_col().gap_1().child(
        div()
            .text_xs()
            .text_color(color(TEXT_4))
            .child(label.to_owned()),
    );
    for b in briefs {
        g = g.child(
            div()
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .bg(color(CONTENT))
                .border_1()
                .border_color(color(BORDER))
                .child(state_mark(Kind::Task, b.state))
                .child(div().text_color(color(TEXT_4)).child(b.key.clone()))
                .child(
                    div()
                        .truncate()
                        .text_color(color(TEXT_2))
                        .child(b.title.clone()),
                ),
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

fn who(by: &str) -> &str {
    if by == "human" { "you" } else { by }
}

fn event_line(e: &Event) -> Div {
    let when = ago(&e.at);
    let what = match &e.op {
        Op::NodeCreate { .. } => "created it".to_owned(),
        Op::NodeSet { data, .. } => {
            let mut parts = Vec::new();
            if let Some(s) = data.state {
                parts.push(format!("moved it to {}", title_case(s.as_str())));
            }
            if data.title.is_some() {
                parts.push("renamed it".into());
            }
            if data.body.is_some() {
                parts.push("edited the plan".into());
            }
            if let Some(p) = data.priority {
                parts.push(format!(
                    "set priority to {}",
                    PRIORITY_NAMES[usize::from(p.get().clamp(1, 4) - 1)]
                ));
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
    let head = div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(18.))
                .flex_none()
                .rounded_full()
                .bg(color(if human { ACCENT } else { BORDER_STRONG }))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.))
                .text_color(color(TEXT))
                .child(if human { "Y" } else { "C" }),
        )
        .child(
            div()
                .text_color(color(TEXT_2))
                .font_weight(FontWeight::MEDIUM)
                .child(who(&e.by).to_owned()),
        )
        .when(!note, |d| {
            d.child(div().text_color(color(TEXT_3)).child(what.clone()))
        })
        .child(
            div()
                .text_xs()
                .text_color(color(TEXT_4))
                .child(format!("· {when}")),
        );
    div().flex().flex_col().gap_2().child(head).when(note, |d| {
        d.child(
            div()
                .ml(px(26.))
                .p_3()
                .rounded_md()
                .bg(color(CONTENT))
                .border_1()
                .border_color(color(BORDER))
                .text_color(color(TEXT))
                .child(what),
        )
    })
}

/// "just now", "5m", "3h", "2d", or the date, as Linear shows ages.
fn ago(at: &str) -> String {
    let Ok(t) = at.parse::<jiff::Timestamp>() else {
        return at.to_owned();
    };
    let secs = jiff::Timestamp::now().as_second() - t.as_second();
    match secs {
        ..60 => "just now".into(),
        60..3600 => format!("{}m", secs / 60),
        3600..86400 => format!("{}h", secs / 3600),
        86400..604800 => format!("{}d", secs / 86400),
        _ => at.get(..10).unwrap_or(at).to_owned(),
    }
}
