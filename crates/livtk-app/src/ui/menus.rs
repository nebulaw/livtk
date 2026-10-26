//! The menu bar and the Dock menu: one table of the commands the app already has, so the menus,
//! the keys and the command palette never disagree. GPUI shows each item's key from its binding, and
//! macOS greys an item no focused handler takes. The table is the same on every platform; one
//! without a menu bar shows none.

use gpui_kit::component::input;
use gpui_kit::*;

use crate::model::View;

use super::{
    About, AddLinkMenu, BringToFront, CheckForUpdates, CloseWindow, Decide, GoView, HideApp,
    HideOthers, InstallCli, Minimize, NewProject, NewTask, Note, OpenFolder, OpenGitHub,
    OpenPalette, OpenSettings, Priority1, Priority2, Priority3, Priority4, Quit, Reject,
    ReportIssue, RestartToUpdate, ShowAll, Standing, ToggleAll, ToggleBottom, ToggleEmpty,
    ToggleHelp, ToggleLayout, TogglePanel, ToggleSidebar, ZoomFit, ZoomIn, ZoomOut, ZoomWindow,
};

/// What the menus show of the app's state: the projects and which is open, the view and its
/// options, ticked as a Mac app ticks them, and what waits in the inbox. The menus are made again
/// only when it changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuState {
    pub projects: Vec<String>,
    pub current: usize,
    /// The view shown.
    pub view: Option<View>,
    pub board: bool,
    pub show_empty: bool,
    pub show_dismissed: bool,
    pub inbox: usize,
    /// A newer build staged and waiting, by its number.
    pub update: Option<u64>,
}

/// An item that runs `action`, ticked when `checked`.
fn item(name: impl Into<SharedString>, action: impl Action, checked: bool) -> MenuItem {
    MenuItem::Action {
        name: name.into(),
        action: Box::new(action),
        os_action: None,
        checked,
        disabled: false,
    }
}

/// How many projects the menus list by name.
pub const PROJECTS_LISTED: usize = 9;

/// Switch to project `index` (from 0): one action for every project the menus list.
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = livtk, no_json)]
pub struct SwitchProject {
    pub index: usize,
}

/// The action that switches to project `i` (from 0), for the first `PROJECTS_LISTED`.
pub fn project_action(i: usize) -> Option<Box<dyn Action>> {
    (i < PROJECTS_LISTED).then(|| Box::new(SwitchProject { index: i }) as Box<dyn Action>)
}

/// The projects, by name, each switching to it, the open one ticked.
fn projects(state: &MenuState) -> Vec<MenuItem> {
    state
        .projects
        .iter()
        .take(PROJECTS_LISTED)
        .enumerate()
        .filter_map(|(i, name)| {
            project_action(i).map(|action| MenuItem::Action {
                name: name.clone().into(),
                action,
                os_action: None,
                checked: i == state.current,
                disabled: false,
            })
        })
        .collect()
}

/// The menu bar, as `state` has it.
pub fn menu_bar(state: &MenuState) -> Vec<Menu> {
    vec![
        Menu::new("livtk").items(
            [
                MenuItem::action("About livtk", About),
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::action("Check for Updates…", CheckForUpdates),
            ]
            .into_iter()
            .chain(state.update.map(|build| {
                MenuItem::action(
                    format!("Restart to Update (build {build})"),
                    RestartToUpdate,
                )
            }))
            .chain([
                MenuItem::separator(),
                MenuItem::action("Install Command Line Tool", InstallCli),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide livtk", HideApp),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit livtk", Quit),
            ]),
        ),
        Menu::new("File").items([
            MenuItem::action("New Task", NewTask),
            MenuItem::action("New Project", NewProject),
            MenuItem::action("Open Another Project…", OpenFolder),
            MenuItem::submenu(Menu::new("Projects").items(projects(state))),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", input::Undo, OsAction::Undo),
            MenuItem::os_action("Redo", input::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", input::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", input::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", input::Paste, OsAction::Paste),
            MenuItem::os_action("Select All", input::SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items(
            // Every view, in the sidebar's order, the one shown ticked.
            View::ALL
                .into_iter()
                .map(|view| item(view.title(), GoView { view }, state.view == Some(view)))
                .chain([
                    MenuItem::separator(),
                    item("Board", ToggleLayout, state.board),
                    item("Show Empty Groups", ToggleEmpty, state.show_empty),
                    item("Show Dismissed Work", ToggleAll, state.show_dismissed),
                    MenuItem::separator(),
                    MenuItem::action("Toggle Sidebar", ToggleSidebar),
                    MenuItem::action("Toggle Bottom Bar", ToggleBottom),
                    MenuItem::action("Toggle Detail Panel", TogglePanel),
                    MenuItem::action("Command Palette", OpenPalette),
                    MenuItem::separator(),
                    MenuItem::action("Zoom Graph In", ZoomIn),
                    MenuItem::action("Zoom Graph Out", ZoomOut),
                    MenuItem::action("Fit Graph", ZoomFit),
                ]),
        ),
        Menu::new("Task").items([
            MenuItem::action("Acknowledge, Ratify or Accept", Decide),
            MenuItem::action("Ratify as Standing Rule", Standing),
            MenuItem::action("Reject", Reject),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Priority").items([
                MenuItem::action("Urgent", Priority1),
                MenuItem::action("High", Priority2),
                MenuItem::action("Medium", Priority3),
                MenuItem::action("Low", Priority4),
            ])),
            MenuItem::separator(),
            MenuItem::action("Add Note", Note),
            MenuItem::action("Add Link…", AddLinkMenu),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", ZoomWindow),
            MenuItem::separator(),
            MenuItem::action("Bring All to Front", BringToFront),
        ]),
        Menu::new("Help").items([
            MenuItem::action("Keyboard Shortcuts", ToggleHelp),
            MenuItem::separator(),
            MenuItem::action("livtk on GitHub", OpenGitHub),
            MenuItem::action("Report an Issue", ReportIssue),
        ]),
    ]
}

/// The Dock's menu: a new task, the inbox with what waits in it, and the projects.
pub fn dock_menu(state: &MenuState) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::action("New Task", NewTask),
        MenuItem::action(
            if state.inbox == 0 {
                "Inbox".to_owned()
            } else {
                format!("Inbox ({})", state.inbox)
            },
            GoView { view: View::Inbox },
        ),
    ];
    let listed = projects(state);
    if listed.len() > 1 {
        items.push(MenuItem::separator());
        items.extend(listed);
    }
    items
}
