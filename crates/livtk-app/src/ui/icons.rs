//! The app's icons: Boxicons v3, filled (see `assets/icons`). Each is named once here and compiled
//! in by its file; the rest of the folder costs nothing. GPUI draws an SVG only in a colour of its
//! own, so every icon is given one, and follows its row's hover through a group.

use gpui_kit::*;

use super::theme::color;

macro_rules! icons {
    ($($name:ident => $file:literal,)*) => {
        /// Every icon the app draws.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon {
            $($name,)*
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
    Project => "folder",
    OpenProject => "folder-plus",
    Shortcuts => "help-circle",
    NewTask => "pencil-sparkles",
    List => "list-ul",
    Board => "columns-4",
    Filter => "menu-filter",
    Display => "slider-alt",
    Close => "x",
    Search => "search",
    CaretDown => "caret-big-down",
    CaretRight => "caret-big-right",
    Check => "check",
    Grouping => "blocks",
    Ordering => "arrow-down-up",
    Show => "eye",
    Hide => "eye-slash",
    ClosePanel => "dock-right-arrow",
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
