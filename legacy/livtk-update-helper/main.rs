//! `livtk-update`: the small program inside the app that installs a staged update once the app
//! has quit. Its command line never changes:
//!
//! ```text
//! livtk-update --pid <app's pid> --installed <app> --staged <staged app> \
//!              --health <file> --build <number> [--wait <seconds>]
//! ```
//!
//! It waits for the app to exit, swaps the staged build in, starts it and waits for it to write
//! its build number to the health file; if it does not, the old app comes back. It has no
//! network code, and never will: finding and checking updates is the app's.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use livtk_update::swap::{Open, Outcome, Swap, exited, swap};

/// The variable a started app reads to know where to say it started.
const HEALTH_VAR: &str = "LIVTK_UPDATE_HEALTH";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let (Some(pid), Some(installed), Some(staged), Some(health), Some(build)) = (
        arg("--pid").and_then(|p| p.parse::<u32>().ok()),
        arg("--installed").map(PathBuf::from),
        arg("--staged").map(PathBuf::from),
        arg("--health").map(PathBuf::from),
        arg("--build").and_then(|b| b.parse::<u64>().ok()),
    ) else {
        eprintln!(
            "usage: livtk-update --pid N --installed APP --staged APP --health FILE --build N [--wait SECONDS]"
        );
        return ExitCode::from(2);
    };
    let wait = Duration::from_secs(arg("--wait").and_then(|w| w.parse().ok()).unwrap_or(20));
    if !exited(pid, Duration::from_secs(30)) {
        eprintln!("livtk-update: the app (pid {pid}) is still running; nothing changed");
        return ExitCode::FAILURE;
    }
    let s = Swap {
        installed,
        staged,
        health: health.clone(),
        build,
        wait,
    };
    let mut launcher = Open {
        program: "livtk-app".into(),
        health_var: HEALTH_VAR.into(),
    };
    match swap(&s, &mut launcher) {
        Ok(Outcome::Updated) => {
            eprintln!("livtk-update: updated to build {build}");
            ExitCode::SUCCESS
        }
        Ok(Outcome::RolledBack) => {
            eprintln!("livtk-update: build {build} did not start; the previous build is back");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("livtk-update: {e}");
            ExitCode::FAILURE
        }
    }
}
