//! `livtk-app [DIR...]`: open the given livtk projects, or every project livtk knows. On macOS
//! `livtk-app` on PATH is a link into `livtk.app`, which alone may post notifications: started
//! through it, this opens the bundle. `--version` says which build this is; `--install-cli` links
//! `livtk` and `livtk-app` onto PATH.

use std::path::PathBuf;

use livtk_app::launch::{self, Start};
use livtk_app::model::Quiet;
use livtk_core::registry;

fn main() {
    // As raw bytes: an argument need not be UTF-8 (a path), and must never crash the app.
    let first = std::env::args_os().nth(1);
    match first.as_ref().and_then(|a| a.to_str()) {
        Some("--version") => {
            println!("livtk-app {}", livtk_build::DESCRIBE);
            return;
        }
        Some("--install-cli") => {
            std::process::exit(install_cli());
        }
        _ => {}
    }
    livtk_app::applog::init();
    let config = registry::config_dir();
    // The projects asked for, as absolute roots: what a handoff passes on.
    let mut asked: Vec<PathBuf> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for arg in std::env::args_os().skip(1).map(PathBuf::from) {
        // Remember what was opened by hand, as its project root, so it is listed next time.
        match livtk_core::find_dir(&arg) {
            Ok(livtk) => {
                let root = livtk.parent().map(PathBuf::from).unwrap_or(arg);
                if let Some(c) = &config {
                    let _ = registry::add(c, &root);
                }
                asked.push(root.clone());
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
    // Where it was started: this shell's folder, or the one a handoff passed on, since the
    // bundle itself starts elsewhere.
    let started_in = std::env::var_os(launch::STARTED_IN)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    if dirs.is_empty()
        && let Some(here) = &started_in
        && livtk_core::find_dir(here).is_ok()
    {
        dirs.push(here.clone());
    }

    // Where notifications can be: on macOS only in the bundle. Started through a link into one,
    // this opens it and steps aside.
    let mut quiet = None;
    if cfg!(target_os = "macos")
        && let Ok(invoked) = std::env::current_exe()
    {
        let resolved = std::fs::canonicalize(&invoked).unwrap_or_else(|_| invoked.clone());
        match launch::start(&invoked, &resolved) {
            Start::Open(bundle) => {
                if !asked.is_empty() && launch::bundle_running() {
                    eprintln!(
                        "livtk-app: livtk.app is running, so it only comes to the front; the \
                         projects given are registered and show in it after a restart"
                    );
                }
                let env = launch::passed_on(std::env::vars_os());
                if launch::open_bundle(&bundle, &asked, started_in.as_deref(), &env) {
                    log::info!("opened {}", bundle.display());
                    return;
                }
                eprintln!(
                    "livtk-app: cannot open {}: running here, without notifications",
                    bundle.display()
                );
                quiet = Some(Quiet::OpenFailed);
            }
            Start::Bundled => {}
            Start::Bare => quiet = Some(Quiet::NoBundle),
        }
    }
    log::info!(
        "livtk-app {} from {:?}, {} projects, {}",
        livtk_build::DESCRIBE,
        std::env::current_exe().ok(),
        dirs.len(),
        match quiet {
            None => "notifications where the system allows them".to_owned(),
            Some(q) => format!("no notifications: {q:?}"),
        }
    );
    livtk_app::ui::run(dirs, quiet);
}

/// `livtk-app --install-cli`: link the bundle's `livtk` and `livtk-app` into ~/.local/bin, from
/// the bundle this runs in, else the installed one. The exit status.
fn install_cli() -> i32 {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let bundle = std::env::current_exe()
        .ok()
        .and_then(|e| std::fs::canonicalize(e).ok())
        .and_then(|e| launch::bundle_of(&e))
        .or_else(|| home.as_ref().map(|h| h.join(launch::IN_HOME)))
        .filter(|b| b.exists());
    let (Some(bundle), Some(home)) = (bundle, home) else {
        eprintln!(
            "livtk-app: no livtk.app to link to: install it with scripts/bundle-app --install"
        );
        return 1;
    };
    let bin = home.join(launch::BIN_IN_HOME);
    let path = std::env::var_os("PATH").unwrap_or_default();
    match launch::install_cli(&bundle, &bin, &path) {
        Ok(done) => {
            for link in &done.linked {
                println!("linked {} -> {}", link.display(), bundle.display());
            }
            for file in &done.refused {
                println!(
                    "left {}: a file, not a link; move it away and run this again",
                    file.display()
                );
            }
            if !done.on_path {
                println!(
                    "{} is not on PATH: add it in your shell's profile, e.g. export PATH=\"$HOME/{}:$PATH\"",
                    bin.display(),
                    launch::BIN_IN_HOME
                );
            }
            for other in &done.shadowed {
                println!(
                    "{} comes earlier on PATH and runs instead: remove it (cargo uninstall livtk-cli livtk-app removes cargo's copies)",
                    other.display()
                );
            }
            for other in &done.leftover {
                println!(
                    "{} is left over from an earlier install and never runs: remove it (cargo uninstall livtk-cli livtk-app removes cargo's copies)",
                    other.display()
                );
            }
            i32::from(!done.refused.is_empty())
        }
        Err(e) => {
            eprintln!("livtk-app: cannot link the command line tools: {e}");
            1
        }
    }
}
