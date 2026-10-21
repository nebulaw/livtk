//! The window: the top bar (the project, the palette's field, the panels' toggles), a sidebar on
//! the black canvas, an inset content panel (the view itself, its dock floating over it), the
//! bottom bar, the detail panel, and overlays (new task, shortcuts, notices). It renders the model
//! and forwards input to it; no behaviour lives here.
//!
//! Keys are bound only on the list area. Text fields (the filter, notes, the new-task form) and
//! the terminal live outside it, so typing never triggers a command.

mod activity_grid;
mod agents;
pub mod contour_grid;
mod detail;
pub mod diagram;
mod dock;
mod docs;
mod feed;
mod form;
mod graph;
pub mod icons;
mod list;
pub mod menus;
mod node_ref;
mod notices;
mod overlays;
mod panes;
mod peek;
mod runs;
mod settings;
mod sidebar;
pub mod state_flow;
pub mod terminal;
mod terminals;
mod theme;

use detail::*;
use dock::*;
use node_ref::*;
use overlays::*;
use sidebar::*;
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
use livtk_core::format::duration;
use livtk_core::{Kind, State, Summary};
use uuid::Uuid;

use crate::model::Axis as PaneAxis;
use crate::model::Split;
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

/// Go to `view`: one action for every view, its keys from `View::go_keys`.
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = livtk, no_json)]
pub struct GoView {
    pub view: View,
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

/// A board column's width.
pub(super) const BOARD_COLUMN: f32 = 256.;

/// The palette's width.
pub(super) const PALETTE_W: f32 = 640.;

pub(super) const GROUPINGS: &[(&str, &str, Grouping)] = &[
    ("group-state", "State", Grouping::State),
    ("group-priority", "Priority", Grouping::Priority),
    ("group-project", "Project", Grouping::Project),
    ("group-none", "No grouping", Grouping::None),
];

pub(super) const ORDERINGS: &[(&str, &str, Ordering)] = &[
    ("order-priority", "Priority", Ordering::Priority),
    ("order-updated", "Last updated", Ordering::Updated),
    ("order-created", "Created", Ordering::Created),
];

pub(super) const PRIORITY_IDS: [&str; 4] = ["priority-1", "priority-2", "priority-3", "priority-4"];
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
        act: impl Fn(&A, &mut LivtkApp, &mut Window, &mut Context<LivtkApp>) + 'static,
    ) {
        let app = app.clone();
        cx.on_action(move |a: &A, cx| {
            cx.activate(true);
            let _ = window.update(cx, |_, window, cx| {
                window.activate_window();
                app.update(cx, |app, cx| act(a, app, window, cx))
            });
        });
    }
    on::<NewTask>(window, &app, cx, |_, app, w, cx| app.open_form(w, cx));
    on::<GoView>(window, &app, cx, |a, app, w, cx| app.go(a.view, w, cx));
    on::<menus::SwitchProject>(window, &app, cx, |a, app, w, cx| {
        app.switch_project(a.index, w, cx)
    });
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
    // G and a view's key goes there, each view saying its own key.
    cx.bind_keys(View::ALL.into_iter().filter_map(|view| {
        let keys = view.go_keys()?.to_lowercase();
        Some(KeyBinding::new(&keys, GoView { view }, nav))
    }));
    cx.bind_keys([
        KeyBinding::new("j", Down, nav),
        KeyBinding::new("down", Down, nav),
        KeyBinding::new("k", Up, nav),
        KeyBinding::new("up", Up, nav),
        KeyBinding::new("enter", Open, nav),
        KeyBinding::new("escape", Close, nav),
        KeyBinding::new("escape", Close, Some(peek::DETAIL)),
        KeyBinding::new("escape", Close, Some(settings::SETTINGS)),
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
    /// A pane just split off, growing in; a pane just closed, shrinking away in the layout it
    /// left (drawn empty); and which of each, so each animates once.
    pane_born: Option<u64>,
    pane_gone: Option<(Split, u64)>,
    pane_epoch: usize,
    /// The split's views as the sidebar last laid them out, and how each line moves from where
    /// it was to where it is now (or comes in), while it does: a view joining the split slides
    /// up into its group, one leaving slides back down, the rest make room.
    side_was: Option<Vec<(u64, Option<View>)>>,
    side_moves: HashMap<String, panes::SideMove>,
    side_epoch: usize,
    split_bounds: std::cell::RefCell<HashMap<Vec<usize>, Bounds<Pixels>>>,

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
    /// The runs modal, when it asks something.
    asking: Option<runs::Asking>,
    /// The runs the last quit stopped were offered back this launch.
    runs_offered: bool,
    /// Settings, open.
    settings: settings::Settings,
    /// Agents' page for an agent, and the Sessions view's polling.
    agent_form: agents::AgentForm,
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
    pub fn new(mut model: Model, window: &mut Window, cx: &mut Context<Self>) -> Self {
        report_unknown_icons(&mut model);
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
        let settings = settings::Settings::new(window, cx);
        let agent_form = agents::AgentForm::new(window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&note, window, Self::on_note),
            cx.subscribe_in(&filter, window, Self::on_filter),
            cx.subscribe_in(&palette_input, window, Self::on_palette_input),
            cx.subscribe_in(&reader.find, window, Self::on_find),
            cx.subscribe_in(&settings.gate, window, Self::on_gate_input),
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
                    app.survey_runs(cx);
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
            pane_born: None,
            pane_gone: None,
            pane_epoch: 0,
            side_was: None,
            side_moves: HashMap::new(),
            side_epoch: 0,
            split_bounds: Default::default(),
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
            asking: None,
            runs_offered: false,
            settings,
            agent_form,
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
            Menu::DetailMode => "detail-mode",
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
        if self.help || self.about || self.asking.is_some() {
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
            v if v.has_board() && self.model.layout() == Layout::Board => None,
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
        if self.asking.is_some() {
            // Asked about runs: closing answers nothing, so nothing quits or starts.
            self.asking = None;
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
                .filter(|&s| d.show_dismissed || !kind.is_dismissed(s))
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
        // The settings list their sections in the full sidebar, whatever the window.
        if self.settings.open.is_some() {
            return false;
        }
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
        (self.model.detail.is_some() && self.model.detail_mode == crate::model::DetailMode::Side)
            || self.doc_panel_open()
            || self.agents_panel_open()
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
            (View::Agents, _) => self.agents_view(window, cx),
            (View::Sessions, _) => self.sessions_view(cx),
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

    /// Make the menu bar and the Dock menu again when what they show changed: the projects, the
    /// view and its options, what waits in the inbox.
    fn sync_menus(&mut self, cx: &mut Context<Self>) {
        let m = &self.model;
        let d = m.display();
        let state = menus::MenuState {
            projects: m.projects.iter().map(|p| p.name.clone()).collect(),
            current: m.current,
            view: Some(m.view()),
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
        report_unknown_icons(&mut self.model);
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
        let rail = self.rail(window);
        self.track_side(rail, cx);
        let sidebar = self.sidebar(window, cx);
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
        let asking = self.runs_overlay(cx);
        let palette = self.palette_overlay(cx);
        let peek = self.center_peek(window, cx);
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
                } else if this.help || this.about || this.asking.is_some() || this.form.is_some() {
                    this.close_top(window, cx);
                } else if this.reader.finding && this.model.view() == View::Docs {
                    this.close_find(window, cx);
                } else if this.model.view() == View::Docs
                    && this.model.docs.reading
                    && this.model.detail.is_none()
                {
                    // Done reading: the list again.
                    this.leave_reading(window, cx);
                } else if this.model.agents_back(this.model.view()) {
                    // In Sessions, a session read gives way to the list, then the panel to
                    // nothing; in Agents, the panel.
                    cx.notify();
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
                if this.model.view().has_board() {
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
        // A detail opened as a full page from this pane: in the list's place.
        let full = self.full_page(self.content_width(window), true, cx);
        let main = match (self.terminal_view(cx), editing) {
            _ if full.is_some() => full.unwrap_or_else(|| div().into_any_element()),
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
        // Settings open: their page in the middle, one box, the panes behind as they were.
        let panes = if self.settings.open.is_some() {
            div()
                .size_full()
                .flex()
                .rounded_xl()
                .border_1()
                .border_color(color(BORDER))
                .bg(color(CONTENT))
                .overflow_hidden()
                .child(self.settings_page(cx))
                .into_any_element()
        } else {
            self.pane_tree(focused, window, cx)
        };

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
                if this.settings.open.is_some() {
                    this.close_settings(window, cx);
                } else {
                    this.open_settings(settings::Section::Appearance, window, cx);
                }
            }))
            // Quitting from the window asks first while agents are working.
            .on_action(cx.listener(|this, _: &Quit, window, cx| this.ask_quit(window, cx)))
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
            .on_action(cx.listener(|this, a: &GoView, w, cx| this.go(a.view, w, cx)))
            .on_action(cx.listener(|this, a: &menus::SwitchProject, w, cx| {
                this.switch_project(a.index, w, cx)
            }))
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
            .children(peek)
            .children(palette)
            .children(help)
            .children(about)
            .children(asking)
            .children(notice)
            .children(drag)
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
    /// How a node's detail opens: side peek, center peek, full page.
    DetailMode,
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
    /// The gutter between two panes, named by the first pane either side of it; `across`
    /// between stacked panes.
    Divider {
        a: u64,
        b: u64,
        across: bool,
    },
}

impl Edge {
    /// Whether it runs across, so its glow lies along it sideways.
    fn across(self) -> bool {
        matches!(self, Edge::Bottom | Edge::Divider { across: true, .. })
    }
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

enum Item {
    Band(usize),
    Row(usize),
}

// ---- small pieces ---------------------------------------------------------------------------------

/// The flow above the activity feed, in one quiet line: what waits on the human and for how
/// long, and the median time in review.
fn flow_strip(f: &livtk_core::Flow) -> Option<Div> {
    let mut parts: Vec<String> = f
        .queues
        .iter()
        .map(|q| {
            format!(
                "{} {} waiting on you, the longest {} ({})",
                q.count,
                state_name(q.state).to_lowercase(),
                q.oldest,
                duration(q.oldest_secs)
            )
        })
        .collect();
    if let Some(r) = f.states.iter().find(|t| t.state == State::Review) {
        parts.push(format!("median in review {}", duration(r.median_secs)));
    }
    if parts.is_empty() {
        return None;
    }
    Some(
        div()
            .debug_selector(|| "flow-strip".into())
            .h(px(ROW))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(color(BORDER))
            .text_xs()
            .text_color(color(TEXT_3))
            .child(icon(Icon::You, 12., TEXT_4))
            .child(parts.join(" · ")),
    )
}

pub(super) fn section_label(text: &str) -> Div {
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

/// A view's icon, as the sidebar and the palette show it.
pub fn view_icon(view: View) -> Icon {
    // A view of one kind wears its kind's icon: one place says what a kind looks like.
    if let Some(kind) = view.kind() {
        return kind_icon(kind);
    }
    match view {
        View::Inbox => Icon::Inbox,
        View::Activity => Icon::Activity,
        View::Agents => Icon::Agents,
        View::Sessions => Icon::Sessions,
        View::Graph => Icon::Graph,
        View::Terminal => Icon::Terminal,
        View::Docs => Icon::Docs,
        // Views of one kind are answered above.
        _ => Icon::Grouping,
    }
}

/// Say which declared kinds name an icon the app does not have, and the names it has: the kind
/// shows a general icon meanwhile.
fn report_unknown_icons(model: &mut Model) {
    let Some(config) = model.config() else {
        return;
    };
    let unknown: Vec<String> = config
        .kinds
        .iter()
        .filter_map(|k| {
            let name = k.icon.as_deref()?;
            Icon::named(name)
                .is_none()
                .then(|| format!("[[kinds]] `{}`: no icon `{name}`", k.name))
        })
        .collect();
    if !unknown.is_empty() {
        let names: Vec<&str> = Icon::ALL.iter().map(|i| i.file()).collect();
        model.warn(format!(
            "{}. One of: {}.",
            unknown.join("; "),
            names.join(", ")
        ));
    }
}

/// A kind's icon, wherever the kind shows: its view, its rows' slot, its groups. A declared kind
/// wears the icon its config names (`icon = "flag"`), else a general one.
pub fn kind_icon(kind: Kind) -> Icon {
    match kind {
        Kind::Task => Icon::Tasks,
        Kind::Lesson => Icon::Lessons,
        Kind::Decision => Icon::Decisions,
        Kind::Finding => Icon::Findings,
        Kind::Project => Icon::Projects,
        _ => kind
            .icon_name()
            .and_then(|name| Icon::named(&name))
            .unwrap_or(Icon::Grouping),
    }
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
fn state_dot(kind: Kind, state: State, size: f32) -> Div {
    let faint = matches!(state, State::Backlog | State::Parked) || kind.is_dismissed(state);
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(with_alpha(
            state_color(kind, state),
            if faint { 0.5 } else { 1. },
        ))
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
pub(super) fn event_line(e: &Event, key: &str) -> Div {
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
        Op::NodeSession { data, .. } => livtk_core::format::claim(data),
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

/// "just now", "5m", "3h", "2d", or the date, as Linear shows ages.
pub(super) fn ago(at: &str) -> String {
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
