//! The window: the top bar (the project, the palette's field, the panels' toggles), a sidebar on
//! the black canvas, an inset content panel (the view itself, its dock floating over it), the
//! bottom bar, the detail panel, and overlays (new task, shortcuts, notices). It renders the model
//! and forwards input to it; no behaviour lives here.
//!
//! Keys are bound only on the list area. Text fields (the filter, notes, the new-task form) and
//! the terminal live outside it, so typing never triggers a command.

mod activity_grid;
pub mod contour_grid;
pub mod diagram;
mod docs;
pub mod icons;
pub mod menus;
mod panes;
pub mod state_flow;
pub mod terminal;
mod terminals;
mod theme;

use std::cell::Cell;
use std::collections::HashMap;
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

use crate::model::Axis as PaneAxis;
use crate::model::docs::DocRow;
use crate::model::keys;
use crate::model::{
    Act, Arrival, Ask, CARD_H, CARD_W, Choice, DOTS_BELOW, Detail, DocPick, FeedRow, GroupKey,
    Grouping, Item as PaletteItem, Layout, Level, LinkChoice, LinkGroup, Look, Mark, Model,
    Ordering, Palette, Quiet, Run, Saved, SavedWindow, Step, Toast, View, arrows_with, created_as,
    describe, drop_allowed, has_flow, link_choices, link_curve, state_name, steps_with,
    visible_runs,
};
use icons::{Icon, icon, icon_hover};
use theme::*;

/// A line of a doc asked to be seen (a match found), at `top` in the window: the reader scrolls
/// to it.
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = livtk, no_json)]
pub struct RevealInDoc {
    pub top: f32,
}

/// A link in a doc clicked, to follow: another doc, a node (`livtk:KEY`), a web page.
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = livtk, no_json)]
pub struct FollowLink {
    pub href: String,
}

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
        PrevColumn,
        NextColumn,
        ToggleEmpty,
        GoInbox,
        GoActivity,
        GoTasks,
        GoLessons,
        GoDecisions,
        GoFindings,
        GoGraph,
        GoTerminal,
        GoDocs,
        FindInDoc,
        SaveDoc,
        OpenSettings,
        DocBack,
        DocForward,
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
        ToggleSidebar,
        ToggleBottom,
        TogglePanel,
        SplitRight,
        SplitDown,
        ClosePane,
        NextPane,
        PrevPane,
        NewTerminal,
        OpenPalette,
        PaletteUp,
        PaletteDown,
        Submit,
        Cancel,
        // The menu bar's own: the app, the window, and what has no key of its own.
        Quit,
        HideApp,
        HideOthers,
        ShowAll,
        BringToFront,
        CloseWindow,
        Minimize,
        ZoomWindow,
        About,
        AddLinkMenu,
        InstallCli,
        CheckForUpdates,
        RestartToUpdate,
        OpenGitHub,
        ReportIssue
    ]
);

/// Where livtk lives, for Help.
const REPOSITORY: &str = "https://github.com/nebulaw/tasks";

const NAV: &str = "LivtkNav";
const FORM: &str = "LivtkForm";
const PALETTE: &str = "LivtkPalette";
/// Inside the palette's field: arrows move the highlight instead of the cursor.
const PALETTE_FIELD: &str = "LivtkPalette > Input";

const POLL: Duration = Duration::from_millis(200);
/// Who the app is to the system: what notifications are posted as, and the identifier of the
/// macOS app bundle (`scripts/bundle-app`).
pub const APP_ID: &str = "com.github.nebulaw.livtk";
pub const APP_NAME: &str = "livtk";
/// The system notifications the app posts are tagged with this and their number. Each tells
/// only what is new since the last, so none tells of what another already did and a click opens
/// what that one told of; the system groups them as it does any app's (Linux's ignore tags).
const NOTIFICATION: &str = "livtk-inbox";
/// How many notifications are remembered for their clicks; older ones open the inbox.
const NOTIFIED_KEPT: usize = 32;
const FADE: Duration = Duration::from_millis(180);
const FLASH: Duration = Duration::from_millis(900);
/// A notice coming in, and fading out when its time is up.
const NOTICE_IN: Duration = Duration::from_millis(140);
const NOTICE_FADE: Duration = Duration::from_millis(240);
/// A notification's width, and the height of its text: clipped, then open with Show more.
const TOAST_WIDTH: f32 = 380.;
const TOAST_CLIPPED: f32 = 84.;
const TOAST_OPEN: f32 = 320.;
/// Text longer than this (or of more lines than four) is clipped, with Show more.
const TOAST_LONG: usize = 220;
/// The detail panel opening and closing: it grows from the right edge and the list makes room.
const SLIDE: Duration = Duration::from_millis(240);
/// The expanded sidebar's rows: the rail's icon size, and a font to match.
const NAV_ICON: f32 = 18.;
const NAV_ROW: f32 = 32.;
const NAV_TEXT: f32 = 15.;
const SLIDE_OUT: Duration = Duration::from_millis(200);
/// The selection's highlight gliding to the next row or the one before.
const GLIDE: Duration = Duration::from_millis(120);
/// The detail panel's width: dragged by its left edge between these, never more than a share of
/// the window. Where the content would be left narrower than `CONTENT_MIN`, it floats over it.
const PANEL_WIDTH: f32 = 480.;
/// The bottom bar's height, dragged by its top edge between these; the content above it keeps
/// at least `CONTENT_MIN_H`.
const BOTTOM_HEIGHT: f32 = 236.;
const BOTTOM_MIN: f32 = 96.;
const BOTTOM_MAX: f32 = 640.;
const CONTENT_MIN_H: f32 = 240.;
/// Below the bottom bar, the window's inset (as below the content box).
const BOTTOM_INSET: f32 = 8.;
const PANEL_MIN: f32 = 360.;
const PANEL_MAX: f32 = 720.;
const PANEL_SHARE: f32 = 0.55;
const CONTENT_MIN: f32 = 360.;
/// The sidebar's width, dragged by its right edge between these; dragged under `SIDEBAR_SNAP`
/// it collapses to the rail. Below `RAIL_BELOW` of window it is the rail anyway.
const SIDEBAR_WIDTH: f32 = 232.;
const SIDEBAR_MIN: f32 = 200.;
const SIDEBAR_MAX: f32 = 360.;
const SIDEBAR_SNAP: f32 = 140.;
const RAIL_WIDTH: f32 = 52.;
const RAIL_BELOW: f32 = 1000.;
/// The content's own inset (8 px) and border (2 px), beside the sidebar.
const CONTENT_FRAME: f32 = 10.;
/// How much `+` and `-` zoom the graph.
const ZOOM_STEP: f32 = 1.25;
/// How far J and K scroll a doc being read.
const READ_STEP: f32 = 64.;
/// A list row's leading slot: a task's priority, or another kind's icon.
const LEAD: f32 = 22.;
/// The dock's height, and its gap from the content's bottom edge.
const DOCK_HEIGHT: f32 = 40.;
const DOCK_GAP: f32 = 16.;
/// The room lists and columns add at the end of what scrolls, so their last item can rise
/// clear of the dock floating over them; everything else passes under it.
const DOCK_SPACE: f32 = DOCK_HEIGHT + 2. * DOCK_GAP;
/// The same room in a virtualised list, whose items are all one row tall.
const DOCK_ROWS: usize = 2;
const _: () = assert!(
    DOCK_ROWS as f32 * ROW == DOCK_SPACE,
    "the room must be whole rows"
);
/// On macOS the window's titlebar is the app's own: the traffic lights sit on the window's black
/// at the top bar's left, and the bar drags the window. Elsewhere the system's titlebar stays,
/// and this is 0.
const TITLEBAR: f32 = if cfg!(target_os = "macos") {
    TOP_BAR
} else {
    0.
};
/// The bar across the window's top, over the sidebar, the content and the panel: the palette's
/// field in its middle, the panels' toggles at its right.
const TOP_BAR: f32 = 48.;
/// The palette's field in the top bar, at most this wide, and how long its hover takes to come
/// up or go.
const TOP_FIELD_W: f32 = 520.;
const HOVER_FADE: Duration = Duration::from_millis(150);
/// Room the traffic lights take at the top bar's left, and where they sit, centred in its
/// height.
const TRAFFIC_ROOM: f32 = 84.;
const TRAFFIC_LIGHTS: (f32, f32) = (20., 18.);
/// The content width from which the dock's buttons carry labels; narrower, icons alone.
const DOCK_WIDE: f32 = 900.;
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
pub(super) fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Cmd+W hides the window on macOS only: elsewhere closing it is the window's own business.
const CLOSE_WINDOW: &str = "Close the window; the app keeps watching";

/// Every shortcut, in the order the shortcuts modal shows them: (section, keys, what it does).
/// In `keys`, the words "then", "or" and "+" are separators; everything else is a key cap.
const SHORTCUTS: &[(&str, &[&str], &str)] = &[
    (
        "Navigate",
        &[keys::MOD, "+", "K"],
        "Command palette and search",
    ),
    (
        "Navigate",
        &[keys::MOD, "+", "B"],
        "Collapse or expand the sidebar",
    ),
    (
        "Navigate",
        &[keys::MOD, "+", "J"],
        "Show or hide the bottom bar",
    ),
    (
        "Navigate",
        &[keys::ALT, "+", keys::MOD, "+", "B"],
        "Show or hide the detail panel",
    ),
    ("Navigate", &["J", "or", "↓"], "Next row or group"),
    ("Navigate", &["K", "or", "↑"], "Previous row or group"),
    (
        "Navigate",
        &["H", "L", "or", "←", "→"],
        "Board: the next column left or right",
    ),
    (
        "Navigate",
        &["Enter"],
        "Open the detail panel; on a group, fold it",
    ),
    ("Navigate", &["Esc"], "Close the panel or a modal"),
    ("Navigate", &["G", "then", "I"], "Inbox"),
    ("Navigate", &["G", "then", "A"], "Activity"),
    ("Navigate", &["G", "then", "T"], "Tasks"),
    ("Navigate", &["G", "then", "L"], "Lessons"),
    ("Navigate", &["G", "then", "D"], "Decisions"),
    ("Navigate", &["G", "then", "F"], "Findings"),
    ("Navigate", &["G", "then", "R"], "Graph"),
    ("Navigate", &["G", "then", "O"], "Docs"),
    ("Docs", &[keys::MOD, "+", "F"], "Find in the doc"),
    ("Docs", &[keys::MOD, "+", "S"], "Save the doc being edited"),
    ("Docs", &[keys::MOD, "+", "["], "Back to the doc before"),
    ("Docs", &[keys::MOD, "+", "]"], "Forward again"),
    ("Navigate", &["G", "then", "X"], "Terminal"),
    ("Navigate", &["[", "or", "]"], "Previous or next project"),
    ("Panes", &[keys::MOD, "+", "\\"], "Split the pane right"),
    (
        "Panes",
        &[keys::ALT, "+", keys::MOD, "+", "\\"],
        "Split the pane down",
    ),
    (
        "Panes",
        &[keys::ALT, "+", keys::MOD, "+", "]", "or", "["],
        "Next or previous pane",
    ),
    (
        "Panes",
        &[keys::ALT, "+", keys::MOD, "+", "W"],
        "Close the pane",
    ),
    ("Panes", &[keys::MOD, "+", "T"], "Another terminal"),
    ("Decide", &["A"], "Acknowledge, ratify or accept"),
    (
        "Decide",
        &[keys::SHIFT, "+", "A"],
        "Ratify as a standing rule",
    ),
    ("Decide", &["X"], "Reject"),
    ("Work", &["N", "or", keys::MOD, "+", "N"], "New task"),
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
        &["S", "then", "D"],
        "Show dismissed: canceled, rejected, retired",
    ),
    ("Display", &["S", "then", "E"], "Show empty groups"),
    (
        "Display",
        &["O", "or", keys::MOD, "+", "O"],
        "Open another project",
    ),
    ("Window", &[keys::MOD, "+", "W"], CLOSE_WINDOW),
    ("Window", &[keys::MOD, "+", "Q"], "Quit"),
    ("Display", &[keys::MOD, "+", "?"], "These shortcuts"),
];

/// Open the app on `dirs`. `quiet`: why the system posts none of its notifications where it
/// runs (a bare binary on macOS), which it tells the human.
pub fn run(dirs: Vec<PathBuf>, quiet: Option<Quiet>) {
    let application = gpui_kit::application();
    // The Dock's icon clicked with no window shown brings the app forward. Hidden by Cmd+W,
    // AppKit shows it again itself.
    application.on_reopen(|cx| cx.activate(true));
    application.run(move |cx| {
        gpui_kit::init(cx);
        // Before any window or notification, as GPUI asks.
        cx.set_app_identity(APP_ID, APP_NAME);
        theme::apply(cx);
        bind_keys(cx);
        app_actions(cx);
        let own_titlebar = TITLEBAR > 0.;
        let options = WindowOptions {
            app_id: Some("livtk".into()),
            titlebar: Some(TitlebarOptions {
                title: Some("livtk".into()),
                appears_transparent: own_titlebar,
                traffic_light_position: own_titlebar
                    .then(|| point(px(TRAFFIC_LIGHTS.0), px(TRAFFIC_LIGHTS.1))),
            }),
            // The app's strip drags the window, so AppKit neither drags from the titlebar nor
            // delays clicks there to tell a double-click.
            app_owns_titlebar_drag: own_titlebar,
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        };
        match gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                let mut app = LivtkApp::new(Model::open(&dirs), window, cx);
                if let Some(path) = Saved::path() {
                    app.remember(path, window, cx);
                }
                if let Some(why) = quiet {
                    app.model.tell_quiet(why);
                }
                app.follow_system(cx);
                // Started by the updater: say it started well, so it stays, and what changed.
                if crate::updates::report_started(livtk_build::BUILD) {
                    let notes = crate::updates::notes_of(APP_ID, livtk_build::BUILD);
                    app.model.inform(match notes {
                        Some(n) => format!("Updated to {}. {n}", livtk_build::DESCRIBE),
                        None => format!("Updated to {}.", livtk_build::DESCRIBE),
                    });
                }
                // Only the installed app looks: a development build never touches its staging.
                let bundled = std::env::current_exe()
                    .ok()
                    .and_then(|e| std::fs::canonicalize(e).ok())
                    .is_some_and(|e| crate::launch::in_bundle(&e));
                if bundled {
                    app.watch_for_updates(cx);
                }
                app
            })
        }) {
            Ok((window, app)) => dock_actions(window, app.downgrade(), cx),
            Err(e) => {
                eprintln!("livtk-app: cannot open a window: {e}");
                cx.quit();
            }
        }
    });
}

/// The Dock menu's items, heard with no window active: hidden by Cmd+W, or another app in front.
/// Then only the app's own handlers hear an action, and macOS greys an item none hears; these
/// bring the window forward and act in it. While the window is active, its own take them first.
pub fn dock_actions(window: AnyWindowHandle, app: WeakEntity<LivtkApp>, cx: &mut App) {
    fn on<A: Action>(
        window: AnyWindowHandle,
        app: &WeakEntity<LivtkApp>,
        cx: &mut App,
        act: impl Fn(&mut LivtkApp, &mut Window, &mut Context<LivtkApp>) + 'static,
    ) {
        let app = app.clone();
        cx.on_action(move |_: &A, cx| {
            cx.activate(true);
            let _ = window.update(cx, |_, window, cx| {
                window.activate_window();
                app.update(cx, |app, cx| act(app, window, cx))
            });
        });
    }
    on::<NewTask>(window, &app, cx, |app, w, cx| app.open_form(w, cx));
    on::<GoInbox>(window, &app, cx, |app, w, cx| app.go(View::Inbox, w, cx));
    on::<menus::Project1>(window, &app, cx, |app, w, cx| app.switch_project(0, w, cx));
    on::<menus::Project2>(window, &app, cx, |app, w, cx| app.switch_project(1, w, cx));
    on::<menus::Project3>(window, &app, cx, |app, w, cx| app.switch_project(2, w, cx));
    on::<menus::Project4>(window, &app, cx, |app, w, cx| app.switch_project(3, w, cx));
    on::<menus::Project5>(window, &app, cx, |app, w, cx| app.switch_project(4, w, cx));
    on::<menus::Project6>(window, &app, cx, |app, w, cx| app.switch_project(5, w, cx));
    on::<menus::Project7>(window, &app, cx, |app, w, cx| app.switch_project(6, w, cx));
    on::<menus::Project8>(window, &app, cx, |app, w, cx| app.switch_project(7, w, cx));
    on::<menus::Project9>(window, &app, cx, |app, w, cx| app.switch_project(8, w, cx));
}

/// What the app does whatever window, field or menu has the keys: quit, hide, Help's links.
fn app_actions(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &BringToFront, cx| cx.activate(true));
    cx.on_action(|_: &OpenGitHub, cx| cx.open_url(REPOSITORY));
    cx.on_action(|_: &ReportIssue, cx| cx.open_url(&format!("{REPOSITORY}/issues/new")));
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
        KeyBinding::new("shift-a", Standing, nav),
        KeyBinding::new("x", Reject, nav),
        KeyBinding::new("n", NewTask, nav),
        KeyBinding::new("b", ToggleLayout, nav),
        KeyBinding::new("s d", ToggleAll, nav),
        KeyBinding::new("h", PrevColumn, nav),
        KeyBinding::new("left", PrevColumn, nav),
        KeyBinding::new("l", NextColumn, nav),
        KeyBinding::new("right", NextColumn, nav),
        KeyBinding::new("s e", ToggleEmpty, nav),
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
        KeyBinding::new("g x", GoTerminal, nav),
        KeyBinding::new("g o", GoDocs, nav),
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
        // Everywhere, typing included: Cmd+? on macOS, Ctrl+? elsewhere (`secondary`), is never
        // text. Keyboards report it either as the `?` character or as Shift with `/`, so both
        // spellings are bound.
        KeyBinding::new("secondary-?", ToggleHelp, None),
        KeyBinding::new("secondary-shift-/", ToggleHelp, None),
        KeyBinding::new("secondary-k", OpenPalette, None),
        KeyBinding::new("secondary-b", ToggleSidebar, None),
        KeyBinding::new("secondary-j", ToggleBottom, None),
        // The Docs view's: find in the doc, back and forward through the docs read.
        KeyBinding::new("secondary-f", FindInDoc, None),
        KeyBinding::new("secondary-s", SaveDoc, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-[", DocBack, None),
        KeyBinding::new("secondary-]", DocForward, None),
        KeyBinding::new("secondary-alt-b", TogglePanel, None),
        // Panes: split the focused one, close it, go to the next or the one before.
        KeyBinding::new("secondary-\\", SplitRight, None),
        KeyBinding::new("secondary-alt-\\", SplitDown, None),
        KeyBinding::new("secondary-alt-w", ClosePane, None),
        KeyBinding::new("secondary-alt-]", NextPane, None),
        KeyBinding::new("secondary-alt-[", PrevPane, None),
        KeyBinding::new("secondary-t", NewTerminal, None),
        // Bound after GPUI Component's own, so in the palette's field they win over the cursor.
        // Ctrl+N and Ctrl+P are Control on every platform, as in every Mac text field.
        KeyBinding::new("up", PaletteUp, Some(PALETTE_FIELD)),
        KeyBinding::new("down", PaletteDown, Some(PALETTE_FIELD)),
        KeyBinding::new("ctrl-p", PaletteUp, Some(PALETTE_FIELD)),
        KeyBinding::new("ctrl-n", PaletteDown, Some(PALETTE_FIELD)),
        KeyBinding::new("escape", Cancel, Some(FORM)),
        KeyBinding::new("secondary-enter", Submit, Some(FORM)),
        // The menu bar's, everywhere: Cmd on macOS, Ctrl elsewhere.
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-n", NewTask, None),
        KeyBinding::new("secondary-o", OpenFolder, None),
    ]);
    // What a Mac app's window and app do; other platforms close and quit their own way.
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-w", CloseWindow, None),
            KeyBinding::new("cmd-m", Minimize, None),
            KeyBinding::new("cmd-h", HideApp, None),
            KeyBinding::new("alt-cmd-h", HideOthers, None),
        ]);
    }
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
    /// What has the keys while the shortcuts or About are shown.
    modal_focus: FocusHandle,
    /// The detail panel's state flow: the state it last showed, so a change transitions.
    flow: Entity<state_flow::FlowState>,
    /// Whether the app follows the system's settings it is told of only by asking (Reduce
    /// Motion): the app does; a test sets them itself.
    follows_system: bool,
    /// Each pane's list scrolling, by the pane's id.
    list_scrolls: std::cell::RefCell<HashMap<u64, UniformListScrollHandle>>,
    filter: Entity<InputState>,
    note: Entity<TextareaState>,
    /// The node the note field is for, and each other node's unsent draft, by project and node:
    /// a draft stays with its own node, as switching the panel to another swaps them, and two
    /// projects that share node ids (two clones of one repository) never share a draft.
    note_for: Option<(PathBuf, Uuid)>,
    note_drafts: HashMap<(PathBuf, Uuid), String>,
    /// The command palette's field; what it holds lives in the model.
    palette_input: Entity<InputState>,
    palette_scroll: ScrollHandle,
    /// The board's sideways scroll, which Shift+wheel drives.
    board_scroll: ScrollHandle,
    /// A newer build, staged and verified, waiting for "Restart to Update".
    update: Option<livtk_update::stage::Staged>,
    /// A look for updates under way.
    checking_updates: bool,
    /// Notifications opened with Show more.
    expanded: std::collections::HashSet<u64>,
    /// What the activity grid remembers between frames.
    grid_state: Entity<contour_grid::ContourState>,
    /// Each board column's own scrolling, so a card moved to comes into view.
    column_scrolls: std::cell::RefCell<HashMap<GroupKey, ScrollHandle>>,
    /// Where the graph was drawn last frame, in window coordinates: its size fits the camera,
    /// its origin turns pointer positions into the graph's own.
    graph_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// A press on the graph's background: where the pointer was last, and whether it moved,
    /// which makes it a drag rather than a click.
    graph_press: Option<(Point<Pixels>, bool)>,
    /// The sidebar's width when expanded, and whether the human collapsed it to the rail.
    sidebar_width: f32,
    sidebar_collapsed: bool,
    /// The sidebar's last shape (the rail or not) and its width then, and how many times it
    /// changed: a change slides it from the one to the other.
    rail_was: Option<(bool, f32)>,
    rail_epoch: usize,
    /// The sidebar's width the last change slid from.
    rail_from: Option<f32>,
    /// The docs reader: its scrolling, its sections, its find bar.
    reader: docs::Reader,
    /// Each project's terminals, by its folder: the first started the first time the view is
    /// shown, all kept while the app runs.
    terminals: HashMap<PathBuf, terminals::Terminals>,
    /// The view and project last drawn, so arriving at a terminal gives it the keys, and leaving
    /// gives them back to the list.
    last_place: Option<(u64, View, usize)>,
    /// Whether the bottom bar is shown, and its height; whether it is sliding away, and how many
    /// times it opened or shut (each slides once).
    bottom_open: bool,
    bottom_height: f32,
    bottom_leaving: bool,
    bottom_epoch: usize,
    /// Whether the pointer is over the top bar's field, and how many times that changed: a
    /// change fades its hover in or out.
    top_field_hover: bool,
    top_field_epoch: usize,
    /// Expanded by hand although the window is narrow enough for the rail; forgotten once the
    /// window is wide again.
    sidebar_peek: bool,
    panel_width: f32,
    /// The edge being dragged, if any.
    resizing: Option<Edge>,
    /// Where each pane and each split was drawn, by pane id and by the split's path: what a
    /// divider dragged and the dock's room are measured against.
    pane_bounds: std::cell::RefCell<HashMap<u64, Bounds<Pixels>>>,
    split_bounds: std::cell::RefCell<HashMap<Vec<usize>, Bounds<Pixels>>>,
    /// The divider being dragged: its split's path, and the part before it.
    split_drag: Option<(Vec<usize>, usize)>,
    /// The edge under the pointer (or being dragged), where along it, and since which arrival.
    edge_hover: Option<EdgeHover>,
    /// The glow the pointer just left, fading out.
    edge_leaving: Option<EdgeHover>,
    edge_epoch: usize,
    /// The edge the pointer is over, dragging or not.
    edge_inside: Option<Edge>,
    /// Where `app.toml` is, when the app remembers between runs (tests leave it out), and what
    /// was last written there, so it is written only when something changed.
    saved_path: Option<PathBuf>,
    last_saved: Saved,
    form: Option<NewTaskForm>,
    help: bool,
    /// The About modal is open.
    about: bool,
    /// Settings, open.
    settings: bool,
    /// What the menu bar and the Dock menu were last made from.
    menu_state: Option<menus::MenuState>,
    editing: Option<Editing>,
    /// The detail panel's last frame while open, kept to animate it out after it closes.
    last_detail: Option<Detail>,
    /// The panel closing, drawn until its animation ends.
    detail_leaving: Option<Detail>,
    /// Counts openings and closings, so each plays its animation once and switching rows in an
    /// open panel plays none.
    detail_epoch: usize,
    /// The selection's highlight on its way from one row to the next, after `j` or `k`.
    glide: Option<Glide>,
    glide_epoch: usize,
    menu: Option<Menu>,
    /// The menu the mouse-down being dispatched closed, if any: see `toggle_menu`.
    menu_closed: Option<Menu>,
    /// Whether the window is in front; while it is not, what arrives in the inbox is told by a
    /// system notification.
    window_active: bool,
    /// What each recent system notification told of, by its tag, oldest first, for a click on
    /// it to open; how many were posted, which numbers the next; and this launch, which tells
    /// its tags from an earlier run's still in the system's list (a tag in use replaces).
    notified: Vec<(String, Vec<Arrival>)>,
    notifications: u64,
    launch: Uuid,
    _subscriptions: Vec<Subscription>,
    _poll: Task<()>,
}

impl LivtkApp {
    pub fn new(model: Model, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let nav = cx.focus_handle();
        let flow = cx.new(|_| state_flow::FlowState::default());
        // The flow's hover (its drifting light) draws the window again.
        cx.observe(&flow, |_, _, cx| cx.notify()).detach();
        let grid_state = cx.new(|_| contour_grid::ContourState::default());
        cx.observe(&grid_state, |_, _, cx| cx.notify()).detach();
        // Enter adds the note; Shift+Enter breaks the line. It grows with what is written.
        let note = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Leave a note…")
                .auto_grow(1, 8)
                .submit_on_enter(true)
        });
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter…"));
        let palette_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type a command or search…"));
        let reader = docs::Reader::new(window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&note, window, Self::on_note),
            cx.subscribe_in(&filter, window, Self::on_filter),
            cx.subscribe_in(&palette_input, window, Self::on_palette_input),
            cx.subscribe_in(&reader.find, window, Self::on_find),
            // Focus can end up nowhere: the focused input was in a panel that closed, or an
            // editor that was dropped. Keys would then reach none of the app's handlers, so it
            // goes back to where the human is working.
            cx.on_focus_lost(window, |this, window, cx| this.restore_focus(window, cx)),
            cx.observe_window_activation(window, |this, window, cx| {
                this.window_active = window.is_window_active();
                if this.window_active && this.follows_system {
                    this.read_reduce_motion(cx);
                }
            }),
            // Quitting writes what the app remembers first, not a beat later.
            cx.on_app_quit(|this, _| {
                this.persist();
                std::future::ready(())
            }),
        ];
        // A Mac app's window closes, the app stays: closing (Cmd+W, the red button) hides it, so
        // it keeps watching the projects and telling of what arrives; the Dock brings it back.
        if cfg!(target_os = "macos") {
            window.on_window_should_close(cx, |_, cx| {
                cx.hide();
                false
            });
        }
        // A click on the notification brings the window forward on what it told of.
        let this = cx.entity().downgrade();
        let handle = window.window_handle();
        cx.on_system_notification_response(move |response, cx| {
            if !response.tag.starts_with(NOTIFICATION) {
                return;
            }
            cx.activate(true);
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            let _ = this.update(cx, |app, cx| {
                app.open_notified(&response.tag);
                cx.notify();
            });
        });
        // Claude, the CLI and git change the log from outside; look every 200 ms.
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let alive = this.update(cx, |app, cx| {
                    // Whatever below tells the human something (a tick, a write that failed),
                    // or lets a notice go, shows now: a notice is drawn only when painted.
                    let noticed = told(&app.model);
                    let changed = app.model.tick() | app.model.refresh_docs();
                    // A hidden window draws nothing, and the Dock's count must still be right.
                    app.sync_menus(cx);
                    let arrived = app.model.take_arrivals();
                    if !arrived.is_empty() {
                        log::info!(
                            "inbox: {} arrived ({}), window {}",
                            arrived.len(),
                            arrived
                                .iter()
                                .map(|a| a.key.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                            if app.window_active {
                                "in front: not told"
                            } else {
                                "away: telling"
                            }
                        );
                    }
                    if !arrived.is_empty() && !app.window_active {
                        app.notify_arrived(arrived, cx);
                    } else {
                        // In front: told in the window, with a way to it.
                        for a in arrived {
                            app.model.notify(
                                Toast::info(a.title.clone())
                                    .title(format!("{} waits for you", a.key))
                                    .mark(Mark::Inbox)
                                    .about(
                                        app.model
                                            .projects
                                            .get(a.project)
                                            .map_or("livtk".to_owned(), |p| p.name.clone()),
                                        "Waits for you",
                                    )
                                    .action("Open", Act::Open(a)),
                            );
                        }
                    }
                    app.run_catch_ups(cx);
                    app.run_hooks(cx);
                    let busy = !app.model.flashed.is_empty() || !app.model.leaving().is_empty();
                    let now = Instant::now();
                    app.model.settle(now, FLASH.max(FADE) + POLL);
                    if app.model.age_notices(now, NOTICE_FADE) {
                        app.end_notice_fade(cx);
                    }
                    // What the app remembers is written within a beat of changing.
                    app.persist();
                    let told = told(&app.model) != noticed;
                    let toasts = app.model.toasts();
                    app.expanded.retain(|id| toasts.contains(*id));
                    if changed || busy || told {
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
            modal_focus: cx.focus_handle(),
            flow,
            follows_system: false,
            list_scrolls: Default::default(),
            palette_input,
            palette_scroll: ScrollHandle::new(),
            board_scroll: ScrollHandle::new(),
            column_scrolls: Default::default(),
            grid_state,
            update: None,
            checking_updates: false,
            expanded: Default::default(),
            graph_bounds: Rc::new(Cell::new(Bounds::default())),
            graph_press: None,
            sidebar_width: SIDEBAR_WIDTH,
            sidebar_collapsed: false,
            rail_was: None,
            rail_epoch: 0,
            rail_from: None,
            reader,
            terminals: HashMap::new(),
            last_place: None,
            bottom_open: false,
            bottom_height: BOTTOM_HEIGHT,
            bottom_leaving: false,
            bottom_epoch: 0,
            top_field_hover: false,
            top_field_epoch: 0,
            sidebar_peek: false,
            panel_width: PANEL_WIDTH,
            resizing: None,
            pane_bounds: Default::default(),
            split_bounds: Default::default(),
            split_drag: None,
            edge_hover: None,
            edge_leaving: None,
            edge_epoch: 0,
            edge_inside: None,
            saved_path: None,
            last_saved: Saved::default(),
            filter,
            note,
            note_for: None,
            note_drafts: HashMap::new(),
            form: None,
            help: false,
            about: false,
            settings: false,
            menu_state: None,
            editing: None,
            last_detail: None,
            detail_leaving: None,
            detail_epoch: 0,
            glide: None,
            glide_epoch: 0,
            menu: None,
            menu_closed: None,
            window_active: window.is_window_active(),
            notified: Vec::new(),
            notifications: 0,
            launch: Uuid::now_v7(),
            _subscriptions: subscriptions,
            _poll: poll,
        }
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// The model, for tests to put it in a state no headless frame lands on: the test executor
    /// finishes a background catch-up before it draws.
    pub fn model_mut(&mut self) -> &mut Model {
        &mut self.model
    }

    pub fn help_open(&self) -> bool {
        self.help
    }

    /// Tell of what arrived in the inbox while the window is away: one item by its key and
    /// title, several by their count and keys, in a notification of its own.
    fn notify_arrived(&mut self, arrived: Vec<Arrival>, cx: &mut Context<Self>) {
        let many = self.model.projects.len() > 1;
        let (title, body) = match arrived.as_slice() {
            [one] => {
                let project = self
                    .model
                    .projects
                    .get(one.project)
                    .filter(|_| many)
                    .map_or(String::new(), |p| format!("{} · ", p.name));
                (
                    format!("{project}{} waits for you", one.key),
                    one.title.clone(),
                )
            }
            all => (
                format!("{} things wait for you", all.len()),
                all.iter()
                    .map(
                        |a| match self.model.projects.get(a.project).filter(|_| many) {
                            Some(p) => format!("{} {}", p.name, a.key),
                            None => a.key.clone(),
                        },
                    )
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        };
        self.notifications += 1;
        let tag = format!("{NOTIFICATION}-{}-{}", self.launch, self.notifications);
        cx.show_system_notification(SystemNotification {
            tag: tag.clone().into(),
            title: title.into(),
            body: body.into(),
            actions: Vec::new(),
        });
        self.notified.push((tag, arrived));
        if self.notified.len() > NOTIFIED_KEPT {
            self.notified.remove(0);
        }
    }

    /// The notification tagged `tag` was clicked: its one item open in the inbox, or the inbox
    /// of the first of several; one too old to be remembered opens the inbox.
    fn open_notified(&mut self, tag: &str) {
        let told = self
            .notified
            .iter()
            .find(|(t, _)| t == tag)
            .map(|(_, arrived)| arrived.clone())
            .unwrap_or_default();
        match told.as_slice() {
            [one] => self.model.open_arrival(one),
            [first, ..] => {
                self.model.select_project(first.project);
                self.model.set_view(View::Inbox);
            }
            [] => self.model.set_view(View::Inbox),
        }
    }

    /// A notice began to fade: take it away the moment the fade ends, not at the next poll, as
    /// it would otherwise stand there unseen, taking clicks meant for what is under it.
    fn end_notice_fade(&mut self, cx: &mut Context<Self>) {
        let (shown, _) = self.model.toasts().shown();
        if shown.iter().all(|t| t.leaving.is_none()) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_FADE).await;
            let _ = this.update(cx, |app, cx| {
                if app.model.age_notices(Instant::now(), NOTICE_FADE) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Catch stores up off the window's thread: a replay after a git pull takes as long as the
    /// log is long, and the window keeps showing what it had meanwhile.
    fn run_catch_ups(&mut self, cx: &mut Context<Self>) {
        for job in self.model.take_catch_ups() {
            let work = job.clone();
            let done = cx.background_executor().spawn(async move { work.run() });
            cx.spawn(async move |this, cx| {
                let result = done.await;
                let _ = this.update(cx, |app, cx| {
                    app.model.caught_up(&job, result);
                    app.model.tick();
                    app.run_catch_ups(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Whether an edge is being dragged.
    pub fn resizing(&self) -> bool {
        self.resizing.is_some()
    }

    pub fn form_open(&self) -> bool {
        self.form.is_some()
    }

    /// The open menu.
    pub fn menu_open(&self) -> Option<&'static str> {
        self.menu.map(|m| match m {
            Menu::Filter => "filter",
            Menu::Grouping => "grouping",
            Menu::Ordering => "ordering",
            Menu::View => "view",
            Menu::Priority => "priority",
            Menu::RowPriority(_) => "row-priority",
            Menu::DetailPriority => "detail-priority",
            Menu::AddLink => "add-link",
            Menu::RowState(_) => "row-state",
            Menu::DetailState => "detail-state",
            Menu::Standing => "standing",
            Menu::Projects => "projects",
            Menu::GridWhat => "grid-what",
        })
    }

    fn on_note(
        &mut self,
        state: &Entity<TextareaState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The field reports Shift+Enter too, after breaking the line: only Enter adds the note.
        if let InputEvent::PressEnter { shift: false, .. } = event {
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
            InputEvent::Change if self.model.view() == View::Docs => {
                let text = state.read(cx).value().to_string();
                if text != self.model.docs.query {
                    self.model.search_docs(&text);
                    self.reader.list.scroll_to_item(0);
                    cx.notify();
                }
            }
            InputEvent::Change => {
                let text = state.read(cx).value().to_string();
                if text != self.model.display().filter {
                    self.model.set_filter(&text);
                    cx.notify();
                }
            }
            InputEvent::PressEnter { .. } => {
                self.menu = None;
                self.focus_main(window, cx);
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

    /// Give the keys to what is in front: the new-task form, the palette, the find bar, else the
    /// list.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.help || self.about || self.settings {
            self.modal_focus.focus(window, cx);
        } else if let Some(form) = &self.form {
            form.title.read(cx).focus_handle(cx).focus(window, cx);
        } else if self.model.palette.is_some() {
            self.palette_input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        } else if let Some(editor) = &self.reader.editor {
            editor.read(cx).focus_handle(cx).focus(window, cx);
        } else if self.reader.finding {
            self.reader.find.read(cx).focus_handle(cx).focus(window, cx);
        } else {
            self.focus_main(window, cx);
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

    pub fn bottom_open(&self) -> bool {
        self.bottom_open
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
        // The shortcuts and About give way: the palette must be on top to have the keys.
        self.help = false;
        self.about = false;
        self.menu = None;
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
        self.focus_main(window, cx);
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
            Some(Ask::Help) => self.set_help(true, window, cx),
            Some(Ask::SplitRight) => self.split_pane(PaneAxis::Row, window, cx),
            Some(Ask::SplitDown) => self.split_pane(PaneAxis::Column, window, cx),
            Some(Ask::ClosePane) => self.close_pane(window, cx),
            Some(Ask::NextPane) => self.next_pane(1, window, cx),
            Some(Ask::NewTerminal) => self.new_terminal(window, cx),
            Some(Ask::PickLink) | None => {}
        }
        self.after_move(cx);
    }

    /// Show the current view's filter text in the filter field.
    fn sync_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = if self.model.view() == View::Docs {
            self.model.docs.query.clone()
        } else {
            self.model.display().filter
        };
        if self.filter.read(cx).value().as_ref() != want {
            self.filter
                .update(cx, |s, cx| s.set_value(want.clone(), window, cx));
        }
    }

    /// The keys back to the main area: the terminal in its view, else the list. Where every
    /// overlay, menu and field gives them back.
    fn focus_main(&self, window: &mut Window, cx: &mut App) {
        match self
            .current_terminal()
            .filter(|_| self.model.view() == View::Terminal)
        {
            Some(pane) => pane.read(cx).focus_handle().clone().focus(window, cx),
            None => self.nav.focus(window, cx),
        }
    }

    /// Whether the docs' find bar is open, and whether its field has the keys.
    pub fn doc_find(&self, window: &Window, cx: &App) -> (bool, bool) {
        (
            self.reader.finding,
            self.reader
                .find
                .read(cx)
                .focus_handle(cx)
                .is_focused(window),
        )
    }

    /// Whether the terminal has the keys.
    pub fn terminal_focused(&self, window: &Window, cx: &App) -> bool {
        self.current_terminal()
            .is_some_and(|pane| pane.read(cx).focus_handle().is_focused(window))
    }

    /// A link in a doc, followed: a node's key opens it in the panel, another doc opens here, a
    /// web page in the browser.
    fn follow_link(&mut self, href: &str, cx: &mut Context<Self>) {
        if let Some(key) = href.strip_prefix("livtk:") {
            self.model.show_node(key);
        } else {
            let from = self.model.docs.open.clone().unwrap_or_default();
            match livtk_core::docs::resolve(&from, href) {
                Some(livtk_core::docs::Target::Doc { path, .. }) => self.model.open_doc(&path),
                Some(livtk_core::docs::Target::Web(url)) => cx.open_url(&url),
                Some(livtk_core::docs::Target::Anchor(_)) | None => {}
            }
        }
        cx.notify();
    }

    /// The open project's terminal shown, once it has one.
    fn current_terminal(&self) -> Option<&Entity<terminal::TerminalPane>> {
        self.terminals.get(&self.model.project()?.dir)?.current()
    }

    /// Arriving at a terminal (by any way: a key, the sidebar, the palette, another project)
    /// gives it the keys; leaving one gives them back to the list.
    fn focus_on_arrival(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.model.panes()[self.model.focused()].id;
        let place = (pane, self.model.view(), self.model.current);
        if self.last_place == Some(place) {
            return;
        }
        let was_terminal = self.last_place.is_some_and(|(_, v, _)| v == View::Terminal);
        self.last_place = Some(place);
        // However it was arrived at (a link, the palette, the sidebar), the filter shows this
        // view's.
        self.sync_filter(window, cx);
        if let Some(pane) = self.terminal_view(cx) {
            let focus = pane.read(cx).focus_handle().clone();
            focus.focus(window, cx);
        } else if was_terminal {
            self.focus_main(window, cx);
        }
    }

    fn go(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        self.model.set_view(view);
        self.sync_filter(window, cx);
        cx.notify();
    }

    fn after_move(&mut self, cx: &mut Context<Self>) {
        if self.model.view() == View::Activity {
            let at = self
                .model
                .feed()
                .iter()
                .position(|r| *r == FeedRow::Event(self.model.activity_index));
            if let Some(i) = at {
                self.list_scroll().scroll_to_item(i, ScrollStrategy::Center);
            }
        } else if self.model.view() == View::Docs {
            self.reader.list.scroll_to_item(self.model.docs.index);
        } else if self.model.on_board() {
            self.reveal_card();
        } else if let Some(i) = self.display_index() {
            self.list_scroll().scroll_to_item(i, ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// The board's column holding the selected card scrolled into view, and the card within it.
    fn reveal_card(&self) {
        let Some(id) = self.model.selected().map(|r| r.id) else {
            return;
        };
        let at = self
            .model
            .columns()
            .iter()
            .enumerate()
            .find_map(|(c, (key, rows))| {
                rows.iter().position(|r| r.id == id).map(|i| (c, *key, i))
            });
        if let Some((c, key, i)) = at {
            self.board_scroll.scroll_to_item(c);
            // The column's header comes first, then its cards.
            self.column_scroll(key).scroll_to_item(i + 1);
        }
    }

    /// The list scrolling of the pane acted on.
    fn list_scroll(&self) -> UniformListScrollHandle {
        let id = self.model.panes()[self.model.focused()].id;
        self.list_scrolls
            .borrow_mut()
            .entry(id)
            .or_default()
            .clone()
    }

    fn column_scroll(&self, key: GroupKey) -> ScrollHandle {
        self.column_scrolls
            .borrow_mut()
            .entry(key)
            .or_default()
            .clone()
    }

    /// `j` or `k`: move the selection `by` one, its highlight gliding there when the two rows
    /// are next to each other in a list (no band between them, not on the board or the graph).
    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        // Reading a doc, J and K scroll it a few lines.
        if self.model.view() == View::Docs && self.model.docs.reading {
            let scroll = &self.reader.scroll;
            let o = scroll.offset();
            let y = (o.y - px(by as f32 * READ_STEP))
                .min(px(0.))
                .max(-scroll.max_offset().y);
            scroll.set_offset(point(o.x, y));
            cx.notify();
            return;
        }
        let before = self.spot();
        self.model.select_by(by);
        let after = self.spot();
        self.glide = match (before, after) {
            (Some((from, i)), Some((to, j))) if i.abs_diff(j) == 1 => {
                self.glide_epoch += 1;
                let epoch = self.glide_epoch;
                // Done, the row draws its own highlight again.
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(GLIDE).await;
                    let _ = this.update(cx, |app, cx| {
                        if app.glide.as_ref().is_some_and(|g| g.epoch == epoch) {
                            app.glide = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
                Some(Glide {
                    from,
                    to,
                    down: j > i,
                    epoch,
                })
            }
            _ => None,
        };
        self.after_move(cx);
    }

    /// The selection as a list shows it: the name its line goes by, and the line's place.
    fn spot(&self) -> Option<(String, usize)> {
        match self.model.view() {
            View::Graph | View::Terminal => None,
            View::Docs if self.model.docs.reading => None,
            View::Docs => {
                let i = self.model.docs.index;
                Some((self.model.docs.rows().get(i)?.path().to_owned(), i))
            }
            View::Tasks if self.model.layout() == Layout::Board => None,
            View::Activity => {
                let i = self.model.activity_index;
                let at = self
                    .model
                    .feed()
                    .iter()
                    .position(|r| *r == FeedRow::Event(i))?;
                Some((format!("event-{i}"), at))
            }
            View::Inbox => {
                let i = self.model.selected_index()?;
                Some((self.model.rows().get(i)?.key.clone(), i))
            }
            _ => Some((self.model.selected()?.key.clone(), self.display_index()?)),
        }
    }

    /// How the line named `name` draws the selection: its own highlight (when it is selected
    /// and nothing glides), and a gliding one from and to these offsets, in rows. The glide is
    /// drawn by the upper of its two lines, under the lower one's text: lines paint top to
    /// bottom, so from the lower one it would cover the text above.
    fn highlight(&self, name: &str, selected: bool) -> (bool, Option<(f32, f32, usize)>) {
        let current = match self.model.view() {
            View::Activity => Some(format!("event-{}", self.model.activity_index)),
            View::Docs => self
                .model
                .docs
                .rows()
                .get(self.model.docs.index)
                .map(|r| r.path().to_owned()),
            _ => self.model.selected().map(|r| r.key.clone()),
        };
        let Some(g) = self
            .glide
            .as_ref()
            .filter(|g| Some(&g.to) == current.as_ref())
        else {
            return (selected, None);
        };
        let moving = match (g.down, name == g.from, name == g.to) {
            (true, true, _) => Some((0., 1., g.epoch)),
            (false, _, true) => Some((1., 0., g.epoch)),
            _ => None,
        };
        (selected && name != g.to, moving)
    }

    fn choose(&mut self, choice: &Choice, cx: &mut Context<Self>) {
        self.model.choose(choice);
        cx.notify();
    }

    fn choice_where(&mut self, pick: impl Fn(&Choice) -> bool, cx: &mut Context<Self>) {
        let found = self
            .model
            .acted_on()
            .map(|r| self.model.choices(r.kind, r.state))
            .and_then(|c| c.into_iter().find(|c| pick(c)));
        if let Some(c) = found {
            self.choose(&c, cx);
        }
    }

    /// New Task, from anywhere: the form, over whatever else was open. One already open keeps its
    /// half-typed text and gets the keys back.
    fn open_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = &self.form {
            form.title.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        self.close_overlays(window, cx);
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
        // The body's textarea takes Ctrl+Enter itself, so the form's binding never sees it: it
        // reports it as a secondary Enter (after putting in the newline Enter does).
        let sub = cx.subscribe_in(&body, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter {
                secondary: true, ..
            } = event
            {
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
        // Without the newline Ctrl+Enter leaves at the end.
        let body = form.body.read(cx).value().trim_end().to_owned();
        if self.model.create_task(&title, &body, Some(form.priority)) {
            self.form = None;
            self.sync_filter(window, cx);
            self.focus_main(window, cx);
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
            self.focus_main(window, cx);
        }
        cx.notify();
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            self.focus_main(window, cx);
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
                .child(indented(
                    div().child(Textarea::new(state).appearance(false).xsmall().text_sm()),
                    state,
                ))
                .into_any_element();
        }
        let empty = n.body.trim().is_empty();
        div()
            .id("detail-body")
            .debug_selector(|| "detail-body".into())
            .mx(px(-8.))
            .p(px(8.))
            .rounded_xl()
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

    /// Show the shortcuts, or hide them. While shown they have the keys, as About does, so a key
    /// meant for the list never acts on a task behind them; hidden, what is under them has the
    /// keys again: a half-typed form, or the list.
    fn set_help(&mut self, show: bool, window: &mut Window, cx: &mut Context<Self>) {
        if show {
            self.show_over_form(window, cx);
            self.help = true;
            self.modal_focus.focus(window, cx);
        } else {
            self.help = false;
            self.restore_focus(window, cx);
        }
        cx.notify();
    }

    /// Make way for the shortcuts or About: every other overlay goes but a new task's form,
    /// which stays under them with its text, as nothing typed is ever lost behind an overlay.
    fn show_over_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
        self.model.close_palette();
        self.help = false;
        self.about = false;
    }

    /// Follow the system's settings from now on: Reduce Motion now, and again each time the
    /// window comes forward (it may have changed meanwhile).
    pub fn follow_system(&mut self, cx: &mut Context<Self>) {
        self.follows_system = true;
        self.read_reduce_motion(cx);
    }

    /// Look for updates now and every 6 hours after: the installed app does (a development
    /// build or a test looks only when asked).
    pub fn watch_for_updates(&mut self, cx: &mut Context<Self>) {
        const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
        self.check_updates(false, cx);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(EVERY).await;
                if this
                    .update(cx, |app, cx| app.check_updates(false, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    /// Look for a newer build and stage it, off the main thread. Asked from the menu
    /// (`asked`), the answer is told either way; on its own, only a build ready is.
    fn check_updates(&mut self, asked: bool, cx: &mut Context<Self>) {
        if let Some(staged) = &self.update {
            // Staged already: told again when asked, never unpacked again under the menu.
            if asked {
                self.model.inform(format!(
                    "livtk build {} is ready: Restart to Update.",
                    staged.manifest.build
                ));
                cx.notify();
            }
            return;
        }
        if self.checking_updates {
            return;
        }
        self.checking_updates = true;
        let looked =
            cx.background_spawn(async { crate::updates::check(APP_ID, livtk_build::BUILD) });
        cx.spawn(async move |this, cx| {
            let found = looked.await;
            let _ = this.update(cx, |app, cx| {
                app.checking_updates = false;
                match found {
                    Ok(Some(staged)) => {
                        log::info!("update: build {} staged", staged.manifest.build);
                        app.model.notify(
                            Toast::success("Restart to install it when it suits you.")
                                .title(format!("livtk build {} is ready", staged.manifest.build))
                                .mark(Mark::Meteor)
                                .about("livtk", "Update")
                                .action("Restart to Update", Act::RestartToUpdate),
                        );
                        app.update = Some(staged);
                    }
                    Ok(None) if asked => app
                        .model
                        .inform(format!("livtk is up to date: {}.", livtk_build::DESCRIBE)),
                    Ok(None) => {}
                    Err(e) => {
                        log::warn!("update: {e}");
                        if asked {
                            app.model.warn(format!("Could not update: {e}"));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Restart to Update: start the helper that installs the staged build once this app has
    /// quit, and quit (what the app remembers is written on the way out).
    fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        let Some(staged) = &self.update else { return };
        let exe = std::env::current_exe().and_then(std::fs::canonicalize);
        match exe
            .map_err(|e| e.to_string())
            .and_then(|exe| crate::updates::install(staged, &exe))
        {
            Ok(()) => {
                log::info!("update: installing build {}", staged.manifest.build);
                cx.quit();
            }
            Err(e) => {
                self.model.warn(format!("Could not update: {e}"));
                cx.notify();
            }
        }
    }

    /// Ask the system whether it wants less motion, off the main thread, and do as it says.
    fn read_reduce_motion(&mut self, cx: &mut Context<Self>) {
        let asked = cx.background_spawn(async { crate::system::reduce_motion() });
        cx.spawn(async move |_, cx| {
            let calm = asked.await;
            cx.update(|cx| cx.set_reduce_motion(calm));
        })
        .detach();
    }

    /// Close the overlay on top, and only it (About, the shortcuts, the palette, then the form),
    /// and give the keys to what is now on top.
    fn close_top(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings {
            self.settings = false;
        } else if self.about {
            self.about = false;
        } else if self.help {
            self.help = false;
        } else if self.model.palette.is_some() {
            self.model.close_palette();
        } else {
            self.form = None;
        }
        self.menu = None;
        self.restore_focus(window, cx);
        cx.notify();
    }

    fn close_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model.close_palette();
        self.menu = None;
        self.form = None;
        self.help = false;
        self.about = false;
        self.settings = false;
        self.focus_main(window, cx);
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
        let mut items = Vec::with_capacity(self.model.rows().len() + self.model.groups().len());
        for (gi, g) in self.model.groups().iter().enumerate() {
            if g.key != GroupKey::All {
                items.push(Item::Band(gi));
            }
            items.extend((g.start..g.start + g.shown).map(Item::Row));
        }
        items
    }

    fn display_index(&self) -> Option<usize> {
        if let Some(band) = self.model.band() {
            let gi = self.model.groups().iter().position(|g| g.key == band)?;
            return self
                .items()
                .iter()
                .position(|it| matches!(it, Item::Band(i) if *i == gi));
        }
        let sel = self.model.selected_index()?;
        self.items()
            .iter()
            .position(|it| matches!(it, Item::Row(i) if *i == sel))
    }

    // ---- rendering: the frame -----------------------------------------------------------------

    fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.rail(window) {
            return self.rail_bar(cx);
        }
        let mut side = div()
            .debug_selector(|| "sidebar".into())
            .w(px(self.sidebar_width))
            .flex_none()
            .h_full()
            .relative()
            .flex()
            .flex_col()
            .px_2()
            .pb_3()
            .pt_1()
            .gap(px(2.));

        let inbox = self.model.project().map(|p| p.inbox).unwrap_or(0);
        let unseen = self.model.project().map(|p| p.unseen).unwrap_or(0);
        // The inbox and the feed first, under no heading; then the views of the project's work;
        // then the tools to work on it with.
        for view in View::ALL {
            if view == View::Tasks {
                side = side.child(div().h(px(14.))).child(section_label("Views"));
            }
            if view == View::Graph {
                side = side.child(div().h(px(14.))).child(section_label("Tools"));
            }
            let count = match view {
                View::Inbox => inbox,
                View::Activity => unseen,
                _ => 0,
            };
            side = side.child(
                nav_item(
                    SharedString::from(format!("view-{}", view.title())),
                    view_icon(view),
                    view.title(),
                    view == self.model.view() && !self.model.placeholder(),
                    count,
                )
                .on_click(cx.listener(move |this, _, window, cx| this.go(view, window, cx))),
            );
        }
        side.child(div().flex_1())
            .when_some(self.update.as_ref(), |side, staged| {
                side.child(
                    div()
                        .id("restart-to-update")
                        .debug_selector(|| "restart-to-update".into())
                        .px_2()
                        .h(px(NAV_ROW))
                        .text_size(px(NAV_TEXT))
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded_lg()
                        .text_color(color(ACCENT_HOVER))
                        .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT)))
                        .child(div().size(px(6.)).rounded_full().bg(color(ACCENT)))
                        .child(div().flex_1().child("Restart to Update"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(color(TEXT_4))
                                .child(format!("build {}", staged.manifest.build)),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.restart_to_update(cx))),
                )
            })
            .child(
                div()
                    .id("shortcuts")
                    .group("nav")
                    .px_2()
                    .h(px(NAV_ROW))
                    .text_size(px(NAV_TEXT))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_lg()
                    .text_color(color(TEXT_4))
                    .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_3)))
                    .child(icon_hover(Icon::Shortcuts, NAV_ICON, TEXT_4, "nav", TEXT_3))
                    .child(div().flex_1().child("Shortcuts"))
                    .child(kbd(keys::MOD))
                    .child(kbd("?"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.set_help(!this.help, window, cx)),
                    ),
            )
            .child(self.edge(Edge::Sidebar, window, cx).right_0())
            .into_any_element()
    }

    /// A draggable edge: the sidebar's right, the panel's left, spanning the gutter beside the
    /// content box, so its middle is 4 px from the box on either side. Nothing shows at rest. On
    /// hover a glow follows the pointer along it, the handle at its middle (the cursor stays, by
    /// the developer's rule); dragging brightens it. A double-click puts the width back to its
    /// default. The caller places it: `right_0` beside the sidebar and the content, or across
    /// a floating panel's border.
    fn edge(&self, edge: Edge, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = match edge {
            Edge::Sidebar => "sidebar-edge",
            Edge::Panel => "panel-edge",
            Edge::Bottom => "bottom-edge",
        };
        let dragging = self.resizing == Some(edge);
        // The side edges run the height under the top bar; the bottom one the content's width.
        let len = match edge {
            Edge::Bottom => self.window_content_width(window),
            _ => f32::from(window.viewport_size().height) - TOP_BAR,
        };
        let glow = match (self.edge_hover, self.edge_leaving) {
            (Some(h), _) if h.edge == edge => Some(edge_glow(h, dragging, len, false)),
            (_, Some(h)) if h.edge == edge => Some(edge_glow(h, false, len, true)),
            _ => None,
        };
        let el = div().id(id).debug_selector(move || id.into()).absolute();
        let el = if edge == Edge::Bottom {
            el.left_0().right_0().h(px(EDGE_W))
        } else {
            el.top_0().bottom_0().w(px(EDGE_W))
        };
        el.children(glow)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.edge_inside = Some(edge);
                    return;
                }
                if this.edge_inside == Some(edge) {
                    this.edge_inside = None;
                }
                // Dragging keeps it lit wherever the pointer goes.
                if this.resizing != Some(edge) && this.edge_hover.is_some_and(|h| h.edge == edge) {
                    this.let_go_edge(cx);
                }
            }))
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, window, cx| {
                let at = this.along(edge, e.position, window);
                this.hover_edge(edge, at);
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if e.click_count >= 2 {
                        match edge {
                            Edge::Sidebar => this.sidebar_width = SIDEBAR_WIDTH,
                            Edge::Panel => this.panel_width = PANEL_WIDTH,
                            Edge::Bottom => this.bottom_height = BOTTOM_HEIGHT,
                        }
                        this.resizing = None;
                    } else {
                        this.resizing = Some(edge);
                    }
                    cx.notify();
                }),
            )
    }

    /// The glow lets go: it fades out where it was, then is gone.
    fn let_go_edge(&mut self, cx: &mut Context<Self>) {
        let Some(h) = self.edge_hover.take() else {
            return;
        };
        self.edge_leaving = Some(h);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(GLOW_OUT).await;
            let _ = this.update(cx, |this, cx| {
                if this.edge_leaving.is_some_and(|l| l.epoch == h.epoch) {
                    this.edge_leaving = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// How far along `edge` the window's point `p` is: down a side edge, from under the top
    /// bar; across the bottom one, from the content's left.
    fn along(&self, edge: Edge, p: Point<Pixels>, window: &Window) -> f32 {
        match edge {
            Edge::Bottom => f32::from(p.x) - self.sidebar_px(window),
            _ => f32::from(p.y) - TOP_BAR,
        }
    }

    /// The pointer over `edge`, `at` along it: the glow follows it, fading in when it arrives.
    fn hover_edge(&mut self, edge: Edge, at: f32) {
        self.edge_leaving = None;
        let epoch = match self.edge_hover {
            Some(h) if h.edge == edge => h.epoch,
            _ => {
                self.edge_epoch += 1;
                self.edge_epoch
            }
        };
        self.edge_hover = Some(EdgeHover { edge, at, epoch });
    }

    /// The pointer at `p` while dragging `edge` in a window `size`. The sidebar's width is where
    /// the pointer is; dragged under the snap, it becomes the rail. The panel's width is what lies
    /// right of the pointer, the bottom bar's height what lies below it.
    fn drag_edge(&mut self, edge: Edge, p: Point<Pixels>, size: Size<Pixels>) {
        let (x, y) = (f32::from(p.x), f32::from(p.y));
        let (window_w, window_h) = (f32::from(size.width), f32::from(size.height));
        match edge {
            Edge::Sidebar if x < SIDEBAR_SNAP => {
                if window_w < RAIL_BELOW {
                    self.sidebar_peek = false;
                } else {
                    self.sidebar_collapsed = true;
                }
                self.resizing = None;
            }
            Edge::Sidebar => self.sidebar_width = x.clamp(SIDEBAR_MIN, SIDEBAR_MAX),
            Edge::Panel => self.panel_width = (window_w - x).clamp(PANEL_MIN, PANEL_MAX),
            // Under the bar, the window's bottom inset.
            Edge::Bottom => {
                self.bottom_height = (window_h - y - BOTTOM_INSET).clamp(BOTTOM_MIN, BOTTOM_MAX)
            }
        }
    }

    /// ⌘B and the top bar's button. On a narrow window the sidebar is the rail anyway, so
    /// the toggle opens it there for now; on a wide one it is the human's choice, kept.
    fn toggle_sidebar(&mut self, window: &Window, cx: &mut Context<Self>) {
        if Self::window_width(window) < RAIL_BELOW {
            self.sidebar_peek = !self.sidebar_peek;
        } else {
            self.sidebar_collapsed = !self.sidebar_collapsed;
        }
        cx.notify();
    }

    /// The bottom bar, under the content box between the sidebar and the panel: black, edged as
    /// the content box is, its top edge dragged for its height. It holds the activity grid. However tall
    /// it is kept, the content above it keeps its room. It slides in and away as the detail
    /// panel does, its height animating so the content above gives way smoothly.
    fn bottom_bar(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let room = f32::from(window.viewport_size().height)
            - TOP_BAR
            - CONTENT_MIN_H
            - EDGE_W
            - BOTTOM_INSET;
        let h = self
            .bottom_height
            .min(room)
            .max(BOTTOM_MIN.min(room))
            .max(0.);
        let closing = !self.bottom_open;
        let epoch = self.bottom_epoch;
        let (id, duration) = match (closing, epoch) {
            (true, _) => (format!("bottom-out-{epoch}"), SLIDE_OUT),
            // Open from the start (brought back from the last run): no slide.
            (false, 0) => ("bottom-in-0".to_owned(), Duration::ZERO),
            (false, _) => (format!("bottom-in-{epoch}"), SLIDE),
        };
        let easing: fn(f32) -> f32 = if closing { ease_in_out } else { ease_out_cubic };
        let full = h + EDGE_W;
        div()
            .flex_none()
            .relative()
            .overflow_hidden()
            .pt(px(EDGE_W))
            .child(self.edge(Edge::Bottom, window, cx).top_0())
            .child(
                div()
                    .debug_selector(|| "bottom-bar".into())
                    .h(px(h))
                    .rounded_xl()
                    .bg(color(0x000000))
                    .border_1()
                    .border_color(color(BORDER))
                    .overflow_hidden()
                    .child(self.activity_grid(cx)),
            )
            .with_animation(
                SharedString::from(id),
                Animation::new(duration).with_easing(easing),
                move |el, t| {
                    let open = if closing { 1. - t } else { t };
                    el.h(px(full * open)).opacity(0.3 + 0.7 * open)
                },
            )
            .into_any_element()
    }

    /// ⌘J and the top bar's button: the bottom bar shown or hidden.
    /// It slides as the detail panel does: in, or away and then gone.
    fn toggle_bottom(&mut self, cx: &mut Context<Self>) {
        self.bottom_open = !self.bottom_open;
        self.bottom_epoch += 1;
        self.bottom_leaving = !self.bottom_open;
        if self.bottom_leaving {
            let epoch = self.bottom_epoch;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SLIDE_OUT).await;
                let _ = this.update(cx, |this, cx| {
                    if this.bottom_epoch == epoch {
                        this.bottom_leaving = false;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// ⌥⌘B and the top bar's button: the detail panel closed, or opened on what is selected,
    /// as Enter opens it.
    fn toggle_panel(&mut self, cx: &mut Context<Self>) {
        let reading_doc = self.model.view() == View::Docs && self.model.detail.is_none();
        let docs = &mut self.model.docs;
        if reading_doc && docs.reading {
            // Reading, the panel is the doc's.
            docs.hide_panel = !docs.hide_panel;
        } else if self.model.detail.is_some() {
            self.model.close_detail();
        } else {
            self.model.open_detail();
        }
        cx.notify();
    }

    /// The collapsed sidebar: a rail of icons. The project switcher, each view with a dot when
    /// it holds something new, and the shortcuts; names in tooltips.
    fn rail_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let inbox = self.model.project().map_or(0, |p| p.inbox);
        let unseen = self.model.project().map_or(0, |p| p.unseen);
        let mut rail = div()
            .debug_selector(|| "rail".into())
            .w(px(RAIL_WIDTH))
            .flex_none()
            .h_full()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .pb_3()
            .pt_1()
            .gap(px(2.));
        for view in View::ALL {
            // The inbox and the feed, the views of the work, the tools, as the sidebar groups
            // them.
            if matches!(view, View::Tasks | View::Graph) {
                rail = rail.child(div().h(px(14.)));
            }
            let new = match view {
                View::Inbox => inbox > 0,
                View::Activity => unseen > 0,
                _ => false,
            };
            rail = rail.child(
                rail_button(
                    SharedString::from(format!("rail-{}", view.title())),
                    view_icon(view),
                    view == self.model.view() && !self.model.placeholder(),
                    new,
                    view.title().into(),
                )
                .on_click(cx.listener(move |this, _, window, cx| this.go(view, window, cx))),
            );
        }
        rail.child(div().flex_1())
            .child(
                rail_button(
                    "rail-shortcuts".into(),
                    Icon::Shortcuts,
                    self.help,
                    false,
                    format!("Shortcuts · {}", keys::HELP).into(),
                )
                .on_click(cx.listener(|this, _, window, cx| this.set_help(!this.help, window, cx))),
            )
            .into_any_element()
    }

    /// The projects, from the rail: pick one, or open another.
    fn projects_menu(&self, cx: &mut Context<Self>) -> Div {
        let mut panel = menu_panel(260.).debug_selector(|| "projects-menu".into());
        for i in self.model.recent_projects() {
            let p = &self.model.projects[i];
            let current = i == self.model.current;
            panel = panel.child(
                option_row(SharedString::from(format!("menu-project-{i}")))
                    .debug_selector(move || format!("menu-project-{i}"))
                    .child(icon(Icon::Project, 14., TEXT_3))
                    .child(div().flex_1().truncate().child(capitalized(&p.name)))
                    .when(p.catching_up, |d| d.child(catching_up(i)))
                    .when(p.inbox > 0, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(color(TEXT_4))
                                .child(p.inbox.to_string()),
                        )
                    })
                    .child(check(current))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.model.select_project(i);
                        this.sync_filter(window, cx);
                        this.close_menu(window, cx);
                    })),
            );
        }
        panel.child(menu_divider()).child(
            option_row("menu-open-project".into())
                .debug_selector(|| "menu-open-project".into())
                .child(icon(Icon::OpenProject, 14., TEXT_3))
                .child("Open another project…")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_menu(window, cx);
                    this.open_folder(cx);
                })),
        )
    }

    /// The bar across the window's top: on macOS the traffic lights at its left, and a press on
    /// it drags the window (a double-click zooms it, as the system's titlebar does). In its
    /// middle, the palette's field at rest: what it searches; a click, or its
    /// keys, opens the palette. At its right end, the panels' toggles.
    fn top_bar(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let field = div()
            .id("top-field")
            .debug_selector(|| "top-field".into())
            .w(px(TOP_FIELD_W))
            .max_w_full()
            .min_w_0()
            .h(px(30.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            // Edged as the content box is: its own black, its hairline.
            .rounded_lg()
            .bg(color(CONTENT))
            .border_1()
            .border_color(color(BORDER))
            .text_sm()
            .text_color(color(TEXT_3))
            .relative()
            // Its hover comes up and goes in a breath: a lighter layer over the field (under its
            // text), fading, so the field itself (what the pointer is over) never changes.
            .child({
                let (over, epoch) = (self.top_field_hover, self.top_field_epoch);
                div()
                    .absolute()
                    .top(px(-1.))
                    .left(px(-1.))
                    .right(px(-1.))
                    .bottom(px(-1.))
                    .rounded_lg()
                    .bg(color(HOVER))
                    .border_1()
                    .border_color(color(BORDER))
                    .opacity(if over { 1. } else { 0. })
                    .with_animation(
                        SharedString::from(format!("top-field-{epoch}")),
                        Animation::new(if epoch == 0 {
                            Duration::ZERO
                        } else {
                            HOVER_FADE
                        })
                        .with_easing(ease_out_cubic),
                        move |el, t| el.opacity(if over { t } else { 1. - t }),
                    )
            })
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if this.top_field_hover != *over {
                    this.top_field_hover = *over;
                    this.top_field_epoch += 1;
                    cx.notify();
                }
            }))
            .child(icon(Icon::Search, 15., TEXT_4))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child("Search tasks and commands"),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(color(TEXT_4))
                    .child(keys::PALETTE),
            )
            // Its press is its own: it never drags the window.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.toggle_palette(window, cx)));
        let titlebar = titlebar_height(window) > 0.;
        let open = self.menu == Some(Menu::Projects);
        let project = self.model.project();
        let catching = project.is_some_and(|p| p.catching_up);
        let name = project.map_or("No project".to_owned(), |p| capitalized(&p.name));
        let project_button = div()
            .id("top-project")
            .debug_selector(|| "top-project".into())
            .h(px(30.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_lg()
            .text_base()
            .font_weight(FontWeight::MEDIUM)
            .text_color(color(TEXT_2))
            .when(open, |d| d.bg(color(SELECTED)))
            .when(!open, |d| d.hover(|h| h.bg(color(HOVER))))
            .child(div().max_w(px(220.)).truncate().child(name))
            .when(catching, |d| d.child(catching_up(self.model.current)))
            // Its press is its own: it never drags the window.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_menu(Menu::Projects, window, cx);
                }),
            );
        div()
            .id("top-bar")
            .debug_selector(|| "top-bar".into())
            .flex_none()
            .w_full()
            .h(px(TOP_BAR))
            .relative()
            .when(titlebar, |d| {
                d.on_mouse_down(MouseButton::Left, |e: &MouseDownEvent, window, _| {
                    if e.click_count == 2 {
                        window.titlebar_double_click();
                    } else {
                        window.start_window_move();
                    }
                })
            })
            // At its left, past the traffic lights: the open project, and the recent ones under it.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(if titlebar { TRAFFIC_ROOM } else { 12. }))
                    .flex()
                    .items_center()
                    .child(div().relative().child(project_button).when(open, |d| {
                        let panel = self.projects_menu(cx);
                        d.child(self.popover(Menu::Projects, false, 34., panel, cx))
                    })),
            )
            // Centred on the window, whatever sits at either end.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .px(px(120.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(field),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .pr_3()
                    .flex()
                    .items_center()
                    .child(panel_toggle(
                        "toggle-left",
                        Icon::PanelLeft,
                        !self.rail(window),
                        format!("Sidebar · {}", keys::SIDEBAR),
                        cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)),
                    ))
                    .child(panel_toggle(
                        "toggle-bottom",
                        Icon::PanelBottom,
                        self.bottom_open,
                        format!("Bottom bar · {}", keys::BOTTOM),
                        cx.listener(|this, _, _, cx| this.toggle_bottom(cx)),
                    ))
                    .child(panel_toggle(
                        "toggle-right",
                        Icon::PanelRight,
                        self.panel_open(),
                        format!("Detail panel · {}", keys::PANEL),
                        cx.listener(|this, _, _, cx| this.toggle_panel(cx)),
                    )),
            )
    }

    /// Open or close a menu from its button. A click on the button of the open menu first
    /// reads as a click outside it, which closes it; that must not reopen it.
    fn toggle_menu(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        let just_closed = self.menu_closed.take() == Some(menu);
        if just_closed || self.menu == Some(menu) {
            self.close_menu(window, cx);
        } else {
            self.menu = Some(menu);
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
        match &self.form {
            Some(form) => form.title.read(cx).focus_handle(cx).focus(window, cx),
            None => self.focus_main(window, cx),
        }
        cx.notify();
    }

    /// Esc closes the innermost thing open: a menu, then a modal.
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            self.close_menu(window, cx);
        } else {
            self.close_top(window, cx);
        }
    }

    /// A mouse-down outside `menu`. The flag lives only for this mouse-down: see `toggle_menu`.
    fn clicked_outside(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_closed = Some(menu);
        cx.defer_in(window, |this, _, _| this.menu_closed = None);
        self.close_menu(window, cx);
    }

    /// `panel` as `menu`'s popover, drawn above everything: anchored `top` below its button's
    /// left or right edge, or, from the dock at the bottom, `up` above the button.
    fn popover(
        &self,
        menu: Menu,
        right: bool,
        top: f32,
        panel: Div,
        cx: &mut Context<Self>,
    ) -> Div {
        self.popover_at(menu, right, false, top, panel, cx)
    }

    /// A menu opening upwards from the dock, `gap` above its button.
    fn popover_up(&self, menu: Menu, right: bool, panel: Div, cx: &mut Context<Self>) -> Div {
        self.popover_at(menu, right, true, -8., panel, cx)
    }

    fn popover_at(
        &self,
        menu: Menu,
        right: bool,
        up: bool,
        top: f32,
        panel: Div,
        cx: &mut Context<Self>,
    ) -> Div {
        let anchor = match (up, right) {
            (false, false) => Anchor::TopLeft,
            (false, true) => Anchor::TopRight,
            (true, false) => Anchor::BottomLeft,
            (true, true) => Anchor::BottomRight,
        };
        // Up, the panel rises from where it lands; down, it drops a little into place.
        let rise = if up { 4.0 } else { -4.0 };
        div()
            .absolute()
            .top(px(top))
            .when(right, |a| a.right_0())
            .when(!right, |a| a.left_0())
            .child(
                deferred(
                    anchored()
                        .anchor(anchor)
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
                                    move |el, t| el.opacity(t).mt(px(rise * (1.0 - t))),
                                ),
                        ),
                )
                .with_priority(1),
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
        let open = self.menu == Some(menu);
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
            .text_color(color(TEXT_2))
            .when(open, |b| {
                b.bg(color(SELECTED)).border_color(color(BORDER_STRONG))
            })
            .when(!open, |b| {
                b.border_color(color(BORDER))
                    .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
            })
            .child(current)
            // Small, as Linear keeps its chevrons: the filled caret is heavy at text size.
            .child(icon(Icon::CaretDown, 10., TEXT_4))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.toggle_menu(menu, window, cx)),
            );
        div().relative().child(button).when(open, |d| {
            let panel = panel(self, cx);
            d.child(self.popover(menu, false, 32., panel, cx))
        })
    }

    fn filter_menu(&self, cx: &mut Context<Self>) -> Div {
        let d = self.model.display();
        let panel = menu_panel(264.)
            .debug_selector(|| "filter-menu".into())
            .on_action(
                cx.listener(|this, _: &input::Escape, window, cx| this.close_menu(window, cx)),
            )
            .child(
                div()
                    .h(px(32.))
                    .pl(px(6.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(icon(Icon::Search, 14., TEXT_4))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.filter).appearance(false).small()),
                    ),
            );
        // The docs are searched by their words alone.
        if self.model.view() == View::Docs {
            return panel;
        }
        let mut panel = panel.child(menu_divider()).child(menu_heading("Priority"));
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
        // The states of the view's kind, in its own order, as its groups are; the inbox mixes
        // kinds and has none. Dismissed states only while dismissed work is shown.
        if let Some(kind) = self.model.view().kind() {
            let states: Vec<State> = kind
                .states()
                .iter()
                .copied()
                .filter(|s| d.show_dismissed || !s.is_dismissed())
                .collect();
            if states.len() > 1 {
                panel = panel.child(menu_divider()).child(menu_heading("State"));
                for st in states {
                    let on = d.states.contains(&st);
                    let id = format!("filter-state-{}", st.as_str());
                    panel = panel.child(
                        option_row(SharedString::from(id.clone()))
                            .debug_selector(move || id.clone())
                            .child(checkbox(on))
                            .child(state_mark(kind, st))
                            .child(state_name(st))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.model.toggle_state(st);
                                cx.notify();
                            })),
                    );
                }
            }
        }
        if !d.priorities.is_empty() || !d.states.is_empty() || !d.filter.trim().is_empty() {
            panel = panel.child(menu_divider()).child(
                option_row("clear-filters".into())
                    .debug_selector(|| "clear-filters".into())
                    .text_color(color(TEXT_3))
                    .child("Clear filters")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.model.clear_filters();
                        this.sync_filter(window, cx);
                        cx.notify();
                    })),
            );
        }
        panel
    }

    fn window_width(window: &Window) -> f32 {
        f32::from(window.viewport_size().width)
    }

    /// Whether the sidebar shows as the rail: as the human chose, or because the window is too
    /// narrow for it, unless they opened it there by hand.
    fn rail(&self, window: &Window) -> bool {
        if Self::window_width(window) < RAIL_BELOW {
            !self.sidebar_peek
        } else {
            self.sidebar_collapsed
        }
    }

    fn sidebar_px(&self, window: &Window) -> f32 {
        if self.rail(window) {
            RAIL_WIDTH
        } else {
            self.sidebar_width
        }
    }

    /// The panel's width: as dragged, within its limits and its share of the window.
    fn panel_px(&self, window: &Window) -> f32 {
        let cap = PANEL_MAX.min(Self::window_width(window) * PANEL_SHARE);
        self.panel_width.min(cap).max(PANEL_MIN.min(cap))
    }

    /// Whether the panel is open: on a node, or on the doc being read.
    fn panel_open(&self) -> bool {
        self.model.detail.is_some() || self.doc_panel_open()
    }

    /// Whether the panel shows the doc being read: nothing else open in it, and not put away.
    fn doc_panel_open(&self) -> bool {
        let docs = &self.model.docs;
        self.model.view() == View::Docs
            && docs.reading
            && docs.open.is_some()
            && !docs.hide_panel
            && self.reader.editor.is_none()
            && self.model.detail.is_none()
            && self.detail_leaving.is_none()
    }

    /// Whether the panel floats over the content: when beside it, it would leave the content
    /// narrower than it can be.
    fn panel_floats(&self, window: &Window) -> bool {
        Self::window_width(window) - self.sidebar_px(window) - CONTENT_FRAME - self.panel_px(window)
            < CONTENT_MIN
    }

    /// The content column's width, worked out from the window rather than measured, so a frame
    /// is laid out right the first time: the window, less the sidebar, the content's inset and
    /// border, and the detail panel when it is open beside it.
    fn content_width(&self, window: &Window) -> f32 {
        self.focused_width(self.window_content_width(window))
    }

    /// The content column's width, all panes together.
    fn window_content_width(&self, window: &Window) -> f32 {
        let panel = if self.panel_open() && !self.panel_floats(window) {
            self.panel_px(window)
        } else {
            0.
        };
        Self::window_width(window) - self.sidebar_px(window) - CONTENT_FRAME - panel
    }

    /// Remember between runs through `path`: bring back what it holds now, and write it again
    /// whenever something it holds changes.
    pub fn remember(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let saved = Saved::load(&path);
        self.model.restore(&saved);
        let w = &saved.window;
        self.sidebar_width = w
            .sidebar_width
            .unwrap_or(SIDEBAR_WIDTH)
            .clamp(SIDEBAR_MIN, SIDEBAR_MAX);
        self.sidebar_collapsed = w.sidebar_collapsed;
        self.panel_width = w
            .panel_width
            .unwrap_or(PANEL_WIDTH)
            .clamp(PANEL_MIN, PANEL_MAX);
        self.bottom_open = w.bottom_open;
        self.bottom_height = w
            .bottom_height
            .unwrap_or(BOTTOM_HEIGHT)
            .clamp(BOTTOM_MIN, BOTTOM_MAX);
        self.sync_filter(window, cx);
        self.last_saved = self.to_save();
        self.saved_path = Some(path);
        cx.notify();
    }

    fn to_save(&self) -> Saved {
        let mut saved = self.model.saved();
        saved.window = SavedWindow {
            sidebar_width: Some(self.sidebar_width),
            sidebar_collapsed: self.sidebar_collapsed,
            panel_width: Some(self.panel_width),
            bottom_open: self.bottom_open,
            bottom_height: Some(self.bottom_height),
        };
        saved
    }

    /// Write what is remembered, if it changed since the last write.
    fn persist(&mut self) {
        let Some(path) = &self.saved_path else { return };
        let now = self.to_save();
        if now != self.last_saved {
            if let Err(e) = now.save(path) {
                self.model.warn(format!("cannot save the app's state: {e}"));
            }
            self.last_saved = now;
        }
    }

    /// The dock: one compact bar floating over the bottom of the content, centred on it, holding
    /// what the view can do: a new task, the filter, grouping, ordering, view options (list or
    /// board, empty groups, dismissed work; the graph's dismissed work alone) and the graph's
    /// zoom; none in a view with nothing for it. Labels show where there is room; else the icons
    /// stand alone, named by their tooltips. Menus open upwards from it.
    fn dock(&self, window: &Window, cx: &mut Context<Self>) -> Option<Div> {
        let view = self.model.view();
        let wide = self.content_width(window) >= DOCK_WIDE;
        let lists = matches!(
            view,
            View::Tasks | View::Lessons | View::Decisions | View::Findings
        );
        let mut groups: Vec<Vec<AnyElement>> = Vec::new();
        if view == View::Tasks {
            groups.push(vec![self.dock_new_task(wide, cx)]);
        }
        if view == View::Graph {
            groups.push(self.dock_zoom(cx));
        }
        let mut shaping = Vec::new();
        let doc_list = view == View::Docs && !self.model.docs.reading;
        if lists || view == View::Inbox || doc_list {
            shaping.push(self.dock_filter(wide, cx));
        }
        if lists {
            shaping.push(self.dock_choice(Menu::Grouping, wide, cx));
            shaping.push(self.dock_choice(Menu::Ordering, wide, cx));
        }
        let docs = &self.model.docs;
        if doc_list && docs.query.trim().is_empty() && !docs.tree.folders.is_empty() {
            shaping.push(self.dock_fold_docs(cx));
        }
        if lists {
            shaping.push(self.dock_view_options(wide, cx));
        } else if view == View::Graph {
            shaping.push(self.dock_dismissed(cx));
        }
        if !shaping.is_empty() {
            groups.push(shaping);
        }
        // A view with nothing to do here (the feed) has no dock at all, not an empty one.
        if groups.is_empty() {
            return None;
        }

        let mut dock = div()
            .debug_selector(|| "dock".into())
            .id("dock")
            .h(px(DOCK_HEIGHT))
            .p(px(4.))
            .flex()
            .items_center()
            .gap(px(2.))
            .rounded_xl()
            // Black, as the sidebar: the window's own colour; its border sets it apart.
            .bg(color(WINDOW))
            .border_1()
            .border_color(color(BORDER_STRONG))
            .shadow_lg()
            .text_sm()
            // Presses here are the dock's: nothing under it is clicked or dragged through it.
            .occlude();
        for (i, group) in groups.into_iter().enumerate() {
            if i > 0 {
                dock = dock.child(div().w(px(1.)).h(px(20.)).mx(px(4.)).bg(color(BORDER)));
            }
            dock = dock.children(group);
        }
        Some(div().child(dock))
    }

    /// A button of the dock: an icon, a label when `label` is given, a tooltip always.
    fn dock_button(
        id: &'static str,
        glyph: Icon,
        label: Option<String>,
        tip: &'static str,
        on: bool,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .debug_selector(move || id.into())
            .group(id)
            .h(px(30.))
            .min_w(px(30.))
            .px(px(if label.is_some() { 10. } else { 7. }))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .rounded_lg()
            .when(on, |b| b.bg(color(SELECTED)).text_color(color(TEXT)))
            .when(!on, |b| {
                b.text_color(color(TEXT_3))
                    .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
            })
            .child(if on {
                icon(glyph, 16., TEXT)
            } else {
                icon_hover(glyph, 16., TEXT_3, id, TEXT)
            })
            .when_some(label, |b, l| b.child(div().whitespace_nowrap().child(l)))
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx)
            })
    }

    fn dock_new_task(&self, wide: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("dock-new-task")
            .debug_selector(|| "dock-new-task".into())
            .h(px(30.))
            .min_w(px(30.))
            .px(px(if wide { 10. } else { 7. }))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .rounded_lg()
            .bg(color(ACCENT))
            .text_color(color(TEXT))
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.bg(color(ACCENT_HOVER)))
            .child(icon(Icon::NewTask, 16., TEXT))
            .when(wide, |b| {
                b.child(div().whitespace_nowrap().child("New task"))
            })
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new("New task · N").build(window, cx)
            })
            .on_click(cx.listener(|this, _, window, cx| this.open_form(window, cx)))
            .into_any_element()
    }

    /// A dock button that opens `menu` upwards; the press is the button's alone.
    fn dock_menu(&self, menu: Menu, button: Stateful<Div>, cx: &mut Context<Self>) -> AnyElement {
        let open = self.menu == Some(menu);
        let button = button.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.toggle_menu(menu, window, cx);
            }),
        );
        div()
            .relative()
            .flex_none()
            .child(button)
            .when(open, |d| {
                let panel = match menu {
                    Menu::Filter => self.filter_menu(cx),
                    Menu::Grouping => self.grouping_menu(cx),
                    Menu::View => self.view_menu(cx),
                    _ => self.ordering_menu(cx),
                };
                d.child(self.popover_up(menu, false, panel, cx))
            })
            .into_any_element()
    }

    fn dock_filter(&self, wide: bool, cx: &mut Context<Self>) -> AnyElement {
        let d = self.model.display();
        let active = if self.model.view() == View::Docs {
            usize::from(!self.model.docs.query.trim().is_empty())
        } else {
            usize::from(!d.filter.trim().is_empty())
                + usize::from(!d.priorities.is_empty())
                + usize::from(!d.states.is_empty())
        };
        let label = match (wide, active) {
            (false, _) => None,
            (true, 0) => Some("Filter".to_owned()),
            (true, n) => Some(format!("Filter · {n}")),
        };
        let open = self.menu == Some(Menu::Filter);
        let button = Self::dock_button(
            "filter-button",
            Icon::Filter,
            label,
            "Filter · /",
            open || active > 0,
        );
        self.dock_menu(Menu::Filter, button, cx)
    }

    /// Grouping or Ordering: the current choice when there is room, its menu on a press.
    fn dock_choice(&self, menu: Menu, wide: bool, cx: &mut Context<Self>) -> AnyElement {
        let d = self.model.display();
        let (id, glyph, tip, current) = if menu == Menu::Grouping {
            let name = GROUPINGS
                .iter()
                .find(|g| g.2 == d.grouping)
                .map_or("State", |g| g.1);
            ("dock-grouping", Icon::Grouping, "Grouping", name)
        } else {
            let name = ORDERINGS
                .iter()
                .find(|o| o.2 == d.ordering)
                .map_or("Priority", |o| o.1);
            ("dock-ordering", Icon::Ordering, "Ordering", name)
        };
        let open = self.menu == Some(menu);
        let button = Self::dock_button(id, glyph, wide.then(|| current.to_owned()), tip, open);
        self.dock_menu(menu, button, cx)
    }

    /// How the list is shown, in a menu: lit while it is open or an option is on.
    fn dock_view_options(&self, wide: bool, cx: &mut Context<Self>) -> AnyElement {
        let d = self.model.display();
        let open = self.menu == Some(Menu::View);
        let button = Self::dock_button(
            "dock-view",
            Icon::ViewOptions,
            wide.then(|| "View".to_owned()),
            "View options",
            open || d.show_empty || d.show_dismissed,
        );
        self.dock_menu(Menu::View, button, cx)
    }

    /// The docs' folders all shut with a press, or all open again: lit while all are shut.
    fn dock_fold_docs(&self, cx: &mut Context<Self>) -> AnyElement {
        let shut = self.model.docs_all_shut();
        Self::dock_button(
            "dock-fold-docs",
            Icon::FoldAll,
            None,
            if shut { "Unfold all" } else { "Fold all" },
            shut,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.model.fold_docs(!shut);
            cx.notify();
        }))
        .into_any_element()
    }

    /// The graph's dismissed work: shown or hidden with a press, as it has nothing else to show.
    fn dock_dismissed(&self, cx: &mut Context<Self>) -> AnyElement {
        let on = self.model.display().show_dismissed;
        Self::dock_button(
            "dock-dismissed",
            if on { Icon::Show } else { Icon::Hide },
            None,
            if on {
                "Hide dismissed work · H"
            } else {
                "Show dismissed work · H"
            },
            on,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.model.toggle_dismissed();
            cx.notify();
        }))
        .into_any_element()
    }

    fn dock_zoom(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let z = self.model.graph.camera.zoom;
        vec![
            Self::dock_button("zoom-out", Icon::ZoomOut, None, "Zoom out · -", false)
                .on_click(cx.listener(|this, _, _, cx| this.zoom_graph(1. / ZOOM_STEP, cx)))
                .into_any_element(),
            Self::dock_button(
                "zoom-fit",
                Icon::Fit,
                Some(format!("{:.0}%", z * 100.)),
                "Zoom to fit · 0",
                false,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.model.graph_fit();
                cx.notify();
            }))
            .into_any_element(),
            Self::dock_button("zoom-in", Icon::ZoomIn, None, "Zoom in · +", false)
                .on_click(cx.listener(|this, _, _, cx| this.zoom_graph(ZOOM_STEP, cx)))
                .into_any_element(),
        ]
    }

    fn grouping_menu(&self, cx: &mut Context<Self>) -> Div {
        let current = self.model.display().grouping;
        // A board needs columns: as in Linear, it is never ungrouped.
        let board = self.model.view() == View::Tasks && self.model.layout() == Layout::Board;
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
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.model.set_grouping(g);
                        this.close_menu(window, cx);
                    })),
            );
        }
        panel
    }

    /// How the list is shown, set in one visit, so no choice closes it: the tasks' list or board
    /// first, then a line, then options that each turn on or off: empty groups, dismissed work.
    fn view_menu(&self, cx: &mut Context<Self>) -> Div {
        let d = self.model.display();
        let mut panel = menu_panel(232.).debug_selector(|| "view-menu".into());
        if self.model.view() == View::Tasks {
            for (id, glyph, label, layout) in [
                ("view-layout-list", Icon::List, "List", Layout::List),
                ("view-layout-board", Icon::Board, "Board", Layout::Board),
            ] {
                let on = self.model.layout() == layout;
                panel = panel.child(
                    option_row(id.into())
                        .debug_selector(move || id.into())
                        .child(icon(glyph, 14., if on { TEXT } else { TEXT_3 }))
                        .child(label)
                        // B goes to the other one: its key is shown there.
                        .child(
                            div()
                                .ml_auto()
                                .flex()
                                .items_center()
                                .when(!on, |k| k.child(kbd("B")))
                                .child(check(on)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.model.set_layout(layout);
                            cx.notify();
                        })),
                );
            }
            panel = panel.child(menu_divider().debug_selector(|| "view-menu-divider".into()));
        }
        panel
            .child(view_option(
                "view-show-empty",
                "Show empty groups",
                Some(keys::EMPTY_GROUPS),
                d.show_empty,
                Model::toggle_empty,
                cx,
            ))
            .child(view_option(
                "view-show-dismissed",
                "Show dismissed work",
                Some(keys::DISMISSED),
                d.show_dismissed,
                Model::toggle_dismissed,
                cx,
            ))
            .when(self.model.view() == View::Tasks, |menu| {
                menu.child(view_option(
                    "view-nest",
                    "Nest sub-tasks",
                    None,
                    d.nest,
                    Model::toggle_nest,
                    cx,
                ))
            })
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
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.model.set_ordering(o);
                        this.close_menu(window, cx);
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
            // Alone, the mark fills the rows' leading slot exactly, as non-tasks' kind icons do.
            .when(!name, |b| b.w(px(LEAD)).justify_center())
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

    /// A node's state mark, with its `name` in the panel, that opens the states of its kind, as
    /// Linear's status menu: the moves it can make enabled, the human's own decisions in the
    /// accent, the rest greyed so the whole flow shows. A press here neither selects the row nor
    /// starts a drag.
    fn state_button(
        &self,
        menu: Menu,
        id: Uuid,
        kind: Kind,
        state: State,
        name: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        if !has_flow(kind) {
            return div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .child(state_mark(kind, state))
                .when(name, |d| d.child(state_name(state)));
        }
        let open = self.menu == Some(menu);
        let selector = match menu {
            Menu::DetailState => "detail-state".to_owned(),
            _ => format!("state-of-{id}"),
        };
        let button = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .h(px(22.))
            .flex()
            .flex_none()
            .items_center()
            .rounded_lg()
            .when(name, |b| b.px(px(4.)).gap_2())
            .when(!name, |b| b.w(px(22.)).justify_center())
            .when(open, |b| b.bg(color(SELECTED)))
            .when(!open, |b| b.hover(|h| h.bg(color(HOVER))))
            .child(state_mark(kind, state))
            .when(name, |b| b.child(state_name(state)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_menu(menu, window, cx);
                }),
            );
        div().relative().flex_none().child(button).when(open, |d| {
            let mut panel = menu_panel(220.).debug_selector(|| "set-state-menu".into());
            let all = steps_with(kind, state, self.model.gates());
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
            .child(flow_mark(kind, s))
            .child(label)
            .child(check(s.current && !standing));
        match to {
            Some(choice) => row
                .text_color(color(if decides { ACCENT_HOVER } else { TEXT_2 }))
                .hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.model.choose_on(id, &choice);
                    this.close_menu(window, cx);
                })),
            // Where it is now: highlighted as well as checked.
            None if s.current => row
                .bg(color(SELECTED))
                .text_color(color(TEXT))
                .font_weight(FontWeight::MEDIUM),
            // Not a move from here: shown, so the flow reads whole, but greyed.
            None => row.text_color(color(TEXT_4)).opacity(0.55),
        }
    }

    /// The node's states as a flow (see `state_flow`): a click makes the move, and ratifying a
    /// lesson opens the standing-rule menu first.
    fn state_flow(
        &self,
        id: Uuid,
        kind: Kind,
        state: State,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let steps = steps_with(kind, state, self.model.gates());
        let arrows = arrows_with(kind, state, self.model.gates());
        let standing = (self.menu == Some(Menu::Standing))
            .then(|| steps.iter().find(|s| s.standing.is_some()))
            .flatten()
            .map(|s| {
                let panel = self.standing_menu(id, s, cx);
                self.popover(Menu::Standing, false, 30., panel, cx)
                    .into_any_element()
            });
        let this = cx.entity().downgrade();
        let choose = move |choice: &Choice, _: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |app, cx| {
                app.model.choose_on(id, choice);
                cx.notify();
            });
        };
        let this = cx.entity().downgrade();
        let standing_open = move |window: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |app, cx| app.toggle_menu(Menu::Standing, window, cx));
        };
        state_flow::StateFlow::new(id, kind, steps, arrows, width, self.flow.clone(), choose)
            .on_standing(standing_open)
            .standing_menu(standing)
            .into_any_element()
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
            .group("add-link")
            .h(px(24.))
            .px_2()
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_lg()
            .text_xs()
            .text_color(color(if open { TEXT } else { TEXT_3 }))
            .when(open, |b| b.bg(color(SELECTED)))
            .when(!open, |b| {
                b.hover(|h| h.bg(color(HOVER)).text_color(color(TEXT)))
            })
            .child(if open {
                icon(Icon::Link, 14., TEXT)
            } else {
                icon_hover(Icon::Link, 14., TEXT_3, "add-link", TEXT)
            })
            .child("Add link")
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
            let panel = panel.child(menu_divider()).child(
                option_row("link-choice-doc".into())
                    .debug_selector(|| "link-choice-doc".into())
                    .child(icon(Icon::Docs, 14., TEXT_3))
                    .child("A doc…")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_menu(window, cx);
                        this.help = false;
                        this.model.open_doc_picker();
                        if this.model.palette.is_some() {
                            this.reset_palette_input(window, cx);
                        }
                        cx.notify();
                    })),
            );
            d.child(self.popover(Menu::AddLink, true, 28., panel, cx))
        })
    }

    /// One group of the open node's links, each with an × that removes it.
    fn link_group(&self, g: &LinkGroup, cx: &mut Context<Self>) -> Div {
        let mut out = div().flex().flex_col().gap_1().child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(color(TEXT_4))
                .child(g.label),
        );
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
                            .group("unlink")
                            .size(px(20.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .hover(|s| s.bg(color(HOVER)))
                            .child(icon_hover(Icon::Unlink, 14., TEXT_4, "unlink", TEXT))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Remove the link")
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.model.unlink(rel, outgoing, &key);
                                cx.notify();
                            })),
                    ),
            );
        }
        out
    }

    /// The node's docs, as a link group: each by its title (its path struck when it is gone), a
    /// click opening it in the Docs view, its × taking the link away.
    fn doc_group(
        &self,
        key: &str,
        docs: &[(String, Option<String>)],
        cx: &mut Context<Self>,
    ) -> Div {
        let mut out = div()
            .debug_selector(|| "detail-docs".into())
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(TEXT_4))
                    .child("Docs"),
            );
        for (path, title) in docs {
            let gone = title.is_none();
            let (open, unlink_path, node) = (path.clone(), path.clone(), key.to_owned());
            let unlink = format!("unlink-doc-{path}");
            let row = format!("detail-doc-{path}");
            out = out.child(
                div()
                    .id(SharedString::from(row.clone()))
                    .debug_selector(move || row.clone())
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
                    .when(!gone, |d| d.hover(|h| h.bg(color(HOVER))))
                    .child(icon(Icon::Docs, 13., TEXT_4))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(color(if gone { TEXT_4 } else { TEXT_2 }))
                            .when(gone, |d| d.line_through())
                            .child(title.clone().unwrap_or_else(|| path.clone())),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(160.))
                            .truncate()
                            .text_xs()
                            .text_color(color(TEXT_4))
                            .child(if gone {
                                "missing".to_owned()
                            } else {
                                path.clone()
                            }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(unlink.clone()))
                            .debug_selector(move || unlink.clone())
                            .group("unlink")
                            .size(px(20.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .hover(|s| s.bg(color(HOVER)))
                            .child(icon_hover(Icon::Unlink, 14., TEXT_4, "unlink", TEXT))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Remove the link")
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.model.link_doc(&node, &unlink_path, false);
                                cx.notify();
                            })),
                    )
                    .when(!gone, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            this.model.open_doc(&open);
                            cx.notify();
                        }))
                    }),
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
                Icon::OpenProject,
                "No livtk projects yet",
                "Run `livtk init` in a project, then open it here, or start `livtk-app DIR`.",
            )
            .child(empty_action(
                "empty-open-project",
                "Open a project",
                cx.listener(|this, _, _, cx| this.open_folder(cx)),
            ))
            .into_any_element();
        }
        if self.model.placeholder() {
            return self.placeholder(cx);
        }
        match (self.model.view(), self.model.layout()) {
            (View::Inbox, _) => self.inbox(cx).into_any_element(),
            (View::Activity, _) => self.activity_view(cx),
            (View::Graph, _) => self.graph_view(cx),
            (View::Docs, _) => self.docs_view(window, cx),
            // Drawn in the list's place, outside its keys: `terminal_view`.
            (View::Terminal, _) => div().into_any_element(),
            // No column to draw: nothing matches, or every column is empty and hidden.
            (View::Tasks, Layout::Board) if self.model.columns().is_empty() => empty(
                view_icon(View::Tasks),
                "No tasks here",
                &format!(
                    "Clear the filter, press {} for every column, or N for a new task.",
                    keys::EMPTY_GROUPS
                ),
            )
            .debug_selector(|| "board-empty".into())
            .children(self.empty_list_action(cx))
            .into_any_element(),
            (View::Tasks, Layout::Board) => self.board(cx).into_any_element(),
            _ => self.list(window, cx).into_any_element(),
        }
    }

    // ---- rendering: the graph -------------------------------------------------------------------

    fn zoom_graph(&mut self, factor: f32, cx: &mut Context<Self>) {
        if self.model.view() == View::Graph {
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
                    Icon::Graph,
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
        // The cursor stays as it is, except while dragging the graph around.
        let dragging = matches!(self.graph_press, Some((_, true)));
        div()
            .id("graph")
            .debug_selector(|| "graph".into())
            .size_full()
            .relative()
            .overflow_hidden()
            .when(dragging, |d| d.cursor(CursorStyle::ClosedHand))
            .child(edges)
            .children(unlinked)
            .children(cards)
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
                    // A release outside the graph ends a drag without a click. Nothing follows
                    // the pointer otherwise: cards have no hover look.
                    _ => this.graph_press = None,
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
            // A trackpad's pinch zooms at its centre, as Cmd+scroll (Ctrl+scroll) does.
            .on_pinch(cx.listener(|this, e: &PinchEvent, _, cx| {
                let (x, y) = this.graph_point(e.position);
                this.model.graph_zoom(1. + e.delta, Some((x, y)));
                cx.notify();
            }))
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
    /// The Activity view: the grid above, the days picked in it, then the feed.
    fn activity_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let chip = self.grid_pick_chip(cx);
        let feed = self.feed(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .children(chip)
            .child(div().flex_1().min_h_0().child(feed))
            .into_any_element()
    }

    fn feed(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.model.activity.is_empty() {
            return div()
                .size_full()
                .child(empty(
                    Icon::Activity,
                    if self.model.grid.pick.span.is_some() {
                        "Nothing on these days"
                    } else {
                        "No activity yet"
                    },
                    if self.model.grid.pick.span.is_some() {
                        "Pick another day in the grid, or clear the pick above."
                    } else {
                        "What Claude and you do in this project shows up here, newest first."
                    },
                ))
                .into_any_element();
        }
        let rows = self.model.feed();
        let more = self.model.activity_more;
        // The lines, "Show older" while pages wait, then the dock's room at the end.
        let count = rows.len() + usize::from(more) + DOCK_ROWS;
        let pane = self.model.focused();
        let list = uniform_list(
            "feed",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                // Laid out later than drawn: as its own pane, whichever is focused.
                let was = this.model.act_on(pane);
                let out = range
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
                        None if !(more && i == rows.len()) => dock_room(),
                        None => div()
                            .id("show-older")
                            .debug_selector(|| "show-older".into())
                            .w_full()
                            .h(px(ROW))
                            .group("show-older")
                            .px_4()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(color(TEXT_4))
                            .hover(|s| s.text_color(color(TEXT_2)))
                            .child(icon_hover(
                                Icon::CaretDown,
                                10.,
                                TEXT_4,
                                "show-older",
                                TEXT_2,
                            ))
                            .child("Show older")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.model.activity_older();
                                cx.notify();
                            }))
                            .into_any_element(),
                    })
                    .collect();
                this.model.act_on(was);
                out
            }),
        )
        .track_scroll(&self.list_scroll())
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
        let (own, moving) = self.highlight(&format!("event-{index}"), on);
        let line = div()
            .id(SharedString::from(format!("event-{index}")))
            .debug_selector(move || format!("event-{index}"))
            .relative()
            .w_full()
            .h(px(ROW))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .when(own, |d| d.bg(color(SELECTED)))
            .when(!on, |d| d.hover(|s| s.bg(color(HOVER))))
            .children(moving.map(|(from, to, epoch)| glide_highlight(from, to, epoch)))
            .child(
                div()
                    .size(px(6.))
                    .flex_none()
                    .rounded_full()
                    .when(new, |d| d.bg(color(ACCENT))),
            )
            .child(avatar(human))
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
        if self.model.rows().is_empty() && self.model.leaving().is_empty() {
            return div()
                .size_full()
                .child(empty(
                    Icon::Inbox,
                    "Nothing waits for you",
                    "Plans to acknowledge, lessons to ratify and decisions to accept appear here.",
                ))
                .into_any_element();
        }
        let mut shown: Vec<(Summary, Option<Instant>)> = self
            .model
            .rows()
            .iter()
            .map(|r| (r.clone(), None))
            .collect();
        for (r, at, when) in self.model.leaving() {
            let i = (*at).min(shown.len());
            shown.insert(i, (r.clone(), Some(*when)));
        }
        let selected = self.model.selected().map(|r| r.id);
        let mut list = div()
            .id("inbox")
            .size_full()
            .overflow_y_scroll()
            .pt_1()
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
        // Room at the end of what scrolls, not round it: rows pass under the dock, and the
        // last one can rise clear of it.
        list.child(div().h(px(DOCK_SPACE)).flex_none())
            .into_any_element()
    }

    /// An empty list's way on: the filter cleared, when one hides what is there; else a new
    /// task, where tasks are.
    fn empty_list_action(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if !self.model.display().filter.is_empty() {
            return Some(empty_action(
                "empty-clear-filter",
                "Clear the filter",
                cx.listener(|this, _, window, cx| {
                    this.model.set_filter("");
                    this.sync_filter(window, cx);
                    cx.notify();
                }),
            ));
        }
        (self.model.view() == View::Tasks).then(|| {
            empty_action(
                "empty-new-task",
                "New task",
                cx.listener(|this, _, window, cx| this.open_form(window, cx)),
            )
        })
    }

    /// Tasks, lessons, decisions or findings: a virtualised list in solid group bands.
    fn list(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.model.rows().is_empty() && self.model.groups().is_empty() {
            let what = self.model.view().title().to_lowercase();
            return div()
                .size_full()
                .child(
                    empty(
                        view_icon(self.model.view()),
                        &format!("No {what} here"),
                        "Clear the filter, press S D to show dismissed work, or N for a new task.",
                    )
                    .children(self.empty_list_action(cx)),
                )
                .into_any_element();
        }
        // The rows, then the dock's room at the end of what scrolls.
        let count = self.items().len() + DOCK_ROWS;
        let pane = self.model.focused();
        uniform_list(
            "rows",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                // Laid out later than drawn: its own pane's rows, whichever is focused.
                let was = this.model.act_on(pane);
                let items = this.items();
                let selected = this.model.selected().map(|r| r.id);
                let mut out = Vec::with_capacity(range.len());
                for i in range {
                    match items.get(i) {
                        Some(Item::Band(gi)) => out.push(this.band(*gi, cx)),
                        Some(Item::Row(ri)) => {
                            let r = this.model.rows()[*ri].clone();
                            let sel = Some(r.id) == selected;
                            out.push(this.row(&r, sel, None, false, cx));
                        }
                        None => out.push(dock_room()),
                    }
                }
                this.model.act_on(was);
                out
            }),
        )
        .track_scroll(&self.list_scroll())
        .size_full()
        .into_any_element()
    }

    /// A group's solid header band: its mark, name and count. Click to collapse; drop a row on
    /// it to move the row into the group.
    fn band(&self, gi: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(g) = self.model.groups().get(gi).cloned() else {
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
            .bg(color(if self.model.band() == Some(key) {
                SELECTED
            } else {
                SURFACE
            }))
            .border_b_1()
            .border_color(color(BORDER))
            .hover(|s| s.bg(color(HOVER)))
            .child(icon(
                if g.collapsed {
                    Icon::CaretRight
                } else {
                    Icon::CaretDown
                },
                10.,
                TEXT_4,
            ))
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

    /// A parent's sub-tasks: how many are done, of how many.
    fn sub_task_count(&self, id: Uuid) -> Option<Div> {
        let &(done, total) = self.model.progress().get(&id)?;
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(color(TEXT_4))
                .child(icon(Icon::SubTask, 12., TEXT_4))
                .child(format!("{done}/{total}")),
        )
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
        let (own, moving) = self.highlight(&r.key, selected);
        let mut row = div()
            .id(SharedString::from(format!("row-{}", r.key)))
            .relative()
            .w_full()
            .h(px(ROW))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(color(CONTENT))
            .when(own, |d| d.bg(color(SELECTED)))
            .when(!selected, |d| d.hover(|s| s.bg(color(HOVER))))
            .text_color(color(if blocked { TEXT_3 } else { TEXT_2 }))
            .children(moving.map(|(from, to, epoch)| glide_highlight(from, to, epoch)));
        // Every row leads with the same slot, so keys, states and titles line up whatever the
        // kind: a task's priority, else the kind's icon.
        row = if r.kind == Kind::Task {
            row.child(self.priority_button(Menu::RowPriority(id), id, r.priority, false, cx))
        } else {
            row.child(kind_slot(r.kind))
        };
        let key = r.key.clone();
        row = row
            .child(
                div()
                    .debug_selector(move || format!("key-{key}"))
                    .w(px(62.))
                    .flex_none()
                    .text_color(color(TEXT_4))
                    .child(r.key.clone()),
            )
            .child(self.state_button(Menu::RowState(id), id, r.kind, r.state, false, cx))
            .children(self.model.depth().get(&id).map(|&d| {
                // Under its parent: indented by its depth, led by the turn.
                div().flex_none().pl(px((d - 1) as f32 * 16.)).child(icon(
                    Icon::SubTask,
                    14.,
                    TEXT_4,
                ))
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(color(if blocked { TEXT_3 } else { TEXT }))
                    .child(r.title.clone()),
            )
            .children(self.model.parent_key(r).map(parent_chip))
            .children(self.sub_task_count(id));
        if blocked {
            row = row.child(icon_tag(Icon::Blocked, "Blocked", TEXT_3));
        }
        if r.standing {
            row = row.child(icon_tag(Icon::Standing, "Standing rule", DONE));
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
            .gap(px(6.))
            .p_2();
        for (key, rows) in self.model.columns() {
            let (mark, label) = group_label(key, Kind::Task);
            let mut col = div()
                .id(SharedString::from(format!("col-{key:?}")))
                .debug_selector(move || match key {
                    GroupKey::State(s) => format!("column-{}", s.as_str()),
                    GroupKey::Priority(p) => format!("column-priority-{p}"),
                    GroupKey::All => "column-all".to_owned(),
                })
                .w(px(BOARD_COLUMN))
                .flex_none()
                .h_full()
                .flex()
                .flex_col()
                .gap(px(6.))
                .p(px(6.))
                .rounded_xl()
                .bg(color(SURFACE))
                .border_1()
                .border_color(color(BORDER))
                .overflow_y_scroll()
                .restrict_scroll_to_axis()
                .track_scroll(&self.column_scroll(key))
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
                        .h(px(24.))
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
                    // Cards keep their height, so a full column overflows and scrolls. Two rows:
                    // its icons (with their menus) leading its key, then its title.
                    .flex_none()
                    .p_2()
                    .rounded_lg()
                    .bg(color(if sel { SELECTED } else { CONTENT }))
                    .border_1()
                    .border_color(color(if sel { BORDER_STRONG } else { BORDER }))
                    .hover(|s| s.border_color(color(BORDER_STRONG)))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(color(TEXT_4))
                            .child(self.priority_button(
                                Menu::RowPriority(id),
                                id,
                                r.priority,
                                false,
                                cx,
                            ))
                            .child(self.state_button(
                                Menu::RowState(id),
                                id,
                                Kind::Task,
                                r.state,
                                false,
                                cx,
                            ))
                            .child(div().pl_1().child(r.key.clone()))
                            .children(self.model.parent_of(r).map(parent_chip))
                            .child(div().flex_1())
                            .children(self.sub_task_count(id))
                            .when(blocked, |d| {
                                d.child(icon(Icon::Blocked, 12., TEXT_4)).child("Blocked")
                            }),
                    )
                    .child(
                        div()
                            .px_1()
                            .line_clamp(2)
                            .text_color(color(if blocked { TEXT_3 } else { TEXT }))
                            .child(r.title.clone()),
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
            // Full height behind the floating dock: cards pass under it, and the room at the
            // end lets the last one rise clear of it.
            cols = cols.child(col.child(div().h(px(DOCK_SPACE)).flex_none()));
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

    /// The panel shows another node (or none): put the note field's draft by the node it was
    /// written for, and bring back the new one's own.
    fn track_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = self
            .model
            .detail
            .as_ref()
            .zip(self.model.project())
            .map(|(d, p)| (p.dir.clone(), d.node.id));
        if open == self.note_for {
            return;
        }
        let draft = self.note.read(cx).value().to_string();
        if let Some(was) = self.note_for.take() {
            if draft.is_empty() {
                self.note_drafts.remove(&was);
            } else {
                self.note_drafts.insert(was, draft);
            }
        }
        let back = open
            .as_ref()
            .and_then(|key| self.note_drafts.remove(key))
            .unwrap_or_default();
        self.note.update(cx, |s, cx| s.set_value(back, window, cx));
        self.note_for = open;
    }

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
    fn detail(&mut self, width: f32, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (d, closing) = match (&self.model.detail, &self.detail_leaving) {
            (Some(d), _) => (d.clone(), false),
            (None, Some(d)) => (d.clone(), true),
            // A doc being read: its details, when nothing else is open in the panel.
            (None, None) if self.doc_panel_open() => {
                return Some(
                    div()
                        .debug_selector(|| "detail-panel".into())
                        .flex_none()
                        .h_full()
                        .overflow_hidden()
                        .child(self.doc_panel(width, cx))
                        .into_any_element(),
                );
            }
            (None, None) => return None,
        };
        let panel = self.detail_panel(&d, width, cx);
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
                        el.w(px(width * open)).opacity(0.3 + 0.7 * open)
                    },
                )
                .into_any_element(),
        )
    }

    fn detail_panel(&mut self, d: &Detail, width: f32, cx: &mut Context<Self>) -> Stateful<Div> {
        let n = &d.node;
        let mut props = div().flex().flex_col().gap_2().child(prop_row(
            "Status",
            self.state_button(Menu::DetailState, n.id, n.kind, n.state, true, cx),
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
            props = props.child(prop_row(
                "Scope",
                icon_tag(Icon::Standing, "Standing rule", DONE),
            ));
        }
        props = props.child(prop_row(
            "Created",
            div().child(format!("{} by {}", ago(&n.created), who(&n.created_by))),
        ));
        let mut links = div()
            .debug_selector(|| "detail-links".into())
            .flex()
            .flex_col()
            .gap_3()
            .child(
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
        if !d.docs.is_empty() {
            links = links.child(self.doc_group(&n.key, &d.docs, cx));
        }
        // The panel's content: its width less its padding either side.
        let flow = self.state_flow(n.id, n.kind, n.state, width - 40., cx);

        let title = self.detail_title(n, cx);
        let body = self.detail_body(n, cx);
        let mut timeline = div().flex().flex_col().gap_3();
        for e in d.history.iter().rev() {
            timeline = timeline.child(event_line(e, &n.key));
        }
        div()
            .id(SharedString::from(format!("detail-{}", n.key)))
            .debug_selector(|| "detail-body-panel".into())
            .w(px(width))
            .flex_none()
            .h_full()
            // The window's black shows through, as behind the sidebar.
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
                            .group("close-detail")
                            .size(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .hover(|s| s.bg(color(HOVER)))
                            .child(icon_hover(
                                Icon::DockRight,
                                16.,
                                TEXT_4,
                                "close-detail",
                                TEXT_2,
                            ))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Close · Esc")
                                    .build(window, cx)
                            })
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
                    // What it waits for and what it blocks come before what it says.
                    .child(links)
                    .child(body)
                    .child(section_label("Activity"))
                    .child(timeline),
            )
            .child(
                div()
                    .flex_none()
                    .p_3()
                    .border_t_1()
                    .border_color(color(BORDER))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_2()
                            .child(div().pt(px(7.)).child(icon(Icon::Notes, 14., TEXT_4)))
                            .child(indented(
                                div().flex_1().child(Textarea::new(&self.note).xsmall()),
                                &self.note,
                            )),
                    ),
            )
    }

    // ---- rendering: overlays ------------------------------------------------------------------

    /// A modal: a dim backdrop that closes it when clicked, around a card sized to its content.
    /// `keys`: the modal itself has the keys (the shortcuts, About), rather than a field in it.
    /// `fades`: it fades in (the palette shows at once).
    fn modal(
        &self,
        id: &'static str,
        keys: bool,
        fades: bool,
        card: Div,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .absolute()
            .inset_0()
            // Nothing behind it is hovered, scrolled or clicked while it is open.
            .occlude()
            .key_context(FORM)
            // So no key reaches the list, or a form, behind it.
            .when(keys, |d| d.track_focus(&self.modal_focus))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.escape(window, cx)))
            .on_action(cx.listener(|this, _: &input::Escape, window, cx| this.escape(window, cx)))
            .on_action(cx.listener(|this, _: &Submit, window, cx| this.submit_form(window, cx)))
            .bg(with_alpha(0x000000, 0.6))
            .flex()
            .flex_col()
            .items_center()
            // On a short window the gap above gives way first, then the card, whose content
            // scrolls inside it: a modal never runs past the window's bottom edge.
            .pb(px(16.))
            // A click outside closes it, but not the click that brings the window back from
            // another app: that one only activates it, so nothing half typed is lost.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    if !e.first_mouse {
                        this.close_top(window, cx);
                    }
                }),
            )
            .child(div().h(px(110.)).min_h(px(16.)).flex_shrink(20.))
            .child(
                card.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .min_h_0()
                    .flex_shrink(1.)
                    .overflow_hidden()
                    .rounded_xl()
                    .bg(color(SURFACE))
                    .border_1()
                    .border_color(color(BORDER_STRONG))
                    .shadow_lg(),
            )
            .with_animation(
                SharedString::from(format!("{id}-in")),
                Animation::new(Duration::from_millis(if fades { 140 } else { 0 }))
                    .with_easing(ease_out_quint()),
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
            .hover(|s| s.bg(color(ACCENT_HOVER)))
            .child("Create task")
            .child(
                div()
                    .text_xs()
                    .text_color(with_alpha(TEXT, 0.7))
                    .child(keys::SUBMIT),
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
            .child(indented(
                div()
                    .debug_selector(|| "new-task-body".into())
                    .mx(-TEXTAREA_PX)
                    .child(
                        Textarea::new(&form.body)
                            .appearance(false)
                            .xsmall()
                            .text_sm(),
                    ),
                &form.body,
            ))
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
        Some(self.modal("new-task-modal", false, true, card, cx))
    }

    /// The command palette: a field, what it finds in sections, and the keys that drive it.
    /// The palette, shown at once and gone at once.
    fn palette_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.model.palette.clone()?;
        let card = self.palette_card(&p, cx);
        Some(self.modal("palette-modal", false, false, card, cx))
    }

    /// The palette's card for `p`.
    fn palette_card(&mut self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let mut results = div()
            .id("palette-results")
            .track_scroll(&self.palette_scroll)
            .max_h(px(400.))
            // On a short window the results give way and scroll, so the card fits.
            .min_h_0()
            .flex_shrink(1.)
            .overflow_y_scroll()
            .p_1()
            .flex()
            .flex_col();
        let mut section = None;
        for (i, item) in p.items.iter().enumerate() {
            let s = if p.link.is_some() || p.doc_pick.is_some() {
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
                        .font_weight(FontWeight::MEDIUM)
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
        let context = match (&p.link, &p.doc_pick) {
            (Some(pick), _) => Some(format!("{} · {}", pick.key, pick.choice.label)),
            (_, Some(DocPick::ForNode(key))) => Some(format!("{key} · Doc")),
            (_, Some(DocPick::ForDoc(path))) => Some(path.clone()),
            _ => self.model.acted_on().map(|r| r.key),
        };
        div()
            .debug_selector(|| "palette".into())
            .key_context(PALETTE)
            .w(px(PALETTE_W))
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
                    .child(icon(Icon::Search, 18., TEXT_4))
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
                                .h(px(20.))
                                .px_2()
                                .flex()
                                .items_center()
                                .rounded_full()
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
                    .child(kbd_icon(Icon::Enter))
                    .child("to run")
                    .child(div().w(px(8.)))
                    .child(kbd("Esc"))
                    .child("to close"),
            )
    }

    /// An action's icon in the palette, so every row has one: a move shows the state it goes
    /// to, a priority its mark, the rest the icon of where that action lives in the app.
    fn action_icon(&self, run: &Run) -> Div {
        let slot = div()
            .size(px(16.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center();
        let glyph = match run {
            Run::Choose(c) => {
                let kind = self.model.acted_on().map_or(Kind::Task, |r| r.kind);
                return slot.child(state_mark(kind, c.to));
            }
            Run::Priority(p) => return slot.child(priority_mark(*p)),
            Run::Go(view) => view_icon(*view),
            Run::Project(_) => Icon::Project,
            Run::ToggleLayout if self.model.layout() == Layout::List => Icon::Board,
            Run::ToggleLayout => Icon::List,
            Run::Group(_) => Icon::Grouping,
            Run::Order(_) => Icon::Ordering,
            Run::ToggleDismissed if self.model.display().show_dismissed => Icon::Hide,
            Run::ToggleDismissed => Icon::Show,
            Run::ToggleEmpty => Icon::ViewOptions,
            Run::FitGraph => Icon::Fit,
            Run::PickLink(_) | Run::Ask(Ask::PickLink) => Icon::Link,
            Run::Ask(Ask::NewTask) => Icon::NewTask,
            Run::Ask(Ask::AddNote) => Icon::Notes,
            Run::Ask(Ask::EditTitle | Ask::EditBody) => Icon::Edit,
            Run::Ask(Ask::OpenFolder) => Icon::OpenProject,
            Run::Ask(Ask::Help) => Icon::Shortcuts,
            Run::Ask(Ask::SplitRight) => Icon::PanelRight,
            Run::Ask(Ask::SplitDown) => Icon::PanelBottom,
            Run::Ask(Ask::ClosePane) => Icon::Close,
            Run::Ask(Ask::NextPane) => Icon::ArrowRight,
            Run::Ask(Ask::NewTerminal) => Icon::Terminal,
        };
        slot.child(icon(glyph, 16., TEXT_3))
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
                row.child(self.action_icon(&a.run))
                    .child(div().truncate().child(a.label.clone()))
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
            // Found by its text: the line it is in; else where it is.
            PaletteItem::Doc(d) => row
                .child(icon(Icon::Docs, 16., TEXT_3))
                .child(
                    div()
                        .flex_none()
                        .max_w(px(280.))
                        .truncate()
                        .child(d.title.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(color(TEXT_4))
                        .child(d.line.clone().unwrap_or_else(|| d.path.clone())),
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
        let shown = SHORTCUTS
            .iter()
            .filter(|(_, _, what)| cfg!(target_os = "macos") || *what != CLOSE_WINDOW);
        for (section, keys, what) in shown {
            if *section != current {
                if !current.is_empty() {
                    sections = sections.child(block);
                }
                block = div().flex().flex_col().child(
                    div()
                        .pb_1()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
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
            .debug_selector(|| "shortcuts-card".into())
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
                    .debug_selector(|| "shortcuts-scroll".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .child(sections),
            );
        Some(self.modal("shortcuts-modal", true, true, card, cx))
    }

    /// Make the menu bar and the Dock menu again when what they show changed: the projects, the
    /// view and its options, what waits in the inbox.
    fn sync_menus(&mut self, cx: &mut Context<Self>) {
        let m = &self.model;
        let d = m.display();
        let state = menus::MenuState {
            projects: m.projects.iter().map(|p| p.name.clone()).collect(),
            current: m.current,
            view: View::ALL.iter().position(|v| *v == m.view()).unwrap_or(0),
            board: m.layout() == Layout::Board,
            show_empty: d.show_empty,
            show_dismissed: d.show_dismissed,
            inbox: m.projects.iter().map(|p| p.inbox).sum(),
            update: self.update.as_ref().map(|s| s.manifest.build),
        };
        if self.menu_state.as_ref() != Some(&state) {
            cx.set_menus(menus::menu_bar(&state));
            cx.set_dock_menu(menus::dock_menu(&state));
            self.menu_state = Some(state);
        }
    }

    /// Switch to project `i`, as the File and Dock menus' Projects do.
    fn switch_project(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.model.select_project(i);
        self.sync_filter(window, cx);
        cx.notify();
    }

    /// Add Link…, from the Task menu: the palette, showing the links the node can gain.
    fn add_link_from_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.palette.is_none() {
            self.toggle_palette(window, cx);
        }
        self.palette_input
            .update(cx, |s, cx| s.set_value("Add link", window, cx));
        self.model.palette_query("Add link");
        cx.notify();
    }

    /// Install Command Line Tool, from the livtk menu: link livtk and livtk-app onto PATH from
    /// the bundle this runs in, and say how it went.
    fn install_cli(&mut self) {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let bundle = std::env::current_exe()
            .ok()
            .and_then(|e| std::fs::canonicalize(e).ok())
            .and_then(|e| crate::launch::bundle_of(&e))
            .or_else(|| home.as_ref().map(|h| h.join(crate::launch::IN_HOME)))
            .filter(|b| b.exists());
        let (Some(bundle), Some(home)) = (bundle, home) else {
            self.model.warn(
                "No livtk.app to link the command line tools to: scripts/check --install puts it \
                 in Applications.",
            );
            return;
        };
        let bin = home.join(crate::launch::BIN_IN_HOME);
        let path = std::env::var_os("PATH").unwrap_or_default();
        match crate::launch::install_cli(&bundle, &bin, &path) {
            Ok(done) if !done.refused.is_empty() => self.model.warn(format!(
                "Not linked, a file is in the way: {}",
                done.refused
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Ok(done) => {
                let mut text = format!("livtk and livtk-app are linked in {}.", bin.display());
                if !done.on_path {
                    text.push_str(" That folder is not on PATH: add it in your shell's profile.");
                }
                if let Some(first) = done.shadowed.first() {
                    text.push_str(&format!(
                        " {} comes earlier on PATH and runs instead: remove it.",
                        first.display()
                    ));
                } else if let Some(first) = done.leftover.first() {
                    text.push_str(&format!(
                        " {} is left over from an earlier install: it never runs, so you can \
                         remove it.",
                        first.display()
                    ));
                }
                self.model.inform(text);
            }
            Err(e) => self
                .model
                .warn(format!("Cannot link the command line tools: {e}")),
        }
    }

    /// About livtk: its name, which build this is, what it is for, where it lives.
    /// Settings: how docs read (their text's size, how close their lines sit).
    fn settings_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.settings {
            return None;
        }
        let reading = self.model.reading;
        let choice = |id: String, label: &'static str, on: bool| {
            div()
                .id(SharedString::from(id.clone()))
                .debug_selector(move || id.clone())
                .h(px(26.))
                .px_3()
                .flex()
                .items_center()
                .rounded_md()
                .text_sm()
                .when(on, |d| d.bg(color(SELECTED)).text_color(color(TEXT)))
                .when(!on, |d| {
                    d.text_color(color(TEXT_3))
                        .hover(|h| h.text_color(color(TEXT_2)))
                })
                .child(label)
        };
        let segmented = || {
            div()
                .flex()
                .p(px(2.))
                .gap(px(2.))
                .rounded_lg()
                .bg(color(SURFACE))
                .border_1()
                .border_color(color(BORDER))
        };
        let mut sizes = segmented();
        for size in crate::model::ReadingSize::ALL {
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
        let row = |label: &'static str, control: Div| {
            div()
                .flex()
                .items_center()
                .child(div().flex_1().text_color(color(TEXT_2)).child(label))
                .child(control)
        };
        let card = div()
            .debug_selector(|| "settings".into())
            .w(px(480.))
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(color(TEXT_3))
                    .child("livtk")
                    .child("›")
                    .child("Settings"),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(TEXT_4))
                    .child("Reading docs"),
            )
            .child(row("Text size", sizes))
            .child(row("Density", density));
        Some(self.modal("settings-modal", true, true, card, cx))
    }

    fn about_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.about {
            return None;
        }
        let card = div()
            .debug_selector(|| "about".into())
            .w(px(380.))
            .p_6()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .child(icon(Icon::Inbox, 32., ACCENT))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color(TEXT))
                    .child("livtk"),
            )
            .child(
                div()
                    .debug_selector(|| "about-build".into())
                    .text_color(color(TEXT_3))
                    .child(livtk_build::DESCRIBE),
            )
            .child(
                div()
                    .pt_2()
                    .text_center()
                    .text_color(color(TEXT_2))
                    .child("Durable tasks and memory for Claude Code and its human."),
            )
            .child(div().text_xs().text_color(color(TEXT_4)).child(REPOSITORY));
        Some(self.modal("about-modal", true, true, card, cx))
    }

    /// The notice, at the bottom right above the dock's level, with its ×. The pointer over it
    /// holds it; when its time is up it fades out. Its id stays the same from one notice to the
    /// next, so a pointer over it stays over it; its animations are each notice's own, so a new
    /// one comes in afresh.
    /// The notifications, bottom right above the dock: newest first, a few shown and the rest
    /// counted.
    fn notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (shown, more) = self.model.toasts().shown();
        if shown.is_empty() {
            return None;
        }
        let mut stack = div()
            .absolute()
            // Above the dock's level, so the two never meet.
            .bottom(px(DOCK_SPACE + 8.))
            .right(px(16.))
            .w(px(TOAST_WIDTH))
            .flex()
            .flex_col_reverse()
            .gap_2();
        for (i, t) in shown.into_iter().enumerate() {
            stack = stack.child(self.toast(t, i == 0, cx));
        }
        if more > 0 {
            stack = stack.child(
                div()
                    .debug_selector(|| "toasts-more".into())
                    .self_end()
                    .px_2()
                    .text_xs()
                    .text_color(color(TEXT_4))
                    .child(format!("+{more} more")),
            );
        }
        Some(stack.into_any_element())
    }

    /// One notification, drawn as the new task modal is: a breadcrumb (its icon in its level's
    /// colour, where it comes from and what it is, how many times it came, and a close), its
    /// title and markdown text (four lines, then Show more), and its buttons in a footer under a
    /// hairline. The pointer over it holds it. The newest answers to `notice` in tests.
    fn toast(&self, t: &Toast, newest: bool, cx: &mut Context<Self>) -> AnyElement {
        let id = t.id;
        let hue = match t.level {
            Level::Info => ACCENT,
            Level::Success => REVIEW,
            Level::Warning => PROGRESS,
            Level::Error => RED,
        };
        let glyph = match t.mark {
            Mark::Bug => Icon::Bug,
            Mark::Bell => Icon::Bell,
            Mark::Inbox => Icon::Inbox,
            Mark::Meteor => Icon::Meteor,
            Mark::Message => Icon::Message,
            // The sparkles, as Claude's own mark is drawn.
            Mark::Sparkles => Icon::Claude,
        };
        let name = |what: &str| -> SharedString {
            if newest {
                format!("notice{what}").into()
            } else {
                format!("toast-{id}{what}").into()
            }
        };
        let close_name = name("-close");
        let close = div()
            .id(close_name.clone())
            .debug_selector(move || close_name.to_string())
            .group("toast-close")
            .size(px(20.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_lg()
            .hover(|s| s.bg(color(HOVER)))
            .child(icon_hover(Icon::Close, 14., TEXT_4, "toast-close", TEXT))
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new("Close").build(window, cx)
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.dismiss(id);
                cx.notify();
            }));
        let (from, what) = t.crumbs();
        let crumbs = div()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(color(TEXT_3))
            .child(icon(glyph, 14., hue))
            .child(from.to_owned())
            .child("›")
            .child(what.to_owned())
            .child(div().flex_1())
            .when(t.count > 1, |d| {
                d.child(
                    div()
                        .text_color(color(TEXT_4))
                        .child(format!("×{}", t.count)),
                )
            })
            .child(close);
        let expanded = self.expanded.contains(&id);
        let long = t.text.len() > TOAST_LONG || t.text.lines().count() > 4;
        let text = div()
            .id(SharedString::from(format!("toast-text-{id}")))
            .text_color(color(TEXT_2))
            .when(long && !expanded, |d| {
                d.max_h(px(TOAST_CLIPPED)).overflow_hidden()
            })
            .when(expanded, |d| d.max_h(px(TOAST_OPEN)).overflow_y_scroll())
            .child(TextView::markdown(
                SharedString::from(format!("toast-md-{id}")),
                t.text.clone(),
            ));
        let mut body = div()
            .flex()
            .flex_col()
            .gap_1()
            .when_some(t.title.clone(), |d, title| {
                d.child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color(TEXT))
                        .child(title),
                )
            })
            .child(text);
        if long {
            body = body.child(
                div()
                    .id(SharedString::from(format!("toast-more-{id}")))
                    .debug_selector(move || format!("toast-more-{id}"))
                    .text_xs()
                    .text_color(color(TEXT_3))
                    .hover(|s| s.text_color(color(TEXT)))
                    .child(if expanded { "Show less" } else { "Show more" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.expanded.remove(&id) {
                            this.expanded.insert(id);
                        }
                        cx.notify();
                    })),
            );
        }
        // As the modal's footer: the first button its Create task, the rest quiet.
        let footer = (!t.actions.is_empty()).then(|| {
            let mut row = div().flex().justify_end().gap_2();
            for (i, b) in t.actions.iter().enumerate() {
                let act = b.act.clone();
                let act_name = name(&format!("-act-{i}"));
                let first = i == 0;
                row = row.child(
                    div()
                        .id(act_name.clone())
                        .debug_selector(move || act_name.to_string())
                        .h(px(28.))
                        .px_3()
                        .flex()
                        .items_center()
                        .rounded_lg()
                        .font_weight(FontWeight::MEDIUM)
                        .when(first, |d| {
                            d.bg(color(ACCENT))
                                .text_color(color(TEXT))
                                .hover(|s| s.bg(color(ACCENT_HOVER)))
                        })
                        .when(!first, |d| {
                            d.border_1()
                                .border_color(color(BORDER_STRONG))
                                .text_color(color(TEXT_2))
                                .hover(|s| s.bg(color(HOVER)))
                        })
                        .child(b.label.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.model.dismiss(id);
                            this.act(&act, cx);
                        })),
                );
            }
            row
        });
        let card_name = name("");
        let card = div()
            .id(card_name.clone())
            .debug_selector(move || card_name.to_string())
            .w_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_xl()
            .bg(color(SURFACE))
            .border_1()
            .border_color(color(BORDER_STRONG))
            .shadow_lg()
            .text_color(color(TEXT))
            .text_sm()
            // Presses here are the notification's: nothing under it is clicked through it.
            .occlude()
            // Only the pointer lets it go: a key pressed with the pointer resting on it, which
            // GPUI by default takes as leaving, does not.
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.model.hold(id, *hovered, Instant::now());
                cx.notify();
            }))
            .child(crumbs)
            .child(body)
            .when_some(footer, |d, footer| {
                d.child(div().h(px(1.)).bg(color(BORDER))).child(footer)
            });
        let card = if t.leaving.is_some() {
            card.with_animation(
                SharedString::from(format!("toast-out-{id}")),
                Animation::new(NOTICE_FADE).with_easing(ease_in_out),
                |el, t| el.opacity(1.0 - t),
            )
            .into_any_element()
        } else {
            card.into_any_element()
        };
        // The coming in is drawn whether or not the card is leaving, so it plays once per
        // notification: one caught while fading comes back at full strength.
        div()
            .child(card)
            .with_animation(
                SharedString::from(format!("toast-in-{id}")),
                Animation::new(NOTICE_IN).with_easing(ease_out_quint()),
                |el, t| el.opacity(t),
            )
            .into_any_element()
    }

    /// The `[[on]]` hooks of each project the human just wrote to, off the window's thread: a
    /// hook never holds up the window, and a failing one is tried again on the next write.
    fn run_hooks(&mut self, cx: &mut Context<Self>) {
        for (dir, before) in self.model.take_written() {
            let ran = cx.background_spawn(async move {
                livtk_core::Workspace::open(&dir).and_then(|mut ws| ws.dispatch_from(before))
            });
            cx.spawn(async move |_, _| match ran.await {
                Ok(done) => {
                    for f in done.failed {
                        log::warn!("hook failed, to be tried again: {f}");
                    }
                }
                Err(e) => log::warn!("hooks: {e}"),
            })
            .detach();
        }
    }

    /// A notification's button pressed.
    fn act(&mut self, act: &Act, cx: &mut Context<Self>) {
        match act {
            Act::Open(arrival) => self.model.open_arrival(arrival),
            Act::RestartToUpdate => self.restart_to_update(cx),
        }
        cx.notify();
    }
}

impl Render for LivtkApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if Self::window_width(window) >= RAIL_BELOW {
            self.sidebar_peek = false;
        }
        let sidebar = self.sidebar(window, cx);
        let rail = self.rail(window);
        // Collapsing or expanding the sidebar slides it; a drag of its edge does not.
        let side_w = self.sidebar_px(window);
        let from = match self.rail_was {
            Some((was, w)) if was != rail => {
                self.rail_epoch += 1;
                Some(w)
            }
            _ => None,
        };
        if from.is_some() {
            self.rail_from = from;
        }
        self.rail_was = Some((rail, side_w));
        let from_w = self.rail_from.unwrap_or(side_w);
        let epoch = self.rail_epoch;
        let sidebar = div()
            .flex_none()
            .h_full()
            .overflow_hidden()
            .child(sidebar)
            .when(epoch == 0, |d| d.w(px(side_w)))
            .with_animation(
                SharedString::from(format!("sidebar-{epoch}")),
                Animation::new(if epoch == 0 { Duration::ZERO } else { SLIDE })
                    .with_easing(ease_out_cubic),
                move |el, t| {
                    if epoch == 0 {
                        el
                    } else {
                        el.w(px(from_w + (side_w - from_w) * t))
                            .opacity(0.4 + 0.6 * t)
                    }
                },
            );
        let top_bar = self.top_bar(window, cx);
        let bottom = (self.bottom_open || self.bottom_leaving).then(|| self.bottom_bar(window, cx));
        self.focus_on_arrival(window, cx);
        let dock = self.dock(window, cx);
        let content = self.content(window, cx);
        self.track_detail(window, cx);
        self.track_note(window, cx);
        let panel_w = self.panel_px(window);
        let floats = self.panel_floats(window);
        // A narrow window has no room for the panel beside the content: it floats over it.
        // The panel's edge, while it is open (not while it slides away): in the gutter right of
        // the content box, or across the border of a panel floating over it.
        let mut panel_edge = self
            .panel_open()
            .then(|| self.edge(Edge::Panel, window, cx));
        let float_edge = if floats { panel_edge.take() } else { None };
        let detail = self.detail(panel_w, cx).map(|d| {
            if floats {
                div()
                    .debug_selector(|| "panel-float".into())
                    // Presses here are the panel's: none reaches the titlebar strip or the
                    // content under it.
                    .occlude()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .flex()
                    .bg(color(WINDOW))
                    .border_l_1()
                    .border_color(color(BORDER_STRONG))
                    .shadow_lg()
                    .child(d)
                    // Centred on the 1 px border, which positions count from inside of.
                    .children(float_edge.map(|e| e.left(px(-EDGE_W / 2. - 0.5))))
                    .into_any_element()
            } else {
                d
            }
        });
        let dragging_edge = self.resizing.is_some();
        // While an edge is dragged, follow the pointer anywhere in the window, ahead of what is
        // under it, and let go when the button is.
        let drag = self.resizing.map(|edge| {
            let this = cx.entity().downgrade();
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let moved = this.clone();
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                        if phase != DispatchPhase::Capture {
                            return;
                        }
                        let size = window.viewport_size();
                        let held = e.pressed_button == Some(MouseButton::Left);
                        let _ = moved.update(cx, |app, cx| {
                            if held {
                                app.drag_edge(edge, e.position, size);
                                let at = app.along(edge, e.position, window);
                                app.hover_edge(edge, at);
                            } else {
                                app.resizing = None;
                            }
                            cx.notify();
                        });
                    });
                    let released = this.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                        if phase != DispatchPhase::Capture {
                            return;
                        }
                        let _ = released.update(cx, |app, cx| {
                            // Let go away from the edge, its glow goes; over it, it stays.
                            if app.edge_inside != app.resizing {
                                app.let_go_edge(cx);
                            }
                            app.resizing = None;
                            cx.notify();
                        });
                    });
                },
            )
            .absolute()
            .size_full()
        });
        self.sync_menus(cx);
        let form = self.form_overlay(cx);
        let help = self.help_overlay(cx);
        let about = self.about_overlay(cx);
        let settings = self.settings_overlay(cx);
        let palette = self.palette_overlay(cx);
        let notice = self.notice(cx);
        let list_area = div()
            .flex_1()
            .min_h_0()
            .key_context(NAV)
            .track_focus(&self.nav)
            .on_action(cx.listener(|this, _: &Down, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &Up, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Open, window, cx| {
                // A doc a search found opens with the find bar on what was searched.
                let docs = &this.model.docs;
                let searched = this.model.view() == View::Docs
                    && !docs.reading
                    && !docs.query.trim().is_empty();
                if searched
                    && let Some(DocRow::Doc { path, .. }) = docs.rows().get(docs.index).cloned()
                {
                    this.open_found(&path, window, cx);
                    return;
                }
                this.model.enter();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PrevColumn, _, cx| {
                this.model.select_column_by(-1);
                this.after_move(cx);
            }))
            .on_action(cx.listener(|this, _: &NextColumn, _, cx| {
                this.model.select_column_by(1);
                this.after_move(cx);
            }))
            .on_action(cx.listener(|this, _: &Close, window, cx| {
                if this.menu.is_some() {
                    this.escape(window, cx);
                } else if this.help || this.about || this.form.is_some() {
                    this.close_top(window, cx);
                } else if this.reader.finding {
                    this.close_find(window, cx);
                } else if this.model.view() == View::Docs
                    && this.model.docs.reading
                    && this.model.detail.is_none()
                {
                    // Done reading: the list again.
                    this.leave_reading(window, cx);
                } else if this.model.view() == View::Activity
                    && this.model.detail.is_none()
                    && this.model.grid.pick.span.is_some()
                {
                    // In the feed, the days picked in the grid give way to every day.
                    this.model.grid_unpick();
                    cx.notify();
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
            .on_action(cx.listener(|this, _: &FocusFilter, window, cx| {
                this.menu = Some(Menu::Filter);
                this.filter.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Note, window, cx| {
                if this.model.detail.is_none() {
                    this.model.open_detail();
                }
                // Only into a panel that is there: with nothing to open (the graph with no card
                // chosen), the keys stay where they are.
                if this.model.detail.is_some() {
                    this.note.read(cx).focus_handle(cx).focus(window, cx);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleLayout, _, cx| {
                if this.model.view() == View::Tasks {
                    this.model.toggle_layout();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleAll, _, cx| {
                this.model.toggle_dismissed();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleEmpty, _, cx| {
                this.model.toggle_empty();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_graph(ZOOM_STEP, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_graph(1. / ZOOM_STEP, cx)))
            .on_action(cx.listener(|this, _: &ZoomFit, _, cx| {
                if this.model.view() == View::Graph {
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
        // The terminal takes the list's place, outside the list's keys, so every plain key
        // reaches the shell.
        let find_bar =
            (self.model.view() == View::Docs && self.reader.finding).then(|| self.find_bar(cx));
        // Editing a doc: its editor in the list's place too, so typing is text.
        let editing = (self.model.view() == View::Docs)
            .then(|| self.doc_editor(cx))
            .flatten();
        let main = match (self.terminal_view(cx), editing) {
            (_, Some(e)) => e,
            (Some(t), None) => self.terminal_element(t, cx),
            (None, None) => list_area.into_any_element(),
        };
        // The focused pane: its view with the keys, the dock floating over its bottom, the docs'
        // find bar over its top right (outside the list's keys, so typing in it is text).
        let focused = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(main)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(DOCK_GAP))
                    .flex()
                    .justify_center()
                    .children(dock),
            )
            .children(find_bar)
            .into_any_element();
        let panes = self.pane_tree(focused, window, cx);
        let split_drag = self.split_drag_listener(cx);
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(color(WINDOW))
            .text_color(color(TEXT_2))
            .text_sm()
            .when(dragging_edge, |d| d.cursor(CursorStyle::ResizeLeftRight))
            // Esc closes an open menu before anything focused hears it: the terminal, which
            // takes every key, included.
            .capture_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" && this.menu.is_some() {
                    this.close_menu(window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_action(
                cx.listener(|this, _: &OpenPalette, window, cx| this.toggle_palette(window, cx)),
            )
            // From anywhere, typing included, as their keys (Cmd on macOS) and the menu bar do.
            .on_action(cx.listener(|this, _: &NewTask, window, cx| this.open_form(window, cx)))
            .on_action(cx.listener(|this, _: &OpenFolder, _, cx| this.open_folder(cx)))
            .on_action(cx.listener(|this, _: &GoInbox, w, cx| this.go(View::Inbox, w, cx)))
            .on_action(cx.listener(|this, _: &GoActivity, w, cx| this.go(View::Activity, w, cx)))
            .on_action(cx.listener(|this, _: &GoTasks, w, cx| this.go(View::Tasks, w, cx)))
            .on_action(cx.listener(|this, _: &GoLessons, w, cx| this.go(View::Lessons, w, cx)))
            .on_action(cx.listener(|this, _: &GoDecisions, w, cx| this.go(View::Decisions, w, cx)))
            .on_action(cx.listener(|this, _: &GoFindings, w, cx| this.go(View::Findings, w, cx)))
            .on_action(cx.listener(|this, _: &GoGraph, w, cx| this.go(View::Graph, w, cx)))
            .on_action(cx.listener(|this, _: &GoTerminal, w, cx| this.go(View::Terminal, w, cx)))
            .on_action(cx.listener(|this, _: &GoDocs, w, cx| this.go(View::Docs, w, cx)))
            .on_action(
                cx.listener(|this, link: &FollowLink, _, cx| this.follow_link(&link.href, cx)),
            )
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| {
                if cfg!(target_os = "macos") {
                    cx.hide();
                } else {
                    window.remove_window();
                }
                let _ = this;
            }))
            .on_action(cx.listener(|_, _: &Minimize, window, _| window.minimize_window()))
            .on_action(cx.listener(|_, _: &ZoomWindow, window, _| window.zoom_window()))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.show_over_form(window, cx);
                this.settings = true;
                this.modal_focus.focus(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &About, window, cx| {
                this.show_over_form(window, cx);
                this.about = true;
                this.modal_focus.focus(window, cx);
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &AddLinkMenu, window, cx| {
                    this.add_link_from_menu(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &CheckForUpdates, _, cx| {
                this.check_updates(true, cx);
            }))
            .on_action(cx.listener(|this, _: &RestartToUpdate, _, cx| {
                this.restart_to_update(cx);
            }))
            .on_action(cx.listener(|this, _: &InstallCli, _, cx| {
                this.install_cli();
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &menus::Project1, w, cx| this.switch_project(0, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &menus::Project2, w, cx| this.switch_project(1, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &menus::Project3, w, cx| this.switch_project(2, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &menus::Project4, w, cx| this.switch_project(3, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &menus::Project5, w, cx| this.switch_project(4, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &menus::Project6, w, cx| this.switch_project(5, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &menus::Project7, w, cx| this.switch_project(6, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &menus::Project8, w, cx| this.switch_project(7, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &menus::Project9, w, cx| this.switch_project(8, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleBottom, _, cx| this.toggle_bottom(cx)))
            .on_action(cx.listener(|this, _: &TogglePanel, _, cx| this.toggle_panel(cx)))
            .on_action(
                cx.listener(|this, _: &SplitRight, w, cx| this.split_pane(PaneAxis::Row, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SplitDown, w, cx| this.split_pane(PaneAxis::Column, w, cx)),
            )
            .on_action(cx.listener(|this, _: &ClosePane, w, cx| this.close_pane(w, cx)))
            .on_action(cx.listener(|this, _: &NextPane, w, cx| this.next_pane(1, w, cx)))
            .on_action(cx.listener(|this, _: &PrevPane, w, cx| this.next_pane(-1, w, cx)))
            .on_action(cx.listener(|this, _: &NewTerminal, w, cx| this.new_terminal(w, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleHelp, window, cx| {
                    this.set_help(!this.help, window, cx)
                }),
            )
            // Esc in a text field gives the keys back to the list, wherever the field is: the
            // one rule for every field that does nothing more with it. Those that do (the filter
            // closes its menu, an editor cancels, a modal closes) take it first, as GPUI stops an
            // action at the first handler, so it reaches here only from the rest (a note).
            .on_action(cx.listener(|this, _: &input::Escape, window, cx| {
                // Editing a doc, Esc leaves (asking first, with unsaved text).
                if this.reader.editor.is_some() {
                    this.doc_leave_edit(false, window, cx);
                    return;
                }
                // In the find bar, Esc shuts it.
                if this.reader.finding {
                    this.close_find(window, cx);
                    return;
                }
                this.focus_main(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FindInDoc, window, cx| this.open_find(window, cx)))
            .on_action(cx.listener(|this, _: &SaveDoc, _, cx| this.doc_save(false, cx)))
            .on_action(cx.listener(|this, _: &DocBack, _, cx| {
                if this.model.view() == View::Docs {
                    this.model.doc_back();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &DocForward, _, cx| {
                if this.model.view() == View::Docs {
                    this.model.doc_forward();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, r: &RevealInDoc, _, cx| this.reveal_in_doc(r.top, cx)))
            .child(top_bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .flex()
                    .child(sidebar)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .pb_2()
                            .pr_2()
                            .relative()
                            .flex()
                            .flex_col()
                            .children(panel_edge.map(|e| e.right_0()))
                            .child(div().w_full().flex_1().min_h_0().flex().child(panes))
                            .children(bottom),
                    )
                    // Beside the content box, on the window's own black, as the sidebar is: the content
                    // stands alone in the middle.
                    .children(detail),
            )
            .children(form)
            .children(palette)
            .children(help)
            .children(about)
            .children(settings)
            .children(notice)
            .children(drag)
            .children(split_drag)
    }
}

/// How tall the app's own titlebar is: `TITLEBAR` on macOS, none in full screen, where macOS
/// hides the traffic lights, and none elsewhere.
fn titlebar_height(window: &Window) -> f32 {
    if TITLEBAR > 0. && !window.is_fullscreen() {
        TITLEBAR
    } else {
        0.
    }
}

/// A toggle of the top bar's: `icon`, lit while its panel is `open`, named with its keys in
/// `tip`. Its press is its own: it never drags the window.
fn panel_toggle(
    id: &'static str,
    i: Icon,
    open: bool,
    tip: String,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let tip = SharedString::from(tip);
    div()
        .id(id)
        .debug_selector(move || id.into())
        .group(id)
        .size(px(31.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .hover(|h| h.bg(color(HOVER)))
        .child(if open {
            icon(i, 23., TEXT_2)
        } else {
            icon_hover(i, 23., TEXT_4, id, TEXT_2)
        })
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
}

/// A project's name as the top bar and its menu show it: its first letter a capital.
fn capitalized(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

fn prio(app: &mut LivtkApp, p: u8, cx: &mut Context<LivtkApp>) {
    app.model.set_priority(p);
    cx.notify();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Menu {
    Filter,
    Grouping,
    Ordering,
    /// How a list is shown: the tasks' list or board, its empty groups, its dismissed work.
    View,
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
    /// The open node's state, in the detail panel.
    DetailState,
    /// Ratify or ratify as a standing rule, from the flow in the detail panel.
    Standing,
    /// The projects, from the collapsed sidebar's rail.
    Projects,
    /// The activity grid's range, and what it counts.
    GridWhat,
}

/// The selection's highlight gliding between two neighbouring lines, named as `spot` names them.
struct Glide {
    from: String,
    to: String,
    /// Towards the end of the list.
    down: bool,
    epoch: usize,
}

/// The highlight on its way between two lines, drawn by the upper one: from and to offsets in
/// rows below it.
fn glide_highlight(from: f32, to: f32, epoch: usize) -> AnyElement {
    div()
        .debug_selector(|| "glide".into())
        .absolute()
        .left_0()
        .right_0()
        .h(px(ROW))
        .bg(color(SELECTED))
        .with_animation(
            SharedString::from(format!("glide-{epoch}")),
            Animation::new(GLIDE).with_easing(ease_out_cubic),
            move |el, t| el.top(px((from + (to - from) * t) * ROW)),
        )
        .into_any_element()
}

/// An edge the human drags to resize: the sidebar's right, the detail panel's left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    Sidebar,
    Panel,
    /// The bottom bar's top.
    Bottom,
}

/// The pointer on an edge: which, how far along it, and which arrival (each fades in).
#[derive(Clone, Copy, Debug, PartialEq)]
struct EdgeHover {
    edge: Edge,
    at: f32,
    epoch: usize,
}

/// An edge spans the gutter between the content box and the pane beside it.
const EDGE_W: f32 = 8.;
/// The glow along a hovered edge, and the handle at its middle.
const GLOW_LEN: f32 = 120.;
const HANDLE: f32 = 20.;
/// The glow fading in as the pointer arrives, and out as it leaves.
const GLOW_IN: Duration = Duration::from_millis(140);
const GLOW_OUT: Duration = Duration::from_millis(180);

/// The glow on an edge at the pointer: a lens of light, accent at its middle and thickest
/// there, thinning and fading to nothing at both ends, and the handle on it. Three pills of
/// light make the lens, each shorter and wider than the last; where they overlap the light is
/// strongest. Drawn last (deferred), so it lies over the panes on either side; it takes no
/// pointer events. While dragging it is brighter.
fn edge_glow(h: EdgeHover, dragging: bool, len: f32, leaving: bool) -> AnyElement {
    let half = GLOW_LEN / 2.;
    let at = h.at.clamp(half, (len - half).max(half));
    let strength = if dragging { 1. } else { 0.75 };
    // Down a side edge, or across the bottom one: the same glow turned.
    let across = h.edge == Edge::Bottom;
    let pill = |w: f32, len: f32, alpha: f32| {
        let lit = with_alpha(ACCENT, alpha * strength);
        let clear = with_alpha(ACCENT, 0.);
        let (into, out_of) = (
            linear_gradient(
                if across { 90. } else { 180. },
                linear_color_stop(clear, 0.),
                linear_color_stop(lit, 1.),
            ),
            linear_gradient(
                if across { 90. } else { 180. },
                linear_color_stop(lit, 0.),
                linear_color_stop(clear, 1.),
            ),
        );
        let d = div().absolute().flex();
        if across {
            d.left(px((GLOW_LEN - len) / 2.))
                .top(px((HANDLE - w) / 2.))
                .w(px(len))
                .h(px(w))
                .child(div().flex_1().rounded_l_full().bg(into))
                .child(div().flex_1().rounded_r_full().bg(out_of))
        } else {
            d.top(px((GLOW_LEN - len) / 2.))
                .left(px((HANDLE - w) / 2.))
                .w(px(w))
                .h(px(len))
                .flex_col()
                .child(div().flex_1().rounded_t_full().bg(into))
                .child(div().flex_1().rounded_b_full().bg(out_of))
        }
    };
    let glow = div().debug_selector(|| "edge-glow".into()).absolute();
    let glow = if across {
        glow.left(px(at - half))
            .top(px((EDGE_W - HANDLE) / 2.))
            .w(px(GLOW_LEN))
            .h(px(HANDLE))
    } else {
        glow.top(px(at - half))
            .left(px((EDGE_W - HANDLE) / 2.))
            .w(px(HANDLE))
            .h(px(GLOW_LEN))
    };
    let resize = icon(Icon::Resize, 12., TEXT);
    let resize = if across {
        resize.with_transformation(Transformation::rotate(radians(std::f32::consts::FRAC_PI_2)))
    } else {
        resize
    };
    let handle = div().debug_selector(|| "edge-handle".into()).absolute();
    let handle = if across {
        handle.left(px((GLOW_LEN - HANDLE) / 2.)).top_0()
    } else {
        handle.top(px((GLOW_LEN - HANDLE) / 2.)).left_0()
    };
    deferred(
        glow.child(pill(1., GLOW_LEN, 0.9))
            .child(pill(2., GLOW_LEN * 0.66, 0.7))
            .child(pill(4., GLOW_LEN * 0.4, 0.6))
            .child(
                handle
                    .size(px(HANDLE))
                    .rounded_full()
                    // A ring of the window's own black sets it off the content box's border.
                    .border_2()
                    .border_color(color(WINDOW))
                    .bg(color(if dragging { ACCENT_HOVER } else { ACCENT }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(resize),
            )
            .with_animation(
                SharedString::from(format!(
                    "edge-glow-{}-{}",
                    h.epoch,
                    if leaving { "out" } else { "in" }
                )),
                Animation::new(if leaving { GLOW_OUT } else { GLOW_IN })
                    .with_easing(ease_out_cubic),
                move |el, t| el.opacity(if leaving { 1. - t } else { t }),
            ),
    )
    .into_any_element()
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

/// One row of the room at the end of a virtualised list: empty, under the dock.
fn dock_room() -> AnyElement {
    div().w_full().h(px(ROW)).into_any_element()
}

/// A button on the collapsed sidebar's rail: an icon, a dot when its view has work waiting, and
/// its name as a tooltip.
fn rail_button(
    id: SharedString,
    glyph: Icon,
    on: bool,
    badge: bool,
    tip: SharedString,
) -> Stateful<Div> {
    let name = id.clone();
    div()
        .id(id)
        .debug_selector(move || name.to_string())
        .relative()
        .size(px(36.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_lg()
        .when(on, |d| d.bg(color(SELECTED)))
        .when(!on, |d| d.hover(|s| s.bg(color(HOVER))))
        .child(icon(glyph, 18., if on { TEXT } else { TEXT_3 }))
        .when(badge, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(6.))
                    .right(px(6.))
                    .size(px(7.))
                    .rounded_full()
                    .bg(color(ACCENT))
                    .debug_selector(|| "rail-badge".into()),
            )
        })
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
}

/// A non-task row's leading slot: its kind's icon, as wide as a task's priority button.
fn kind_slot(kind: Kind) -> Div {
    let glyph = match kind {
        Kind::Task => Icon::Tasks,
        Kind::Lesson => Icon::Lessons,
        Kind::Decision => Icon::Decisions,
        Kind::Finding => Icon::Findings,
    };
    div()
        .w(px(LEAD))
        .h(px(22.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .child(icon(glyph, 14., TEXT_4).debug_selector(|| "kind-icon".into()))
}

/// A view's icon, as the sidebar and the palette show it.
fn view_icon(view: View) -> Icon {
    match view {
        View::Inbox => Icon::Inbox,
        View::Activity => Icon::Activity,
        View::Tasks => Icon::Tasks,
        View::Lessons => Icon::Lessons,
        View::Decisions => Icon::Decisions,
        View::Findings => Icon::Findings,
        View::Graph => Icon::Graph,
        View::Terminal => Icon::Terminal,
        View::Docs => Icon::Docs,
    }
}

/// Beside project `i`'s name while its store catches up with its log off the window's thread.
fn catching_up(i: usize) -> Div {
    div()
        .debug_selector(move || format!("catching-up-{i}"))
        .flex_none()
        .whitespace_nowrap()
        .text_xs()
        .text_color(color(TEXT_4))
        .child("Catching up…")
}

fn nav_item(
    id: SharedString,
    glyph: Icon,
    label: &str,
    active: bool,
    count: usize,
) -> Stateful<Div> {
    let name = id.clone();
    div()
        .id(id)
        .debug_selector(move || name.to_string())
        .group("nav")
        .px_2()
        .h(px(NAV_ROW))
        .text_size(px(NAV_TEXT))
        .rounded_lg()
        .flex()
        .items_center()
        .gap_2()
        .when(active, |d| d.bg(color(SELECTED)).text_color(color(TEXT)))
        .when(!active, |d| {
            d.text_color(color(TEXT_3))
                .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT_2)))
        })
        .child(if active {
            icon(glyph, NAV_ICON, TEXT)
        } else {
            icon_hover(glyph, NAV_ICON, TEXT_4, "nav", TEXT_3)
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
/// A state's mark in the flow and the state menu: at full strength where the node is or can go,
/// so the backlog's mark, faint by Linear's convention, never reads as unavailable when it is a
/// move.
fn flow_mark(kind: Kind, s: &Step) -> Div {
    let mark = state_mark(kind, s.state);
    if s.look() == Look::Out {
        mark
    } else {
        mark.opacity(1.)
    }
}

/// Who did something, in the feed and the timeline: you, or Claude (sparkles, as AI is drawn).
fn avatar(human: bool) -> Div {
    div()
        .size(px(20.))
        .flex_none()
        .rounded_full()
        .bg(color(if human { ACCENT } else { BORDER_STRONG }))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(
            if human { Icon::You } else { Icon::Claude },
            12.,
            TEXT,
        ))
}

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

/// The section an item of the palette sits in.
fn section_of(item: &PaletteItem, query: &str) -> &'static str {
    match item {
        PaletteItem::Action(_) => "Actions",
        PaletteItem::Node(_) if query.trim().is_empty() => "Updated lately",
        PaletteItem::Node(_) => "Results",
        PaletteItem::Doc(_) => "Docs",
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
        .text_color(color(TEXT_2))
        .hover(|s| s.bg(color(HOVER)).text_color(color(TEXT)))
}

/// A row of the View options menu: its box, its label and its key, `act` on a click.
fn view_option(
    id: &'static str,
    label: &'static str,
    key: Option<&'static str>,
    on: bool,
    act: fn(&mut Model),
    cx: &mut Context<LivtkApp>,
) -> Stateful<Div> {
    option_row(id.into())
        .debug_selector(move || id.into())
        .child(checkbox(on))
        .child(label)
        .when_some(key, |row, k| row.child(div().ml_auto().child(kbd(k))))
        .on_click(cx.listener(move |this, _, _, cx| {
            act(&mut this.model);
            cx.notify();
        }))
}

/// The mark of the chosen option in a menu that picks one, at the row's right end.
fn check(on: bool) -> Div {
    div()
        .ml_auto()
        .pl_2()
        .flex()
        .items_center()
        .when(on, |d| d.child(icon(Icon::Check, 14., TEXT)))
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
        .when(on, |c| {
            c.bg(color(ACCENT))
                .border_color(color(ACCENT))
                .child(icon(Icon::Check, 11., TEXT))
        })
        .when(!on, |c| c.border_color(color(TEXT_4)))
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

/// A tag led by an icon in its colour instead of a dot: blocked, standing rule.
fn icon_tag(glyph: Icon, label: &str, c: u32) -> Div {
    div()
        .h(px(20.))
        .px(px(7.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(5.))
        .rounded_full()
        .text_xs()
        .bg(color(SURFACE))
        .border_1()
        .border_color(color(BORDER_STRONG))
        .text_color(color(TEXT_2))
        .child(icon(glyph, 12., c))
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

/// A key cap that shows an icon rather than a letter: Enter.
fn kbd_icon(glyph: Icon) -> Div {
    div()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(3.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .bg(color(LIFTED))
        .border_1()
        .border_color(color(BORDER_STRONG))
        .child(icon(glyph, 12., TEXT_3))
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

/// An empty view: its own icon, large and faded, what is missing, and how to fill it.
fn empty(glyph: Icon, title: &str, hint: &str) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .child(icon(glyph, 32., BORDER_STRONG).mb_2())
        .child(div().text_color(color(TEXT_2)).child(title.to_owned()))
        .child(
            div()
                .text_xs()
                .text_color(color(TEXT_4))
                .child(hint.to_owned()),
        )
}

/// An empty state's way on: one button, the accent's.
fn empty_action(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .mt_3()
        .h(px(28.))
        .px_3()
        .flex()
        .items_center()
        .rounded_lg()
        .bg(color(ACCENT))
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color(TEXT))
        .hover(|h| h.bg(color(ACCENT_HOVER)))
        .child(label)
        .on_click(on_click)
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

/// One event of the timeline of the node keyed `key` now.
fn event_line(e: &Event, key: &str) -> Div {
    let when = ago(&e.at);
    let what = match &e.op {
        Op::NodeCreate { .. } => match created_as(e, key) {
            Some(first) => format!("created it as {first}"),
            None => "created it".to_owned(),
        },
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
        Op::DocLink { data, .. } => format!("linked the doc {}", data.path),
        Op::DocUnlink { data, .. } => format!("unlinked the doc {}", data.path),
    };
    let human = e.by == "human";
    let note = matches!(e.op, Op::NodeNote { .. });
    let head = div()
        .flex()
        .items_center()
        .gap_2()
        .child(avatar(human))
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
                // Markdown, as the description: emphasis, lists, links, code by language.
                .child(TextView::markdown(
                    SharedString::from(format!("note-{}", e.id)),
                    crate::model::note_markdown(&what),
                )),
        )
    })
}

/// `el` around a growing text field, taking its Tab and Shift+Tab: gpui's text area indents
/// only a field of fixed height and passes them on from one that grows, where they would move
/// the focus. Here they indent and outdent the lines the selection touches, as one edit.
fn indented(el: Div, field: &Entity<TextareaState>) -> Div {
    let (a, b) = (field.clone(), field.clone());
    el.on_action(move |_: &input::IndentInline, window, cx| reindent_field(&a, false, window, cx))
        .on_action(move |_: &input::OutdentInline, window, cx| reindent_field(&b, true, window, cx))
}

fn reindent_field(field: &Entity<TextareaState>, out: bool, window: &mut Window, cx: &mut App) {
    cx.stop_propagation();
    field.update(cx, |s, cx| {
        let text = s.value().to_string();
        let (range, new, after) = crate::model::reindent(&text, s.selected_range(), out);
        s.set_selected_range(range, cx);
        s.replace(new, window, cx);
        s.set_selected_range(after, cx);
    });
}

/// What the notifications show, to tell when the window must draw again: each shown one and
/// whether it is leaving.
fn told(m: &Model) -> (Vec<(u64, bool, usize)>, usize) {
    let (shown, more) = m.toasts().shown();
    let shown = shown
        .iter()
        .map(|t| (t.id, t.leaving.is_some(), t.count))
        .collect();
    (shown, more)
}

/// A sub-task shown away from its parent: the parent's key, quietly.
fn parent_chip(key: &str) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .text_color(color(TEXT_4))
        .child(icon(Icon::SubTask, 12., TEXT_4))
        .child(key.to_owned())
}

/// A board column's width.
const BOARD_COLUMN: f32 = 256.;
/// The palette's width.
const PALETTE_W: f32 = 640.;

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
