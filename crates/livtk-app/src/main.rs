//! `livtk-app [DIR...]`: open the given livtk projects, or every project livtk knows.

use std::path::PathBuf;

use livtk_core::registry;

fn main() {
    let config = registry::config_dir();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for arg in std::env::args_os().skip(1).map(PathBuf::from) {
        // Remember what was opened by hand, as its project root, so it is listed next time.
        match livtk_core::find_dir(&arg) {
            Ok(livtk) => {
                let root = livtk.parent().map(PathBuf::from).unwrap_or(arg);
                if let Some(c) = &config {
                    let _ = registry::add(c, &root);
                }
                dirs.push(root);
            }
            Err(e) => eprintln!("livtk-app: {}: {e}", arg.display()),
        }
    }
    if let Some(c) = &config {
        match registry::projects(c) {
            Ok(known) => dirs.extend(known),
            Err(e) => eprintln!("livtk-app: {e}"),
        }
    }
    if dirs.is_empty()
        && let Ok(here) = std::env::current_dir()
        && livtk_core::find_dir(&here).is_ok()
    {
        dirs.push(here);
    }
    livtk_app::ui::run(dirs);
}
