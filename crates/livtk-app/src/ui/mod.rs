//! The window: a sidebar on the black canvas, an inset content panel (header, display toolbar,
//! the list or board), the detail panel, and overlays (new task, shortcuts, notices). It renders
//! the model and forwards input to it; no behaviour lives here.
//!
//! Keys are bound only on the list area. Text fields (the filter, notes, the new-task form) live
//! outside it, so typing never triggers a command.

mod theme;

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::Sizable;
use gpui_kit::component::input::{self, Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use livtk_core::event::{Event, Op};
use livtk_core::{Kind, State, Summary};
use uuid::Uuid;

use crate::model::{
    Ask, CARD_H, CARD_W, Choice, DOTS_BELOW, Detail, Display, FeedRow, GroupKey, Grouping,
    Item as PaletteItem, Layout, LinkChoice, LinkGroup, Model, Ordering, Step, View, choices,
    describe, drop_allowed, has_flow, link_choices, link_curve, state_name, state_short, steps,
    visible_runs,
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
        GoActivity,
        GoTasks,
        GoLessons,
        GoDecisions,
        GoFindings,
        GoGraph,
        ZoomIn,
        ZoomOut,
        ZoomFit,
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
        OpenPalette,
        PaletteUp,
        PaletteDown,
        Submit,
        Cancel
    ]
);

const NAV: &str = "LivtkNav";
const FORM: &str = "LivtkForm";
const PALETTE: &str = "LivtkPalette";
/// Inside the palette's field: arrows move the highlight instead of the cursor.
const PALETTE_FIELD: &str = "LivtkPalette > Input";

const POLL: Duration = Duration::from_millis(200);
const FADE: Duration = Duration::from_millis(180);
const FLASH: Duration = Duration::from_millis(900);
/// The detail panel opening and closing: it grows from the right edge and the list makes room.
const SLIDE: Duration = Duration::from_millis(240);
const SLIDE_OUT: Duration = Duration::from_millis(200);
const DETAIL_WIDTH: f32 = 480.;
/// How much `+` and `-` zoom the graph.
const ZOOM_STEP: f32 = 1.25;
/// An arrowhead on screen: its tip, and the way it points.
type Arrowhead = ((f32, f32), (f32, f32));

/// The ways a link is drawn: touching the focus; ordering work, plain and dimmed; other
/// links, plain and dimmed.
const LINK_STYLES: usize = 5;
/// The graph's background dots, in world units.
const GRID_STEP: f32 = 24.;
/// The detail title's line, shared by the title and its editor.
const TITLE_LINE: f32 = 28.;
/// The horizontal padding GPUI Component gives an extra-small textarea.
const TEXTAREA_PX: Pixels = px(4.);

/// Quick to start, gentle to land: the panel opening.
fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Every shortcut, in the order the shortcuts modal shows them: (section, keys, what it does).
/// In `keys`, the words "then", "or" and "+" are separators; everything else is a key cap.
const SHORTCUTS: &[(&str, &[&str], &str)] = &[
    (
        "Navigate",
        &["Ctrl", "+", "K"],
        "Command palette and search",
    ),
    ("Navigate", &["J", "or", "↓"], "Next row"),
    ("Navigate", &["K", "or", "↑"], "Previous row"),
    ("Navigate", &["Enter"], "Open the detail panel"),
    ("Navigate", &["Esc"], "Close the panel or a modal"),
    ("Navigate", &["G", "then", "I"], "Inbox"),
    ("Navigate", &["G", "then", "A"], "Activity"),
    ("Navigate", &["G", "then", "T"], "Tasks"),
    ("Navigate", &["G", "then", "L"], "Lessons"),
    ("Navigate", &["G", "then", "D"], "Decisions"),
    ("Navigate", &["G", "then", "F"], "Findings"),
    ("Navigate", &["G", "then", "R"], "Graph"),
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
    ("Display", &["/"], "Filter menu"),
    ("Display", &["B"], "List or board"),
    (
        "Display",
        &["H"],
        "Show dismissed: canceled, rejected, retired",
    ),
    ("Display", &["O"], "Open a project folder"),
    ("Display", &["Ctrl", "+", "?"], "These shortcuts"),
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
        KeyBinding::new("g a", GoActivity, nav),
        KeyBinding::new("g t", GoTasks, nav),
        KeyBinding::new("g l", GoLessons, nav),
        KeyBinding::new("g d", GoDecisions, nav),
        KeyBinding::new("g f", GoFindings, nav),
        KeyBinding::new("g r", GoGraph, nav),
        KeyBinding::new("+", ZoomIn, nav),
        KeyBinding::new("=", ZoomIn, nav),
        KeyBinding::new("-", ZoomOut, nav),
        KeyBinding::new("0", ZoomFit, nav),
        KeyBinding::new("[", PrevProject, nav),
        KeyBinding::new("]", NextProject, nav),
        KeyBinding::new("1", Priority1, nav),
        KeyBinding::new("2", Priority2, nav),
        KeyBinding::new("3", Priority3, nav),
        KeyBinding::new("4", Priority4, nav),
        // Everywhere, typing included: Ctrl+? is never text. Keyboards report it either as the
        // `?` character or as Shift with `/`, so both spellings are bound.
        KeyBinding::new("ctrl-?", ToggleHelp, None),
        KeyBinding::new("ctrl-shift-/", ToggleHelp, None),
        KeyBinding::new("ctrl-k", OpenPalette, None),
        // Bound after GPUI Component's own, so in the palette's field they win over the cursor.
        KeyBinding::new("up", PaletteUp, Some(PALETTE_FIELD)),
        KeyBinding::new("down", PaletteDown, Some(PALETTE_FIELD)),
        KeyBinding::new("ctrl-p", PaletteUp, Some(PALETTE_FIELD)),
        KeyBinding::new("ctrl-n", PaletteDown, Some(PALETTE_FIELD)),
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
            .rounded_lg()
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

/// A title or body being edited in the detail panel, for the node it belongs to.
struct Editing {
    node: Uuid,
    field: Field,
    /// Saves when the field loses focus; dropped with the edit, so a cancel saves nothing.
    _subscription: Subscription,
}

enum Field {
    Title(Entity<InputState>),
    Body(Entity<TextareaState>),
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
    /// The command palette's field; what it holds lives in the model.
    palette_input: Entity<InputState>,
    palette_scroll: ScrollHandle,
    /// The board's sideways scroll, which Shift+wheel drives.
    board_scroll: ScrollHandle,
    /// Where the graph was drawn last frame, in window coordinates: its size fits the camera,
    /// its origin turns pointer positions into the graph's own.
    graph_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// A press on the graph's background: where the pointer was last, and whether it moved,
    /// which makes it a drag rather than a click.
    graph_press: Option<(Point<Pixels>, bool)>,
    form: Option<NewTaskForm>,
    help: bool,
    editing: Option<Editing>,
    /// The detail panel's last frame while open, kept to animate it out after it closes.
    last_detail: Option<Detail>,
    /// The panel closing, drawn until its animation ends.
    detail_leaving: Option<Detail>,
    /// Counts openings and closings, so each plays its animation once and switching rows in an
    /// open panel plays none.
    detail_epoch: usize,
    menu: Option<Menu>,
    /// A menu opened from inside `menu`: Display's Grouping or Ordering.
    sub: Option<Menu>,
    /// The menu the mouse-down being dispatched closed, if any: see `toggle_menu`.
    menu_closed: Option<Menu>,
    _subscriptions: Vec<Subscription>,
    _poll: Task<()>,
}

impl LivtkApp {
    pub fn new(model: Model, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let nav = cx.focus_handle();
        let note = cx.new(|cx| InputState::new(window, cx).placeholder("Leave a note…"));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter…"));
        let palette_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type a command or search…"));
        let subscriptions = vec![
            cx.subscribe_in(&note, window, Self::on_note),
            cx.subscribe_in(&filter, window, Self::on_filter),
            cx.subscribe_in(&palette_input, window, Self::on_palette_input),
            // Focus can end up nowhere: the focused input was in a panel that closed, or an
            // editor that was dropped. Keys would then reach none of the app's handlers, so it
            // goes back to where the human is working.
            cx.on_focus_lost(window, |this, window, cx| this.restore_focus(window, cx)),
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
            palette_input,
            palette_scroll: ScrollHandle::new(),
            board_scroll: ScrollHandle::new(),
            graph_bounds: Rc::new(Cell::new(Bounds::default())),
            graph_press: None,
            filter,
            note,
            form: None,
            help: false,
            editing: None,
            last_detail: None,
            detail_leaving: None,
            detail_epoch: 0,
            menu: None,
            sub: None,
            menu_closed: None,
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

    /// The innermost open menu.
    pub fn menu_open(&self) -> Option<&'static str> {
        self.sub.or(self.menu).map(|m| match m {
            Menu::Filter => "filter",
            Menu::Display => "display",
            Menu::Grouping => "grouping",
            Menu::Ordering => "ordering",
            Menu::Priority => "priority",
            Menu::RowPriority(_) => "row-priority",
            Menu::DetailPriority => "detail-priority",
            Menu::AddLink => "add-link",
            Menu::RowState(_) => "row-state",
            Menu::Standing => "standing",
        })
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
            InputEvent::PressEnter { .. } => {
                self.menu = None;
                self.nav.focus(window, cx);
                cx.notify();
            }
            _ => {}
        }
    }

    fn on_palette_input(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change if self.model.palette.is_some() => {
                let text = state.read(cx).value().to_string();
                self.model.palette_query(&text);
                self.palette_scroll.scroll_to_item(0);
                cx.notify();
            }
            InputEvent::PressEnter { .. } => self.run_palette(None, window, cx),
            _ => {}
        }
    }

    /// Give the keys to what is in front: the new-task form, the palette, else the list.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = &self.form {
            form.title.read(cx).focus_handle(cx).focus(window, cx);
        } else if self.model.palette.is_some() {
            self.palette_input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        } else {
            self.nav.focus(window, cx);
        }
    }

    /// Set the graph's zoom at its centre, for benchmarks.
    #[doc(hidden)]
    pub fn zoom_for_test(&mut self, zoom: f32) {
        let z = self.model.graph.camera.zoom;
        self.model.graph_zoom(zoom / z, None);
    }

    pub fn palette_open(&self) -> bool {
        self.model.palette.is_some()
    }

    /// Ctrl+K: open the palette over the view, or close it. The new-task form keeps it out,
    /// so unsaved text is never lost behind it.
    fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.palette.is_some() {
            self.close_overlays(window, cx);
            return;
        }
        if self.form.is_some() {
            return;
        }
        self.help = false;
        self.menu = None;
        self.sub = None;
        self.model.open_palette();
        self.reset_palette_input(window, cx);
        cx.notify();
    }

    /// An empty, focused palette field, scrolled to the top.
    fn reset_palette_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.palette_input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        self.palette_scroll.scroll_to_item(0);
    }

    /// Start adding a link from the node open in the panel: the palette picks the other end.
    fn pick_link(&mut self, choice: LinkChoice, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
        self.help = false;
        self.model.open_link_picker(choice);
        if self.model.palette.is_some() {
            self.reset_palette_input(window, cx);
        }
        cx.notify();
    }

    fn move_palette(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.model.palette_move(delta);
        if let Some(p) = &self.model.palette {
            self.palette_scroll
                .scroll_to_item(palette_child(&p.items, p.selected));
        }
        cx.notify();
    }

    /// Run the highlighted item, or item `index` when clicked, then do what only the view can.
    fn run_palette(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let ask = match index {
            Some(i) => self.model.palette_run_at(i),
            None => self.model.palette_run(),
        };
        if ask == Some(Ask::PickLink) {
            // The palette stays, now picking the link's other end, from an empty field.
            self.reset_palette_input(window, cx);
            cx.notify();
            return;
        }
        self.nav.focus(window, cx);
        // Going to a node may have cleared the filter that hid it.
        self.sync_filter(window, cx);
        match ask {
            Some(Ask::NewTask) => self.open_form(window, cx),
            Some(Ask::AddNote) => {
                if self.model.detail.is_none() {
                    self.model.open_detail();
                }
                self.note.read(cx).focus_handle(cx).focus(window, cx);
            }
            Some(Ask::EditTitle | Ask::EditBody) => {
                if self.model.detail.is_none() {
                    self.model.open_detail();
                }
                self.start_edit(ask == Some(Ask::EditBody), window, cx);
            }
            Some(Ask::OpenFolder) => self.open_folder(cx),
            Some(Ask::Help) => self.help = true,
            Some(Ask::PickLink) | None => {}
        }
        self.after_move(cx);
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
        if self.model.view == View::Activity {
            let at = self
                .model
                .feed()
                .iter()
                .position(|r| *r == FeedRow::Event(self.model.activity_index));
            if let Some(i) = at {
                self.list_scroll.scroll_to_item(i, ScrollStrategy::Center);
            }
        } else if let Some(i) = self.display_index() {
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

    pub fn editing(&self) -> Option<&'static str> {
        self.editing.as_ref().map(|e| match e.field {
            Field::Title(_) => "title",
            Field::Body(_) => "body",
        })
    }

    /// Edit the open node's title or body in place, as in Linear: Enter (Ctrl+Enter for the
    /// body) or clicking away saves, Esc cancels.
    fn start_edit(&mut self, body: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.model.detail.as_ref().map(|d| d.node.clone()) else {
            return;
        };
        let (field, focus, subscription) = if body {
            let state = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("Add a description…")
                    .auto_grow(4, 24)
                    .default_value(node.body.clone())
            });
            // The textarea takes Ctrl+Enter itself and reports it as a secondary Enter.
            let sub = cx.subscribe_in(&state, window, |this, _, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::PressEnter {
                        secondary: true, ..
                    } => this.save_edit(true, window, cx),
                    InputEvent::Blur => this.save_edit(false, window, cx),
                    _ => {}
                }
            });
            let focus = state.read(cx).focus_handle(cx);
            (Field::Body(state), focus, sub)
        } else {
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Title")
                    .default_value(node.title.clone())
            });
            let sub = cx.subscribe_in(&state, window, |this, _, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::PressEnter { .. } => this.save_edit(true, window, cx),
                    InputEvent::Blur => this.save_edit(false, window, cx),
                    _ => {}
                }
            });
            let focus = state.read(cx).focus_handle(cx);
            (Field::Title(state), focus, sub)
        };
        // Whatever was being edited before is saved first.
        self.save_edit(false, window, cx);
        focus.focus(window, cx);
        self.editing = Some(Editing {
            node: node.id,
            field,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Save the edit, if any; `refocus` gives the keys back to the list.
    fn save_edit(&mut self, refocus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        match &edit.field {
            Field::Title(state) => {
                let title = state.read(cx).value().to_string();
                self.model.edit(edit.node, Some(&title), None);
            }
            Field::Body(state) => {
                // Trailing blank lines carry nothing (Ctrl+Enter itself adds one).
                let body = state.read(cx).value().trim_end().to_owned();
                self.model.edit(edit.node, None, Some(&body));
            }
        }
        if refocus {
            self.nav.focus(window, cx);
        }
        cx.notify();
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            self.nav.focus(window, cx);
            cx.notify();
        }
    }

    /// The open node's title: double-click to edit.
    fn detail_title(&self, n: &livtk_core::Node, cx: &mut Context<Self>) -> AnyElement {
        if let Some(Editing {
            node,
            field: Field::Title(state),
            ..
        }) = &self.editing
            && *node == n.id
        {
            // The same box as the title it replaces, so the text does not move: the input's own
            // padding is dropped and its line height matches.
            return div()
                .debug_selector(|| "title-editor".into())
                .mx(px(-8.))
                .px(px(8.))
                .py(px(2.))
                .rounded_xl()
                .bg(color(HOVER))
                .on_action(
                    cx.listener(|this, _: &input::Escape, window, cx| this.cancel_edit(window, cx)),
                )
                .child(flush_input(
                    Input::new(state)
                        .line_height(px(TITLE_LINE))
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD),
                    TITLE_LINE,
                ))
                .into_any_element();
        }
        div()
            .id("detail-title")
            .debug_selector(|| "detail-title".into())
            .mx(px(-8.))
            .px(px(8.))
            .py(px(2.))
            .rounded_xl()
            .cursor_text()
            .hover(|s| s.bg(color(HOVER)))
            .line_height(px(TITLE_LINE))
            .text_xl()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(color(TEXT))
            .child(n.title.clone())
            .on_click(cx.listener(|this, e: &ClickEvent, window, cx| {
                if e.click_count() >= 2 {
                    this.start_edit(false, window, cx);
                }
            }))
            .into_any_element()
    }

    /// The open node's body as markdown: double-click to edit; when empty, one click.
    fn detail_body(&self, n: &livtk_core::Node, cx: &mut Context<Self>) -> AnyElement {
        if let Some(Editing {
            node,
            field: Field::Body(state),
            ..
        }) = &self.editing
            && *node == n.id
        {
            return div()
                .debug_selector(|| "body-editor".into())
                .key_context(FORM)
                .mx(px(-8.))
                // The view's 8 px all round, less the textarea's own padding (4 px across at
                // this size, none down), so the text stays where it was.
                .px(px(8.) - TEXTAREA_PX)
                .py(px(8.))
                .rounded_xl()
                .bg(color(HOVER))
                .on_action(
                    cx.listener(|this, _: &Submit, window, cx| this.save_edit(true, window, cx)),
                )
                .on_action(cx.listener(|this, _: &Cancel, window, cx| this.cancel_edit(window, cx)))
                .on_action(
                    cx.listener(|this, _: &input::Escape, window, cx| this.cancel_edit(window, cx)),
                )
                .child(Textarea::new(state).appearance(false).xsmall().text_sm())
                .into_any_element();
        }
        let empty = n.body.trim().is_empty();
        div()
            .id("detail-body")
            .debug_selector(|| "detail-body".into())
            .mx(px(-8.))
            .p(px(8.))
            .rounded_xl()
            .cursor_text()
            .hover(|s| s.bg(color(HOVER)))
            .text_color(color(TEXT_2))
            .when(empty, |d| {
                d.text_color(color(TEXT_4)).child("Add a description…")
            })
            .when(!empty, |d| {
                d.child(TextView::markdown(
                    SharedString::from(format!("body-{}", n.key)),
                    n.body.clone(),
                ))
            })
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                if empty || e.click_count() >= 2 {
                    this.start_edit(true, window, cx);
                }
            }))
            .into_any_element()
    }

    fn close_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model.close_palette();
        self.menu = None;
        self.sub = None;
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
                .rounded_lg()
                .text_color(color(TEXT_4))
                .cursor_pointer()
                .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_3)))
                .child("+ Open project")
                .on_click(cx.listener(|this, _, _, cx| this.open_folder(cx))),
        );
        side = side.child(div().h(px(14.))).child(section_label("Views"));
        let inbox = self.model.project().map(|p| p.inbox).unwrap_or(0);
        let unseen = self.model.project().map(|p| p.unseen).unwrap_or(0);
        for view in View::ALL {
            let count = match view {
                View::Inbox => inbox,
                View::Activity => unseen,
                _ => 0,
            };
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
                .rounded_lg()
                .text_xs()
                .text_color(color(TEXT_4))
                .cursor_pointer()
                .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_3)))
                .child(kbd("Ctrl"))
                .child(kbd("?"))
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
                    .child(if m.view == View::Activity {
                        format!(
                            "{}{}",
                            m.activity.len(),
                            if m.activity_more { "+" } else { "" }
                        )
                    } else if m.view == View::Graph {
                        let links = m.graph.edges.len();
                        format!(
                            "{} · {links} link{}",
                            m.graph.nodes.len(),
                            if links == 1 { "" } else { "s" }
                        )
                    } else {
                        format!("{}", m.filtered.len())
                    }),
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
                .rounded_lg()
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

    /// Open or close a menu from its button. A click on the button of the open menu first
    /// reads as a click outside it, which closes it; that must not reopen it.
    fn toggle_menu(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        let just_closed = self.menu_closed.take() == Some(menu);
        if menu.is_sub() {
            self.sub = (!just_closed && self.sub != Some(menu)).then_some(menu);
        } else if just_closed || self.menu == Some(menu) {
            self.close_menu(window, cx);
        } else {
            self.menu = Some(menu);
            self.sub = None;
            if menu == Menu::Filter {
                self.filter.read(cx).focus_handle(cx).focus(window, cx);
            }
        }
        cx.notify();
    }

    /// Close every menu and give the keys back to where they were: the form, or the list.
    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_none() {
            return;
        }
        self.menu = None;
        self.sub = None;
        match &self.form {
            Some(form) => form.title.read(cx).focus_handle(cx).focus(window, cx),
            None => self.nav.focus(window, cx),
        }
        cx.notify();
    }

    /// Esc closes the innermost thing open: a submenu, a menu, then a modal.
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sub.take().is_some() {
            cx.notify();
        } else if self.menu.is_some() {
            self.close_menu(window, cx);
        } else {
            self.close_overlays(window, cx);
        }
    }

    /// A mouse-down outside `menu`. A submenu closes alone; its parent stays open for the next
    /// choice, as in Linear. The flag lives only for this mouse-down: see `toggle_menu`.
    fn clicked_outside(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        let sub_closing = self.menu_closed.is_some_and(Menu::is_sub);
        if !menu.is_sub() && (self.sub.is_some() || sub_closing) {
            return;
        }
        self.menu_closed = Some(menu);
        cx.defer_in(window, |this, _, _| this.menu_closed = None);
        if menu.is_sub() {
            self.sub = None;
            cx.notify();
        } else {
            self.close_menu(window, cx);
        }
    }

    /// `panel` as `menu`'s popover: anchored `top` below its button's left or right edge, and
    /// drawn above everything, submenus above their menu.
    fn popover(
        &self,
        menu: Menu,
        right: bool,
        top: f32,
        panel: Div,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .absolute()
            .top(px(top))
            .when(right, |a| a.right_0())
            .when(!right, |a| a.left_0())
            .child(
                deferred(
                    anchored()
                        .anchor(if right {
                            Anchor::TopRight
                        } else {
                            Anchor::TopLeft
                        })
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            panel
                                .occlude()
                                .on_mouse_down_out(cx.listener(move |this, _, window, cx| {
                                    this.clicked_outside(menu, window, cx)
                                }))
                                .with_animation(
                                    SharedString::from(format!("menu-{menu:?}")),
                                    Animation::new(Duration::from_millis(120))
                                        .with_easing(ease_out_quint()),
                                    |el, t| el.opacity(t).mt(px(-4.0 * (1.0 - t))),
                                ),
                        ),
                )
                .with_priority(if menu.is_sub() { 2 } else { 1 }),
            )
    }

    /// A button that opens a menu of choices, showing the current one, as Linear's selects do.
    fn select(
        &self,
        menu: Menu,
        id: &'static str,
        current: Div,
        panel: impl FnOnce(&Self, &mut Context<Self>) -> Div,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.sub == Some(menu) || self.menu == Some(menu);
        let button = div()
            .id(id)
            .debug_selector(move || id.into())
            .h(px(28.))
            .pl_2()
            .pr(px(6.))
            .flex()
            .items_center()
            .gap_2()
            .rounded_lg()
            .border_1()
            .cursor_pointer()
            .text_color(color(TEXT_2))
            .when(open, |b| {
                b.bg(color(SELECTED)).border_color(color(BORDER_STRONG))
            })
            .when(!open, |b| {
                b.border_color(color(BORDER))
                    .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
            })
            .child(current)
            .child(div().text_xs().text_color(color(TEXT_4)).child("▾"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.toggle_menu(menu, window, cx)),
            );
        let right = menu.is_sub();
        div().relative().child(button).when(open, |d| {
            let panel = panel(self, cx);
            d.child(self.popover(menu, right, 32., panel, cx))
        })
    }

    /// The row under the header: Filter on the left with what it filters, Display on the right.
    /// Both open menus, as in Linear; only List and Board are tabs (in the header).
    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let d: Display = self.model.display();
        let active =
            usize::from(!d.filter.trim().is_empty()) + usize::from(!d.priorities.is_empty());
        let filter_label = if active == 0 {
            "Filter".to_owned()
        } else {
            format!("Filter · {active}")
        };
        let mut row = div()
            .flex_none()
            .h(px(44.))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(color(BORDER))
            .child(self.menu_button(Menu::Filter, "filter-button", filter_label, active > 0, cx));
        if !d.filter.trim().is_empty() {
            let text = format!("Title or key: {}", d.filter.trim());
            row = row.child(chip(
                "clear-text-filter",
                text,
                cx.listener(|this, _, window, cx| {
                    this.model.set_filter("");
                    this.sync_filter(window, cx);
                    cx.notify();
                }),
            ));
        }
        if !d.priorities.is_empty() {
            let names: Vec<&str> = d
                .priorities
                .iter()
                .map(|p| PRIORITY_NAMES[usize::from(p.clamp(&1, &4) - 1)])
                .collect();
            row = row.child(chip(
                "clear-priority-filter",
                format!("Priority: {}", names.join(", ")),
                cx.listener(|this, _, _, cx| {
                    for p in this.model.display().priorities {
                        this.model.toggle_priority(p);
                    }
                    cx.notify();
                }),
            ));
        }
        row.child(div().flex_1()).child(self.menu_button(
            Menu::Display,
            "display-button",
            "Display".to_owned(),
            false,
            cx,
        ))
    }

    /// A toolbar button and, when open, its menu, anchored under it and drawn above everything.
    fn menu_button(
        &self,
        menu: Menu,
        id: &'static str,
        label: String,
        active: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = self.menu == Some(menu);
        let button = div()
            .id(id)
            .debug_selector(move || id.into())
            .h(px(28.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .rounded_lg()
            .border_1()
            .cursor_pointer()
            .when(open || active, |b| {
                b.bg(color(SELECTED))
                    .border_color(color(BORDER_STRONG))
                    .text_color(color(TEXT))
            })
            .when(!open && !active, |b| {
                b.border_color(color(BORDER))
                    .text_color(color(TEXT_3))
                    .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT_2)))
            })
            .child(
                div()
                    .text_xs()
                    .child(if menu == Menu::Filter { "≡" } else { "⚙" }),
            )
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.toggle_menu(menu, window, cx)),
            );
        div().relative().child(button).when(open, |d| {
            let panel = match menu {
                Menu::Filter => self.filter_menu(cx),
                _ => self.display_menu(cx),
            };
            d.child(self.popover(menu, menu == Menu::Display, 32., panel, cx))
        })
    }

    fn filter_menu(&self, cx: &mut Context<Self>) -> Div {
        let d = self.model.display();
        let mut panel = menu_panel(264.)
            .debug_selector(|| "filter-menu".into())
            .on_action(
                cx.listener(|this, _: &input::Escape, window, cx| this.close_menu(window, cx)),
            )
            .child(
                div()
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .child(Input::new(&self.filter).appearance(false).small()),
            )
            .child(menu_divider())
            .child(menu_heading("Priority"));
        for p in 1..=4u8 {
            let on = d.priorities.contains(&p);
            panel = panel.child(
                option_row(SharedString::from(format!("filter-priority-{p}")))
                    .child(checkbox(on))
                    .child(priority_mark(p))
                    .child(PRIORITY_NAMES[usize::from(p - 1)])
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.toggle_priority(p);
                        cx.notify();
                    })),
            );
        }
        if !d.priorities.is_empty() || !d.filter.trim().is_empty() {
            panel = panel.child(menu_divider()).child(
                option_row("clear-filters".into())
                    .text_color(color(TEXT_3))
                    .child("Clear filters")
                    .on_click(cx.listener(|this, _, window, cx| {
                        for p in this.model.display().priorities {
                            this.model.toggle_priority(p);
                        }
                        this.model.set_filter("");
                        this.sync_filter(window, cx);
                        cx.notify();
                    })),
            );
        }
        panel
    }

    /// Grouping and Ordering, each its own menu, and whether dismissed work shows.
    fn display_menu(&self, cx: &mut Context<Self>) -> Div {
        let d = self.model.display();
        let grouping = GROUPINGS
            .iter()
            .find(|g| g.2 == d.grouping)
            .map_or("State", |g| g.1);
        let ordering = ORDERINGS
            .iter()
            .find(|o| o.2 == d.ordering)
            .map_or("Priority", |o| o.1);
        let mut panel = menu_panel(272.)
            .debug_selector(|| "display-menu".into())
            .child(display_field(
                "Grouping",
                self.select(
                    Menu::Grouping,
                    "grouping-select",
                    div().child(grouping),
                    Self::grouping_menu,
                    cx,
                ),
            ))
            .child(display_field(
                "Ordering",
                self.select(
                    Menu::Ordering,
                    "ordering-select",
                    div().child(ordering),
                    Self::ordering_menu,
                    cx,
                ),
            ));
        if self.model.view != View::Inbox {
            let on = self.model.show_all;
            panel = panel.child(menu_divider()).child(
                option_row("show-dismissed".into())
                    .debug_selector(|| "show-dismissed".into())
                    .child(div().flex_1().child("Show dismissed"))
                    .child(switch(on))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.model.toggle_show_all();
                        cx.notify();
                    })),
            );
        }
        panel
    }

    fn grouping_menu(&self, cx: &mut Context<Self>) -> Div {
        let current = self.model.display().grouping;
        // A board needs columns: as in Linear, it is never ungrouped.
        let board = self.model.view == View::Tasks && self.model.layout == Layout::Board;
        let mut panel = menu_panel(180.).debug_selector(|| "grouping-menu".into());
        for &(id, label, g) in GROUPINGS {
            if board && g == Grouping::None {
                continue;
            }
            panel = panel.child(
                option_row(id.into())
                    .debug_selector(move || id.into())
                    .child(label)
                    .child(check(current == g))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.set_grouping(g);
                        this.sub = None;
                        cx.notify();
                    })),
            );
        }
        panel
    }

    fn ordering_menu(&self, cx: &mut Context<Self>) -> Div {
        let current = self.model.display().ordering;
        let mut panel = menu_panel(180.).debug_selector(|| "ordering-menu".into());
        for &(id, label, o) in ORDERINGS {
            panel = panel.child(
                option_row(id.into())
                    .debug_selector(move || id.into())
                    .child(label)
                    .child(check(current == o))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.set_ordering(o);
                        this.sub = None;
                        cx.notify();
                    })),
            );
        }
        panel
    }

    /// A task's priority mark that opens a menu of priorities, as in Linear. A press here is
    /// the button's alone: it neither selects the row nor starts a drag.
    fn priority_button(
        &self,
        menu: Menu,
        id: Uuid,
        current: u8,
        name: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.menu == Some(menu);
        let selector = match menu {
            Menu::DetailPriority => "detail-priority".to_owned(),
            _ => format!("priority-of-{id}"),
        };
        let button = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .h(px(22.))
            .px(px(4.))
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .rounded_lg()
            .cursor_pointer()
            .when(open, |b| b.bg(color(SELECTED)))
            .when(!open, |b| b.hover(|h| h.bg(color(HOVER))))
            .child(priority_mark(current))
            .when(name, |b| {
                b.child(PRIORITY_NAMES[usize::from(current.clamp(1, 4) - 1)])
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_menu(menu, window, cx);
                }),
            );
        div().relative().flex_none().child(button).when(open, |d| {
            let mut panel = menu_panel(180.).debug_selector(|| "set-priority-menu".into());
            for p in 1..=4u8 {
                let row_id = format!("set-priority-{p}");
                panel = panel.child(
                    option_row(SharedString::from(row_id.clone()))
                        .debug_selector(move || row_id.clone())
                        .child(priority_mark(p))
                        .child(PRIORITY_NAMES[usize::from(p - 1)])
                        .child(check(current == p))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.model.set_priority_of(id, p);
                            this.close_menu(window, cx);
                        })),
                );
            }
            d.child(self.popover(menu, false, 26., panel, cx))
        })
    }

    /// A node's state mark that opens the states of its kind, as Linear's status menu: the
    /// moves it can make enabled, the human's own decisions in the accent, the rest greyed so
    /// the whole flow shows. A press here neither selects the row nor starts a drag.
    fn state_button(&self, id: Uuid, kind: Kind, state: State, cx: &mut Context<Self>) -> Div {
        if !has_flow(kind) {
            return div().flex_none().child(state_mark(kind, state));
        }
        let menu = Menu::RowState(id);
        let open = self.menu == Some(menu);
        let selector = format!("state-of-{id}");
        let button = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .size(px(22.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_lg()
            .cursor_pointer()
            .when(open, |b| b.bg(color(SELECTED)))
            .when(!open, |b| b.hover(|h| h.bg(color(HOVER))))
            .child(state_mark(kind, state))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_menu(menu, window, cx);
                }),
            );
        div().relative().flex_none().child(button).when(open, |d| {
            let mut panel = menu_panel(220.).debug_selector(|| "set-state-menu".into());
            let all = steps(kind, state);
            let mut side = false;
            for s in &all {
                if !s.main && !side {
                    panel = panel.child(menu_divider());
                    side = true;
                }
                panel = panel.child(self.state_option(
                    id,
                    kind,
                    s,
                    s.to.clone(),
                    state_name(s.state),
                    cx,
                ));
                if let Some(standing) = &s.standing {
                    panel = panel.child(self.state_option(
                        id,
                        kind,
                        s,
                        Some(standing.clone()),
                        "Ratified, as a standing rule",
                        cx,
                    ));
                }
            }
            d.child(self.popover(menu, false, 26., panel, cx))
        })
    }

    /// A state in the state menu: enabled when `to` is a move the node can make.
    fn state_option(
        &self,
        id: Uuid,
        kind: Kind,
        s: &Step,
        to: Option<Choice>,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let standing = to.as_ref().is_some_and(|c| c.always);
        let row_id = format!(
            "set-state-{}{}",
            s.state.as_str(),
            if standing { "-standing" } else { "" }
        );
        let decides = to.as_ref().is_some_and(|c| c.decides);
        let row = div()
            .id(SharedString::from(row_id.clone()))
            .debug_selector(move || row_id.clone())
            .h(px(30.))
            .px(px(6.))
            .flex()
            .items_center()
            .gap_2()
            .rounded_lg()
            .child(state_mark(kind, s.state))
            .child(label)
            .child(check(s.current && !standing));
        match to {
            Some(choice) => row
                .cursor_pointer()
                .text_color(color(if decides { ACCENT_HOVER } else { TEXT_2 }))
                .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.model.choose_on(id, &choice);
                    this.close_menu(window, cx);
                })),
            // Not a move from here: shown, so the flow reads whole, but greyed.
            None if s.current => row.text_color(color(TEXT)),
            None => row.text_color(color(TEXT_4)).opacity(0.55),
        }
    }

    /// The flow of the open node's states: the main path left to right with arrows, the states
    /// off it below. Where it is now is filled; where it can go is outlined and clickable, in
    /// the accent when that move is the human's own decision; the rest is dim.
    fn state_flow(&self, id: Uuid, kind: Kind, state: State, cx: &mut Context<Self>) -> Div {
        let all = steps(kind, state);
        let mut path = div()
            .debug_selector(|| "state-flow".into())
            .w_full()
            .flex()
            .items_start();
        for (i, s) in all.iter().filter(|s| s.main).enumerate() {
            if i > 0 {
                path = path.child(
                    div()
                        .flex_1()
                        .min_w(px(10.))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .child(div().flex_1().h(px(1.)).bg(color(BORDER_STRONG)))
                        .child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(11.))
                                .text_color(color(TEXT_4))
                                .child("›"),
                        ),
                );
            }
            path = path.child(self.flow_step(id, kind, s, cx));
        }
        let off: Vec<&Step> = all.iter().filter(|s| !s.main).collect();
        let mut out = div().flex().flex_col().gap_3().child(path);
        if !off.is_empty() {
            let mut row = div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().text_xs().text_color(color(TEXT_4)).child("Or"));
            for s in off {
                row = row.child(self.flow_pill(id, kind, s, cx));
            }
            out = out.child(row);
        }
        out
    }

    /// How a step looks for what it is: current, a move, the human's decision, out of reach.
    fn step_style(s: &Step, el: Stateful<Div>) -> Stateful<Div> {
        let reachable = s.to.is_some() || s.standing.is_some();
        if s.current {
            el.bg(color(SELECTED)).border_color(color(BORDER_STRONG))
        } else if reachable && s.decides() {
            el.border_color(color(ACCENT))
                .cursor_pointer()
                .hover(|h| h.bg(with_alpha(ACCENT, 0.16)))
        } else if reachable {
            el.border_color(color(BORDER_STRONG))
                .cursor_pointer()
                .hover(|h| h.bg(color(HOVER)))
        } else {
            el.border_color(color(BORDER)).opacity(0.4)
        }
    }

    /// What a click on a step does: ratify offers the standing-rule choice first.
    fn on_step(
        &self,
        id: Uuid,
        s: &Step,
        el: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let tip: SharedString = match (&s.to, s.current) {
            (Some(c), _) => c.label.into(),
            (None, true) => "Where it is now".into(),
            (None, false) => "Not a move from here".into(),
        };
        let el = el.tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        });
        if s.standing.is_some() {
            el.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_menu(Menu::Standing, window, cx);
                }),
            )
        } else if let Some(choice) = s.to.clone() {
            el.on_click(cx.listener(move |this, _, _, cx| {
                this.model.choose_on(id, &choice);
                cx.notify();
            }))
        } else {
            el
        }
    }

    /// A step on the main path: its mark in a ring, its name below.
    fn flow_step(&self, id: Uuid, kind: Kind, s: &Step, cx: &mut Context<Self>) -> Div {
        let name = s.state.as_str();
        let ring_id = format!("flow-{name}");
        let ring = div()
            .id(SharedString::from(ring_id.clone()))
            .debug_selector(move || ring_id.clone())
            .size(px(26.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .child(state_mark(kind, s.state));
        let ring = self.on_step(id, s, Self::step_style(s, ring), cx);
        let label_id = format!("flow-label-{name}");
        let reachable = s.to.is_some() || s.standing.is_some();
        let label = div()
            .debug_selector(move || label_id.clone())
            .text_xs()
            .whitespace_nowrap()
            .text_color(color(if s.current {
                TEXT
            } else if reachable && s.decides() {
                ACCENT_HOVER
            } else if reachable {
                TEXT_2
            } else {
                TEXT_4
            }))
            .when(s.current, |l| l.font_weight(FontWeight::MEDIUM))
            .child(state_short(s.state));
        let standing_open = self.menu == Some(Menu::Standing) && s.standing.is_some();
        div()
            .relative()
            .w(px(56.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .child(ring)
            .child(label)
            .when(standing_open, |d| {
                let panel = self.standing_menu(id, s, cx);
                d.child(self.popover(Menu::Standing, false, 30., panel, cx))
            })
    }

    /// A state off the main path, as a pill: parked, canceled, rejected.
    fn flow_pill(&self, id: Uuid, kind: Kind, s: &Step, cx: &mut Context<Self>) -> Stateful<Div> {
        let pill_id = format!("flow-{}", s.state.as_str());
        let reachable = s.to.is_some();
        let pill = div()
            .id(SharedString::from(pill_id.clone()))
            .debug_selector(move || pill_id.clone())
            .h(px(26.))
            .px_2()
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_full()
            .border_1()
            .text_xs()
            .text_color(color(if s.current {
                TEXT
            } else if reachable && s.decides() {
                ACCENT_HOVER
            } else if reachable {
                TEXT_2
            } else {
                TEXT_4
            }))
            .child(state_mark(kind, s.state))
            .child(state_name(s.state));
        self.on_step(id, s, Self::step_style(s, pill), cx)
    }

    /// Ratify, or ratify as a standing rule shown at every session start.
    fn standing_menu(&self, id: Uuid, s: &Step, cx: &mut Context<Self>) -> Div {
        let mut panel = menu_panel(240.).debug_selector(|| "standing-menu".into());
        for (row_id, label, choice) in [
            ("ratify-plain", "Ratify", s.to.clone()),
            (
                "ratify-standing",
                "Ratify as a standing rule",
                s.standing.clone(),
            ),
        ] {
            let Some(choice) = choice else { continue };
            panel = panel.child(
                option_row(row_id.into())
                    .debug_selector(move || row_id.into())
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.model.choose_on(id, &choice);
                        this.close_menu(window, cx);
                    })),
            );
        }
        panel
    }

    /// "+ Add link": the relations the open node can gain, each opening the palette to pick
    /// the other end.
    fn add_link_button(&self, kind: Kind, cx: &mut Context<Self>) -> Div {
        let open = self.menu == Some(Menu::AddLink);
        let button = div()
            .id("add-link")
            .debug_selector(|| "add-link".into())
            .h(px(24.))
            .px_2()
            .flex()
            .items_center()
            .rounded_lg()
            .text_xs()
            .cursor_pointer()
            .text_color(color(if open { TEXT } else { TEXT_3 }))
            .when(open, |b| b.bg(color(SELECTED)))
            .when(!open, |b| {
                b.hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
            })
            .child("+ Add link")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.toggle_menu(Menu::AddLink, window, cx)),
            );
        div().relative().child(button).when(open, |d| {
            let mut panel = menu_panel(200.).debug_selector(|| "add-link-menu".into());
            for (i, choice) in link_choices(kind).into_iter().enumerate() {
                let row_id = format!("link-choice-{i}");
                panel = panel.child(
                    option_row(SharedString::from(row_id.clone()))
                        .debug_selector(move || row_id.clone())
                        .child(choice.label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pick_link(choice, window, cx)
                        })),
                );
            }
            d.child(self.popover(Menu::AddLink, true, 28., panel, cx))
        })
    }

    /// One group of the open node's links, each with an × that removes it.
    fn link_group(&self, g: &LinkGroup, cx: &mut Context<Self>) -> Div {
        let mut out = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_xs().text_color(color(TEXT_4)).child(g.label));
        let (rel, outgoing) = (g.rel, g.outgoing);
        let group = g.label.to_lowercase().replace(' ', "-");
        for b in &g.briefs {
            let key = b.key.clone();
            let unlink = format!("unlink-{group}-{key}");
            out = out.child(
                div()
                    .h(px(28.))
                    .pl_2()
                    .pr_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_lg()
                    .bg(color(CONTENT))
                    .border_1()
                    .border_color(color(BORDER))
                    .child(state_mark(Kind::Task, b.state))
                    .child(
                        div()
                            .flex_none()
                            .text_color(color(TEXT_4))
                            .child(b.key.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(color(TEXT_2))
                            .child(b.title.clone()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(unlink.clone()))
                            .debug_selector(move || unlink.clone())
                            .size(px(20.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .text_color(color(TEXT_4))
                            .cursor_pointer()
                            .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT)))
                            .child("×")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.model.unlink(rel, outgoing, &key);
                                cx.notify();
                            })),
                    ),
            );
        }
        out
    }

    /// The new task's priority, chosen from a menu as in Linear's issue composer.
    fn priority_menu(&self, cx: &mut Context<Self>) -> Div {
        let current = self.form.as_ref().map_or(3, |f| f.priority);
        let mut panel = menu_panel(180.).debug_selector(|| "priority-menu".into());
        for p in 1..=4u8 {
            let id = PRIORITY_IDS[usize::from(p - 1)];
            panel = panel.child(
                option_row(id.into())
                    .debug_selector(move || id.into())
                    .child(priority_mark(p))
                    .child(PRIORITY_NAMES[usize::from(p - 1)])
                    .child(check(current == p))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(f) = &mut this.form {
                            f.priority = p;
                        }
                        this.close_menu(window, cx);
                    })),
            );
        }
        panel
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
            (View::Activity, _) => self.feed(cx),
            (View::Graph, _) => self.graph_view(cx),
            (View::Tasks, Layout::Board) => self.board(cx).into_any_element(),
            _ => self.list(window, cx).into_any_element(),
        }
    }

    // ---- rendering: the graph -------------------------------------------------------------------

    fn zoom_graph(&mut self, factor: f32, cx: &mut Context<Self>) {
        if self.model.view == View::Graph {
            self.model.graph_zoom(factor, None);
            cx.notify();
        }
    }

    /// A pointer position in the graph's own coordinates.
    fn graph_point(&self, p: Point<Pixels>) -> (f32, f32) {
        let o = self.graph_bounds.get().origin;
        (f32::from(p.x - o.x), f32::from(p.y - o.y))
    }

    /// The graph: links on a canvas, cards over them, only what is in view. Drag or scroll to
    /// pan, Ctrl+scroll to zoom at the pointer, click a card to open it.
    fn graph_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let drawn = self.graph_bounds.get().size;
        self.model
            .graph_viewport(f32::from(drawn.width), f32::from(drawn.height));
        if self.model.graph.nodes.is_empty() {
            return div()
                .size_full()
                .child(empty(
                    "Nothing to draw yet",
                    "Tasks, lessons, decisions and findings show here with the links between them.",
                ))
                .into_any_element();
        }
        let g = &self.model.graph;
        let cam = g.camera;
        let z = cam.zoom;
        let (vw, vh) = g.viewport();
        // Before the first frame the size is unknown: draw everything once to learn it.
        let known = vw > 0. && vh > 0.;
        let focus = g.focus(self.model.detail.is_some());
        let near = focus.map(|i| g.neighbours(i)).unwrap_or_default();
        // As `Graph::lit`, with the neighbours worked out once for the frame.
        let lit = |i: usize| focus.is_none_or(|f| f == i || near.contains(&i));
        let in_view = |x: f32, y: f32, w: f32, h: f32| {
            !known || (x + w >= 0. && y + h >= 0. && x <= vw && y <= vh)
        };

        // Links as the stretches of their curves that show, gathered by how they are drawn,
        // and the arrowheads whose tips show. A long link costs only what is in view.
        let mut strokes: [Vec<Vec<(f32, f32)>>; LINK_STYLES] = Default::default();
        let mut heads: [Vec<Arrowhead>; LINK_STYLES] = Default::default();
        for e in &g.edges {
            let ((x0, y0), (x1, y1), across) = g.ends(e);
            let (a, b) = (cam.to_screen(x0, y0), cam.to_screen(x1, y1));
            let (lx, ly) = (a.0.min(b.0), a.1.min(b.1));
            if !in_view(lx, ly, (a.0 - b.0).abs(), (a.1 - b.1).abs()) {
                continue;
            }
            let runs = if known {
                visible_runs(&link_curve(a, b, across), vw, vh, 16.)
            } else {
                vec![link_curve(a, b, across)]
            };
            if runs.is_empty() {
                continue;
            }
            let touches = focus.is_some_and(|f| e.from == f || e.to == f);
            let dim = focus.is_some() && !touches;
            let style = match (touches, e.orders(), dim) {
                (true, _, _) => 0,
                (false, true, false) => 1,
                (false, true, true) => 2,
                (false, false, false) => 3,
                (false, false, true) => 4,
            };
            strokes[style].extend(runs);
            if e.orders() && in_view(b.0 - 10., b.1 - 10., 20., 20.) {
                let dir = if across {
                    ((b.0 - a.0).signum(), 0.)
                } else {
                    (0., (b.1 - a.1).signum())
                };
                heads[style].push((b, dir));
            }
        }
        let dots = z < DOTS_BELOW;
        let mut dot_quads = Vec::new();
        let mut cards = Vec::new();
        for (i, n) in g.nodes.iter().enumerate() {
            let (sx, sy) = cam.to_screen(n.x, n.y);
            let (w, h) = (CARD_W * z, CARD_H * z);
            if !in_view(sx, sy, w, h) {
                continue;
            }
            let on = g.selected == Some(i);
            if dots {
                dot_quads.push((sx, sy, w, h, state_color(n.state), on, lit(i)));
                continue;
            }
            let key = n.key.clone();
            cards.push(
                div()
                    .debug_selector(move || format!("gnode-{key}"))
                    .absolute()
                    .left(px(sx))
                    .top(px(sy))
                    .w(px(w))
                    .h(px(h))
                    .px(px(10. * z))
                    .flex()
                    .items_center()
                    .gap(px(8. * z))
                    // As the board's cards, scaled with the zoom.
                    .rounded(px(8. * z))
                    .border_1()
                    .bg(color(if on { SELECTED } else { SURFACE }))
                    .border_color(color(if on { ACCENT } else { BORDER }))
                    .text_size(px(13. * z))
                    .line_height(px(18. * z))
                    .when(!lit(i), |d| d.opacity(0.35))
                    .child(state_dot(n.state, 10. * z))
                    .child(
                        div()
                            .flex_none()
                            .text_color(color(TEXT_4))
                            .child(n.key.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(color(if on { TEXT } else { TEXT_2 }))
                            .child(n.title.clone()),
                    )
                    .into_any_element(),
            );
        }
        let unlinked = g.unlinked_top.filter(|_| !dots).map(|top| {
            let (sx, sy) = cam.to_screen(0., top - 26.);
            div()
                .absolute()
                .left(px(sx))
                .top(px(sy))
                .text_size(px(12. * z))
                .font_weight(FontWeight::MEDIUM)
                .text_color(color(TEXT_4))
                .child("Not linked")
        });
        let bounds = self.graph_bounds.clone();
        let edges = canvas(
            move |b, window, _| {
                // The size fits the camera next frame; draw again if it changed.
                if bounds.get() != b {
                    bounds.set(b);
                    window.refresh();
                }
            },
            move |b, (), window, _| {
                let o = b.origin;
                let at = |(x, y): (f32, f32)| point(o.x + px(x), o.y + px(y));
                // A faint dot grid that moves with the world, so panning is felt; its spacing
                // doubles as the zoom falls, so it never crowds.
                let mut step = GRID_STEP * z;
                while step < 14. {
                    step *= 2.;
                }
                let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
                let dot = with_alpha(BORDER_STRONG, 0.45);
                let mut y = cam.pan_y.rem_euclid(step);
                while y < h {
                    let mut x = cam.pan_x.rem_euclid(step);
                    while x < w {
                        window
                            .paint_quad(fill(Bounds::new(at((x, y)), size(px(1.5), px(1.5))), dot));
                        x += step;
                    }
                    y += step;
                }
                let width = px((1.5 * z).clamp(0.75, 2.));
                let colours = [
                    with_alpha(ACCENT_HOVER, 0.9),
                    with_alpha(TEXT_4, 0.85),
                    with_alpha(TEXT_4, 0.25),
                    with_alpha(BORDER_STRONG, 0.9),
                    with_alpha(BORDER_STRONG, 0.25),
                ];
                // One path per style: every stretch of every link drawn that way.
                for (style, runs) in strokes.iter().enumerate() {
                    if runs.is_empty() {
                        continue;
                    }
                    let mut line = PathBuilder::stroke(width);
                    for run in runs {
                        line.move_to(at(run[0]));
                        for &p in &run[1..] {
                            line.line_to(at(p));
                        }
                    }
                    if let Ok(path) = line.build() {
                        window.paint_path(path, colours[style]);
                    }
                }
                let s = (7. * z).clamp(3.5, 9.);
                for (style, tips) in heads.iter().enumerate() {
                    if tips.is_empty() {
                        continue;
                    }
                    let mut head = PathBuilder::fill();
                    for &(tip, (dx, dy)) in tips {
                        let base = (tip.0 - dx * s, tip.1 - dy * s);
                        let side = (-dy * s * 0.55, dx * s * 0.55);
                        head.move_to(at(tip));
                        head.line_to(at((base.0 + side.0, base.1 + side.1)));
                        head.line_to(at((base.0 - side.0, base.1 - side.1)));
                        head.close();
                    }
                    if let Ok(path) = head.build() {
                        window.paint_path(path, colours[style]);
                    }
                }
                for (x, y, w, h, state, on, lit) in dot_quads {
                    let r = Bounds::new(at((x, y)), size(px(w.max(2.)), px(h.max(2.))));
                    window.paint_quad(quad(
                        r,
                        px((3. * z).max(1.)),
                        with_alpha(state, if lit { 0.9 } else { 0.3 }),
                        px(if on { 1.5 } else { 0. }),
                        color(ACCENT_HOVER),
                        BorderStyle::default(),
                    ));
                }
            },
        )
        .absolute()
        .inset_0();
        let controls = div()
            .absolute()
            .bottom(px(12.))
            .right(px(12.))
            .flex()
            .items_center()
            .p(px(3.))
            .gap(px(2.))
            .rounded_xl()
            .bg(color(SURFACE))
            .border_1()
            .border_color(color(BORDER_STRONG))
            .shadow_lg()
            .text_xs()
            .text_color(color(TEXT_3))
            // Presses here are the controls', not the background's.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                graph_control("zoom-out")
                    .child("−")
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_graph(1. / ZOOM_STEP, cx))),
            )
            .child(
                graph_control("zoom-fit")
                    .min_w(px(52.))
                    .child(format!("{:.0}%", z * 100.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.model.graph_fit();
                        cx.notify();
                    })),
            )
            .child(
                graph_control("zoom-in")
                    .child("+")
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_graph(ZOOM_STEP, cx))),
            );
        let hovering = self.model.graph.hovered.is_some();
        let dragging = matches!(self.graph_press, Some((_, true)));
        div()
            .id("graph")
            .debug_selector(|| "graph".into())
            .size_full()
            .relative()
            .overflow_hidden()
            .cursor(if dragging {
                CursorStyle::ClosedHand
            } else if hovering {
                CursorStyle::PointingHand
            } else {
                CursorStyle::OpenHand
            })
            .child(edges)
            .children(unlinked)
            .children(cards)
            .child(controls)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, _| {
                    this.graph_press = Some((e.position, false));
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                match (this.graph_press, e.pressed_button) {
                    (Some((last, _)), Some(MouseButton::Left)) => {
                        let d = e.position - last;
                        this.model.graph_pan(f32::from(d.x), f32::from(d.y));
                        this.graph_press = Some((e.position, true));
                        cx.notify();
                    }
                    _ => {
                        // A release outside the graph ends a drag without a click.
                        this.graph_press = None;
                        let (x, y) = this.graph_point(e.position);
                        if this.model.graph_hover(x, y) {
                            cx.notify();
                        }
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, cx| {
                    if let Some((_, false)) = this.graph_press.take() {
                        let (x, y) = this.graph_point(e.position);
                        this.model.graph_click(x, y);
                    }
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                let d = e.delta.pixel_delta(px(20.));
                if e.modifiers.control || e.modifiers.platform {
                    let (x, y) = this.graph_point(e.position);
                    this.model
                        .graph_zoom((f32::from(d.y) * 0.004).exp(), Some((x, y)));
                } else {
                    this.model.graph_pan(f32::from(d.x), f32::from(d.y));
                }
                cx.notify();
            }))
            .into_any_element()
    }

    // ---- rendering: lists ---------------------------------------------------------------------

    /// The activity feed: day headings and one line per event, newest first, virtualised as the
    /// lists are; "Show older" ends it while more pages wait.
    fn feed(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.model.activity.is_empty() {
            return div()
                .size_full()
                .child(empty(
                    "No activity yet",
                    "What Claude and you do in this project shows up here, newest first.",
                ))
                .into_any_element();
        }
        let rows = self.model.feed();
        let count = rows.len() + usize::from(self.model.activity_more);
        let list = uniform_list(
            "feed",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .map(|i| match rows.get(i) {
                        // Every line spans the feed, as the lists' rows do.
                        Some(FeedRow::Day(label)) => div()
                            .w_full()
                            .h(px(ROW))
                            .px_4()
                            .flex()
                            .items_end()
                            .pb_1()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(TEXT_4))
                            .child(label.clone())
                            .into_any_element(),
                        Some(FeedRow::Event(e)) => this.feed_line(*e, cx),
                        None => div()
                            .id("show-older")
                            .debug_selector(|| "show-older".into())
                            .w_full()
                            .h(px(ROW))
                            .px_4()
                            .flex()
                            .items_center()
                            .text_color(color(TEXT_4))
                            .cursor_pointer()
                            .hover(|s| s.text_color(color(TEXT_2)))
                            .child("Show older")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.model.activity_older();
                                cx.notify();
                            }))
                            .into_any_element(),
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.list_scroll)
        .size_full();
        div()
            .debug_selector(|| "feed".into())
            .size_full()
            .child(list)
            .into_any_element()
    }

    /// One event: a dot if it is new on this visit, who, what, the node, and when.
    fn feed_line(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(a) = self.model.activity.get(index) else {
            return div().into_any_element();
        };
        let on = index == self.model.activity_index;
        let new = self.model.is_new(a);
        let human = a.event.by == "human";
        let (key, title) = a.node.as_ref().map_or((String::new(), String::new()), |b| {
            (b.key.clone(), b.title.clone())
        });
        let line = div()
            .id(SharedString::from(format!("event-{index}")))
            .debug_selector(move || format!("event-{index}"))
            .w_full()
            .h(px(ROW))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .when(on, |d| d.bg(color(SELECTED)))
            .when(!on, |d| d.hover(|s| s.bg(color(HOVER))))
            .child(
                div()
                    .size(px(6.))
                    .flex_none()
                    .rounded_full()
                    .when(new, |d| d.bg(color(ACCENT))),
            )
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
                    .flex_none()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(if new { TEXT } else { TEXT_2 }))
                    .child(who(&a.event.by).to_owned()),
            )
            .child(
                div()
                    .flex_none()
                    .max_w(px(320.))
                    .truncate()
                    .text_color(color(TEXT_3))
                    .child(describe(a)),
            )
            .child(div().flex_none().text_color(color(TEXT_4)).child(key))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(color(TEXT_2))
                    .child(title),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(ago(&a.event.at)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.activity_select(index);
                this.model.open_detail();
                cx.notify();
            }));
        let flash = self.model.flashed.get(&a.event.id).copied();
        flashing(line, &a.event.id.to_string(), flash)
    }

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
            row = row.child(self.priority_button(Menu::RowPriority(id), id, r.priority, false, cx));
        }
        row = row
            .child(
                div()
                    .w(px(62.))
                    .flex_none()
                    .text_color(color(TEXT_4))
                    .child(r.key.clone()),
            )
            .child(self.state_button(id, r.kind, r.state, cx))
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
        // The wheel scrolls a column down; across, only a sideways swipe or Shift+wheel.
        let mut cols = div()
            .id("board")
            .debug_selector(|| "board".into())
            .size_full()
            .overflow_x_scroll()
            .restrict_scroll_to_axis()
            .track_scroll(&self.board_scroll)
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
                .rounded_xl()
                .bg(color(SURFACE))
                .border_1()
                .border_color(color(BORDER))
                .overflow_y_scroll()
                .restrict_scroll_to_axis()
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
                        .flex_none()
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
                    .debug_selector({
                        let key = r.key.clone();
                        move || format!("card-{key}")
                    })
                    // Cards keep their height, so a full column overflows and scrolls.
                    .flex_none()
                    .p_3()
                    .rounded_lg()
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
                            .child(self.priority_button(
                                Menu::RowPriority(id),
                                id,
                                r.priority,
                                false,
                                cx,
                            ))
                            .child(self.state_button(id, Kind::Task, r.state, cx)),
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
        // Shift+wheel scrolls the board across, before any column sees it: a listener in the
        // capture phase over the board's area, which then stops the event.
        let handle = self.board_scroll.clone();
        let shift_wheel = canvas(
            |_, _, _| (),
            move |area, (), window, _| {
                window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture
                        || !e.modifiers.shift
                        || !area.contains(&e.position)
                    {
                        return;
                    }
                    let d = e.delta.pixel_delta(px(20.));
                    let along = if f32::from(d.x) != 0. { d.x } else { d.y };
                    let max = f32::from(handle.max_offset().x);
                    let mut offset = handle.offset();
                    offset.x = px((f32::from(offset.x) + f32::from(along)).clamp(-max, 0.));
                    handle.set_offset(offset);
                    cx.stop_propagation();
                    window.refresh();
                });
            },
        )
        // Pinned to the corner: an absolute element without offsets sits where the flow would
        // have put it, here below the board.
        .absolute()
        .inset_0();
        div().relative().size_full().child(cols).child(shift_wheel)
    }

    // ---- rendering: the detail panel ----------------------------------------------------------

    /// Notice the panel opening or closing since the last frame.
    fn track_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match (&self.model.detail, self.last_detail.is_some()) {
            (Some(d), was_open) => {
                if !was_open {
                    self.detail_epoch += 1;
                    self.detail_leaving = None;
                }
                self.last_detail = Some(d.clone());
            }
            (None, true) => {
                self.detail_epoch += 1;
                self.detail_leaving = self.last_detail.take();
                let epoch = self.detail_epoch;
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(SLIDE_OUT).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.detail_epoch == epoch {
                            this.detail_leaving = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            (None, false) => {}
        }
    }

    /// The detail panel, sliding in when it opens and out when it closes. Its width animates,
    /// so the list beside it makes room smoothly instead of jumping.
    fn detail(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (d, closing) = match (&self.model.detail, &self.detail_leaving) {
            (Some(d), _) => (d.clone(), false),
            (None, Some(d)) => (d.clone(), true),
            (None, None) => return None,
        };
        let panel = self.detail_panel(&d, cx);
        let (id, duration) = if closing {
            (format!("detail-out-{}", self.detail_epoch), SLIDE_OUT)
        } else {
            (format!("detail-in-{}", self.detail_epoch), SLIDE)
        };
        let easing: fn(f32) -> f32 = if closing { ease_in_out } else { ease_out_cubic };
        Some(
            div()
                .debug_selector(|| "detail-panel".into())
                .flex_none()
                .h_full()
                .overflow_hidden()
                .child(panel)
                .with_animation(
                    SharedString::from(id),
                    Animation::new(duration).with_easing(easing),
                    move |el, t| {
                        let open = if closing { 1.0 - t } else { t };
                        el.w(px(DETAIL_WIDTH * open)).opacity(0.3 + 0.7 * open)
                    },
                )
                .into_any_element(),
        )
    }

    fn detail_panel(&mut self, d: &Detail, cx: &mut Context<Self>) -> Stateful<Div> {
        let n = &d.node;
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
                self.priority_button(Menu::DetailPriority, n.id, n.priority, true, cx),
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
        let mut links = div().flex().flex_col().gap_3().child(
            div()
                .flex()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color(TEXT_4))
                        .child("Links"),
                )
                .child(div().flex_1())
                .child(self.add_link_button(n.kind, cx)),
        );
        for g in &d.link_groups {
            links = links.child(self.link_group(g, cx));
        }
        let flow = self.state_flow(n.id, n.kind, n.state, cx);
        let title = self.detail_title(n, cx);
        let body = self.detail_body(n, cx);
        let mut timeline = div().flex().flex_col().gap_3();
        for e in d.history.iter().rev() {
            timeline = timeline.child(event_line(e));
        }
        div()
            .id(SharedString::from(format!("detail-{}", n.key)))
            .debug_selector(|| "detail-body-panel".into())
            .w(px(DETAIL_WIDTH))
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
                            .debug_selector(|| "close-detail".into())
                            .h(px(24.))
                            .px_2()
                            .flex()
                            .items_center()
                            .rounded_lg()
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
                    .child(title)
                    .when(has_flow(n.kind), |d| d.child(flow))
                    .child(props)
                    .child(body)
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
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.escape(window, cx)))
            .on_action(cx.listener(|this, _: &input::Escape, window, cx| this.escape(window, cx)))
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
                    .rounded_xl()
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
        let priority = self.select(
            Menu::Priority,
            "priority-select",
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(priority_mark(form.priority))
                .child(PRIORITY_NAMES[usize::from(form.priority.clamp(1, 4) - 1)]),
            Self::priority_menu,
            cx,
        );
        let create = div()
            .id("create-task")
            .debug_selector(|| "create-task".into())
            .flex_none()
            .h(px(28.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .rounded_lg()
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
            .on_click(cx.listener(|this, _, window, cx| this.submit_form(window, cx)));
        let card = div()
            .debug_selector(|| "new-task-card".into())
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
                    .child("New task")
                    .child(div().text_color(color(TEXT_4)).child("· starts in Backlog")),
            )
            // Flush with the card's edge, as the breadcrumb above them is.
            .child(flush_input(Input::new(&form.title).text_lg(), 32.))
            .child(
                div().mx(-TEXTAREA_PX).child(
                    Textarea::new(&form.body)
                        .appearance(false)
                        .xsmall()
                        .text_sm(),
                ),
            )
            .child(div().h(px(1.)).bg(color(BORDER)))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(priority)
                    .child(div().flex_1())
                    .child(create),
            );
        Some(self.modal("new-task-modal", card, cx))
    }

    /// The command palette: a field, what it finds in sections, and the keys that drive it.
    fn palette_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.model.palette.clone()?;
        let mut results = div()
            .id("palette-results")
            .track_scroll(&self.palette_scroll)
            .max_h(px(400.))
            .overflow_y_scroll()
            .p_1()
            .flex()
            .flex_col();
        let mut section = None;
        for (i, item) in p.items.iter().enumerate() {
            let s = if p.link.is_some() {
                "Link to"
            } else {
                section_of(item, &p.query)
            };
            if section != Some(s) {
                results = results.child(
                    div()
                        .px_3()
                        .pt_2()
                        .pb_1()
                        .text_xs()
                        .text_color(color(TEXT_4))
                        .child(s),
                );
                section = Some(s);
            }
            results = results.child(self.palette_row(i, item, i == p.selected, cx));
        }
        if p.items.is_empty() {
            results = results.child(
                div()
                    .h(px(64.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(color(TEXT_4))
                    .child("Nothing matches"),
            );
        }
        // What the actions apply to; when picking a link's other end, the link so far.
        let context = match &p.link {
            Some(pick) => Some(format!("{} · {}", pick.key, pick.choice.label)),
            None => self.model.acted_on().map(|r| r.key),
        };
        let card = div()
            .debug_selector(|| "palette".into())
            .key_context(PALETTE)
            .w(px(640.))
            .flex()
            .flex_col()
            .on_action(cx.listener(|this, _: &PaletteUp, _, cx| this.move_palette(-1, cx)))
            .on_action(cx.listener(|this, _: &PaletteDown, _, cx| this.move_palette(1, cx)))
            // One row, the field centred in it, with what the actions apply to at its end.
            .child(
                div()
                    .debug_selector(|| "palette-field".into())
                    .h(px(52.))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .debug_selector(|| "palette-input".into())
                            .flex_1()
                            .min_w_0()
                            .child(flush_input(
                                Input::new(&self.palette_input).text_base(),
                                28.,
                            )),
                    )
                    .when_some(context, |d, key| {
                        d.child(
                            div()
                                .flex_none()
                                .h(px(22.))
                                .px_2()
                                .flex()
                                .items_center()
                                .rounded_lg()
                                .border_1()
                                .border_color(color(BORDER_STRONG))
                                .text_xs()
                                .text_color(color(TEXT_3))
                                .child(key),
                        )
                    }),
            )
            .child(div().h(px(1.)).bg(color(BORDER)))
            .child(results)
            .child(
                div()
                    .h(px(36.))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_t_1()
                    .border_color(color(BORDER))
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(kbd("↑"))
                    .child(kbd("↓"))
                    .child("to move")
                    .child(div().w(px(8.)))
                    .child(kbd("↵"))
                    .child("to run")
                    .child(div().w(px(8.)))
                    .child(kbd("Esc"))
                    .child("to close"),
            );
        Some(self.modal("palette-modal", card, cx))
    }

    fn palette_row(
        &self,
        i: usize,
        item: &PaletteItem,
        on: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let row = div()
            .id(SharedString::from(format!("palette-item-{i}")))
            .debug_selector(move || format!("palette-item-{i}"))
            .h(px(36.))
            .px_3()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .rounded_lg()
            .cursor_pointer()
            .when(on, |r| r.bg(color(SELECTED)).text_color(color(TEXT)))
            .when(!on, |r| r.text_color(color(TEXT_2)))
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.model.palette.as_ref().is_some_and(|p| p.selected != i) {
                    this.model.palette_highlight(i);
                    cx.notify();
                }
            }))
            .on_click(
                cx.listener(move |this, _, window, cx| this.run_palette(Some(i), window, cx)),
            );
        match item {
            PaletteItem::Action(a) => {
                let mut keys = div().flex().items_center().gap_1();
                for k in a.shortcut.unwrap_or_default().split_whitespace() {
                    keys = keys.child(kbd(k));
                }
                row.child(div().truncate().child(a.label.clone()))
                    .when_some(a.context.clone(), |r, c| {
                        r.child(div().flex_none().text_color(color(TEXT_4)).child(c))
                    })
                    .child(div().flex_1())
                    .child(keys)
            }
            PaletteItem::Node(n) => row
                .child(state_mark(n.kind, n.state))
                .child(
                    div()
                        .w(px(60.))
                        .flex_none()
                        .text_color(color(TEXT_4))
                        .child(n.key.clone()),
                )
                .child(div().flex_1().min_w_0().truncate().child(n.title.clone()))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(color(TEXT_4))
                        .child(title_case(n.kind.as_str())),
                ),
        }
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
                .rounded_xl()
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
        // The toolbar's filters apply to lists; the feed and the graph have their own ways.
        let toolbar = (!matches!(self.model.view, View::Activity | View::Graph))
            .then(|| self.toolbar(cx).into_any_element());
        let content = self.content(window, cx);
        self.track_detail(window, cx);
        let detail = self.detail(cx);
        let form = self.form_overlay(cx);
        let help = self.help_overlay(cx);
        let palette = self.palette_overlay(cx);
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
                if this.menu.is_some() {
                    this.escape(window, cx);
                } else if this.help || this.form.is_some() {
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
                this.menu = Some(Menu::Filter);
                this.sub = None;
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
            .on_action(cx.listener(|this, _: &GoActivity, w, cx| this.go(View::Activity, w, cx)))
            .on_action(cx.listener(|this, _: &GoTasks, w, cx| this.go(View::Tasks, w, cx)))
            .on_action(cx.listener(|this, _: &GoLessons, w, cx| this.go(View::Lessons, w, cx)))
            .on_action(cx.listener(|this, _: &GoDecisions, w, cx| this.go(View::Decisions, w, cx)))
            .on_action(cx.listener(|this, _: &GoFindings, w, cx| this.go(View::Findings, w, cx)))
            .on_action(cx.listener(|this, _: &GoGraph, w, cx| this.go(View::Graph, w, cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_graph(ZOOM_STEP, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_graph(1. / ZOOM_STEP, cx)))
            .on_action(cx.listener(|this, _: &ZoomFit, _, cx| {
                if this.model.view == View::Graph {
                    this.model.graph_fit();
                    cx.notify();
                }
            }))
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
            .on_action(
                cx.listener(|this, _: &OpenPalette, window, cx| this.toggle_palette(window, cx)),
            )
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
                        .rounded_xl()
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
                                .children(toolbar)
                                .child(list_area),
                        )
                        .children(detail),
                ),
            )
            .children(form)
            .children(palette)
            .children(help)
            .children(notice)
    }
}

fn prio(app: &mut LivtkApp, p: u8, cx: &mut Context<LivtkApp>) {
    app.model.set_priority(p);
    cx.notify();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Menu {
    Filter,
    Display,
    /// Opened from Display.
    Grouping,
    /// Opened from Display.
    Ordering,
    /// The new task's priority, in the form.
    Priority,
    /// A task's priority, from its row or card.
    RowPriority(Uuid),
    /// The open task's priority, in the detail panel.
    DetailPriority,
    /// The relations a link can be added by, in the detail panel.
    AddLink,
    /// A node's state, from its row or card.
    RowState(Uuid),
    /// Ratify or ratify as a standing rule, from the flow in the detail panel.
    Standing,
}

impl Menu {
    fn is_sub(self) -> bool {
        matches!(self, Menu::Grouping | Menu::Ordering)
    }
}

const GROUPINGS: &[(&str, &str, Grouping)] = &[
    ("group-state", "State", Grouping::State),
    ("group-priority", "Priority", Grouping::Priority),
    ("group-none", "No grouping", Grouping::None),
];

const ORDERINGS: &[(&str, &str, Ordering)] = &[
    ("order-priority", "Priority", Ordering::Priority),
    ("order-updated", "Last updated", Ordering::Updated),
    ("order-created", "Created", Ordering::Created),
];

const PRIORITY_IDS: [&str; 4] = ["priority-1", "priority-2", "priority-3", "priority-4"];

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

fn nav_item(id: SharedString, label: &str, active: bool, count: usize) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .h(px(28.))
        .rounded_lg()
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
                    // The unseen count stops at its cap; past 99 a badge says so.
                    .child(if count > 99 {
                        "99+".to_owned()
                    } else {
                        format!("{count}")
                    }),
            )
        })
}

fn menu_panel(width: f32) -> Div {
    div()
        .w(px(width))
        .p_1()
        .flex()
        .flex_col()
        .rounded_xl()
        .bg(color(SURFACE))
        .border_1()
        .border_color(color(BORDER_STRONG))
        .shadow_lg()
        .text_sm()
}

fn menu_heading(text: &'static str) -> Div {
    div()
        .px_2()
        .pt_2()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color(TEXT_4))
        .child(text)
}

fn menu_divider() -> Div {
    div().my_1().h(px(1.)).bg(color(BORDER))
}

/// A row of a menu: what it is, from the left edge, with no empty slot before it.
/// A state as a dot of its colour, `size` across: the graph's cards shrink with the zoom, and
/// the full mark does not.
fn state_dot(state: State, size: f32) -> Div {
    let faint = matches!(state, State::Backlog | State::Parked) || state.is_dismissed();
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(with_alpha(state_color(state), if faint { 0.5 } else { 1. }))
}

/// A button of the graph's zoom controls.
fn graph_control(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .h(px(24.))
        .min_w(px(24.))
        .px_1()
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .cursor_pointer()
        .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT)))
}

/// The section an item of the palette sits in.
fn section_of(item: &PaletteItem, query: &str) -> &'static str {
    match item {
        PaletteItem::Action(_) => "Actions",
        PaletteItem::Node(_) if query.trim().is_empty() => "Updated lately",
        PaletteItem::Node(_) => "Results",
    }
}

/// The child of the results list that shows item `index`: section headings come before it.
fn palette_child(items: &[PaletteItem], index: usize) -> usize {
    let mut headings = 0;
    let mut last = None;
    for item in items.iter().take(index + 1) {
        let s = std::mem::discriminant(item);
        if last != Some(s) {
            headings += 1;
            last = Some(s);
        }
    }
    index + headings
}

/// A single-line input with no frame and no padding of its own, `height` tall, so its text sits
/// exactly where the surrounding layout puts it. Input's own `h` sets only a multi-line
/// height, hence `Styled::h`.
fn flush_input(input: Input, height: f32) -> Input {
    Styled::h(input.appearance(false).px_0().py_0(), px(height))
}

fn option_row(id: SharedString) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.))
        .px(px(6.))
        .flex()
        .items_center()
        .gap_2()
        .rounded_lg()
        .cursor_pointer()
        .text_color(color(TEXT_2))
        .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT)))
}

/// The mark of the chosen option in a menu that picks one, at the row's right end.
fn check(on: bool) -> Div {
    div()
        .ml_auto()
        .pl_2()
        .text_xs()
        .text_color(color(TEXT))
        .when(on, |d| d.child("✓"))
}

/// A box in a menu that picks several: an outline when off, filled and ticked when on.
fn checkbox(on: bool) -> Div {
    div()
        .size(px(14.))
        .flex_none()
        .rounded(px(3.))
        .border_1()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.))
        .when(on, |c| {
            c.bg(color(ACCENT))
                .border_color(color(ACCENT))
                .text_color(color(TEXT))
                .child("✓")
        })
        .when(!on, |c| c.border_color(color(TEXT_4)))
}

/// A labelled setting in the Display menu: the name on the left, its select on the right.
fn display_field(label: &'static str, select: Div) -> Div {
    div()
        .h(px(36.))
        .px(px(6.))
        .flex()
        .items_center()
        .justify_between()
        .child(div().text_color(color(TEXT_3)).child(label))
        .child(select)
}

fn switch(on: bool) -> Div {
    div()
        .w(px(26.))
        .h(px(16.))
        .flex_none()
        .rounded_full()
        .p(px(2.))
        .flex()
        .when(on, |s| s.bg(color(ACCENT)).justify_end())
        .when(!on, |s| s.bg(color(BORDER_STRONG)))
        .child(div().size(px(12.)).rounded_full().bg(color(TEXT)))
}

/// An active filter, shown beside the Filter button; its × clears it.
fn chip(
    id: &'static str,
    label: String,
    clear: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(24.))
        .pl_2()
        .pr_1()
        .flex()
        .items_center()
        .gap_1()
        .rounded_lg()
        .bg(color(SURFACE))
        .border_1()
        .border_color(color(BORDER_STRONG))
        .text_xs()
        .text_color(color(TEXT_2))
        .child(label)
        .child(
            div()
                .id(SharedString::from(format!("{id}-x")))
                .px_1()
                .rounded(px(3.))
                .text_color(color(TEXT_4))
                .cursor_pointer()
                .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT)))
                .child("×")
                .on_click(clear),
        )
}

fn segments() -> Div {
    div()
        .flex()
        .p(px(2.))
        .gap(px(2.))
        .rounded_lg()
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
        .rounded(px(6.))
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
        .rounded(px(6.))
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
                .rounded_lg()
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
