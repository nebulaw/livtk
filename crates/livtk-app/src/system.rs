//! What the system asks of every app that GPUI does not pass on. GPU-free.

use std::process::Command;

/// Whether the human asked the system for less motion: Reduce Motion on macOS (Accessibility,
/// Display), animations turned off on a GNOME desktop. Read from the system's own settings,
/// which costs a process, so asked when the app starts and when its window comes forward, not
/// every frame.
pub fn reduce_motion() -> bool {
    if cfg!(target_os = "macos") {
        read(
            "defaults",
            &["read", "com.apple.universalaccess", "reduceMotion"],
        )
        .is_some_and(|v| v == "1")
    } else if cfg!(target_os = "linux") {
        read(
            "gsettings",
            &["get", "org.gnome.desktop.interface", "enable-animations"],
        )
        .is_some_and(|v| v == "false")
    } else {
        false
    }
}

/// What `program args` prints, trimmed, when it runs and succeeds.
fn read(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
