//! The app's icons: Boxicons v3, filled (see `assets/icons`). Each is named once here and compiled
//! in by its file; the rest of the folder costs nothing. GPUI draws an SVG only in a colour of its
//! own, so every icon is given one, and follows its row's hover through a group.

use gpui_kit::*;

use super::theme::color;

macro_rules! icons {
    ($($(#[$doc:meta])* $name:ident => $file:literal,)*) => {
        /// Every icon the app draws.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon {
            $($(#[$doc])* $name,)*
        }

        impl Icon {
            pub const ALL: &[Icon] = &[$(Icon::$name,)*];

            /// The file it comes from, without `bx-` and `.svg`.
            pub const fn file(self) -> &'static str {
                match self {
                    $(Icon::$name => $file,)*
                }
            }

            pub fn svg(self) -> &'static [u8] {
                match self {
                    $(Icon::$name => include_bytes!(concat!("../../assets/icons/bx-", $file, ".svg")),)*
                }
            }
        }
    };
}

icons! {
    Inbox => "inbox",
    Activity => "bolt-alt",
    Tasks => "task",
    Lessons => "cognition",
    Decisions => "route",
    Findings => "medical-flask",
    Graph => "circuit-board",
    /// The project's docs.
    Docs => "book",
    Terminal => "terminal",
    /// A project, and a shut folder in the docs' tree; an open one.
    Project => "folder",
    FolderOpen => "folder-open",
    /// A doc's outline, On this page.
    Outline => "list",
    /// Copying a code block.
    Copy => "copy",
    /// The find bar's way to the match before, and the next.
    ChevronUp => "chevron-up",
    ChevronDown => "chevron-down",
    OpenProject => "folder-plus",
    Shortcuts => "help-circle",
    NewTask => "pencil-sparkles",
    List => "list-ul",
    Board => "columns-4",
    Filter => "menu-filter",
    Search => "search",
    CaretDown => "caret-big-down",
    CaretRight => "caret-big-right",
    Check => "check",
    Grouping => "blocks",
    Ordering => "arrow-down-up",
    /// How a list is shown: the dock's View options.
    ViewOptions => "slider-alt",
    Show => "eye",
    Hide => "eye-slash",
    /// Closing the detail panel.
    DockRight => "dock-right-arrow",
    /// The top bar's toggles: the sidebar, the bottom bar, the detail panel.
    PanelLeft => "dock-left",
    PanelBottom => "dock-bottom",
    PanelRight => "dock-right",
    /// The handle on an edge that resizes the sidebar or the panel.
    Resize => "move-horizontal",
    Link => "link",
    Unlink => "unlink",
    Notes => "message-circle-dots-2",
    Edit => "scribble",
    Standing => "bookmark-alt",
    Blocked => "brick",
    Claude => "sparkles-alt",
    You => "user",
    ZoomOut => "minus",
    Fit => "maximize",
    ZoomIn => "plus",
    Enter => "enter",
    Error => "alert-circle",
    /// The state flow's arrows, lit along the moves a node can make.
    ArrowRight => "arrow-right-stroke",
    ArrowLeft => "arrow-left-stroke",
    /// A notice that tells rather than warns.
    Info => "info-circle",
    /// Closing a notice.
    Close => "x",
    /// A sub-task, under its parent.
    SubTask => "turn-right",
    /// The notifications' marks: an error, a warning, an update ready, anything else told.
    Bug => "bug",
    Bell => "bell",
    Meteor => "meteor",
    Message => "message-circle-notification",
}

/// Icon `i`, `size` across, in colour `c`.
pub fn icon(i: Icon, size: f32, c: u32) -> Svg {
    svg()
        .data(i.svg())
        .size(px(size))
        .flex_none()
        .text_color(color(c))
        .debug_selector(move || format!("icon-{}", i.file()))
}

/// Icon `i` that takes colour `hover` while the pointer is over the nearest ancestor in
/// `group`, as that ancestor's own text does.
pub fn icon_hover(i: Icon, size: f32, c: u32, group: &'static str, hover: u32) -> Svg {
    icon(i, size, c).group_hover(group, move |s| s.text_color(color(hover)))
}
