//! How the platform names the app's keys: Command on macOS where other platforms use Control,
//! and macOS's own signs. GPUI binds the keys as `secondary-…`, which means the same; these are
//! the labels the shortcuts modal, the tooltips and the palette show, so they always match.

const MAC: bool = cfg!(target_os = "macos");

/// The key that holds the app's shortcuts down: Command on macOS, Control elsewhere.
pub const MOD: &str = if MAC { "⌘" } else { "Ctrl" };
pub const SHIFT: &str = if MAC { "⇧" } else { "Shift" };

/// Shortcuts as one label: the palette, the shortcuts, the sidebar, a form's submit.
pub const PALETTE: &str = if MAC { "⌘ K" } else { "Ctrl K" };
pub const HELP: &str = if MAC { "⌘ ?" } else { "Ctrl ?" };
pub const SIDEBAR: &str = if MAC { "⌘ B" } else { "Ctrl B" };
pub const SUBMIT: &str = if MAC { "⌘ ↵" } else { "Ctrl ↵" };
/// Ratify as a standing rule.
pub const STANDING: &str = if MAC { "⇧ A" } else { "Shift A" };
/// Show or hide empty groups.
pub const EMPTY_GROUPS: &str = "S E";
/// Show or hide dismissed work.
pub const DISMISSED: &str = "S D";
