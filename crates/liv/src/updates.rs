//! Liv updating itself (proposal 0003, TAS-139): finding a newer build on its channel, checking
//! its signature, size and hash, staging it, and, when asked, handing over to the staged build to
//! finish: it waits for this Liv to quit, swaps itself in, starts, and rolls back if it doesn't
//! start well. Everything here is plain IO, run off the window's thread; the UI only asks.
//!
//! Every channel is signed, and Liv trusts only the keys built into it, each on its own channel
//! ([`keys_for`]):
//! - [`RELEASE_KEY`]: the key releases on GitHub are signed with (TAS-140): stable and preview;
//! - `LIV_DEV_KEY`, built in by `cargo xtask liv` on the machine it builds on: only the local
//!   channel, where that machine's own builds are published.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use livtk_update::github::{GitHubReleases, Track};
use livtk_update::sign::Keys;
use livtk_update::source::{LocalFolder, Source, newer};
use livtk_update::stage::{Staged, stage};
use livtk_update::swap::{Launcher, Open, Outcome, Swap, exited, swap};

use crate::identity::{self, Folders};
use crate::settings::{Channel, Settings};

/// The public key Liv's releases on GitHub are signed with. Empty until the release key exists
/// (TAS-140): until then, nothing from GitHub is trusted.
pub const RELEASE_KEY: &str = "";

/// The variable a Liv started by an update reads to know where to say it started.
pub const HEALTH_VAR: &str = "LIV_UPDATE_HEALTH";

/// How often Liv looks for an update while it runs, beside once at start: no event says a
/// release was made, so this one timer is kept (DEC-8).
pub const EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// The keys this build trusts on `channel`: GitHub's channels the release key only; the local
/// channel this machine's own key too.
pub fn keys_for(channel: Channel) -> Keys {
    let dev = match channel {
        Channel::Local => option_env!("LIV_DEV_KEY").unwrap_or(""),
        Channel::Stable | Channel::Preview => "",
    };
    Keys::new([RELEASE_KEY, dev]).unwrap_or_else(|e| {
        log::error!("a built-in update key is not one: {e}");
        Keys::default()
    })
}

/// The channel a build follows when the settings don't say: local for builds made by
/// `cargo xtask liv` (so the builds it releases here are found), stable otherwise.
pub fn default_channel() -> Channel {
    option_env!("LIV_DEFAULT_CHANNEL")
        .and_then(Channel::parse)
        .unwrap_or(Channel::Stable)
}

/// The channel to follow: the settings', else the build's default.
pub fn channel(settings: &Settings) -> Channel {
    settings.updates.channel.unwrap_or_else(default_channel)
}

/// The repository releases come from: the workspace's (`repository` in Cargo.toml).
pub fn repo() -> &'static str {
    env!("CARGO_PKG_REPOSITORY")
        .trim_start_matches("https://github.com/")
        .trim_end_matches('/')
}

/// The HTTPS client updates are read with, named as GitHub asks: `Liv/<build>`.
pub fn http() -> livtk_update::fetch::Http {
    livtk_update::fetch::Http::new(format!("{}/{}", identity::NAME, livtk_build::DESCRIBE))
}

/// Where `channel`'s builds come from.
pub fn source(channel: Channel, folders: &Folders, keys: Keys) -> Box<dyn Source> {
    match channel {
        Channel::Local => Box::new(LocalFolder::trusting(&folders.updates, keys)),
        Channel::Stable | Channel::Preview => {
            let track = if channel == Channel::Preview {
                Track::Preview
            } else {
                Track::Stable
            };
            Box::new(GitHubReleases::new(
                repo(),
                track,
                identity::SHORT,
                keys,
                Box::new(http()),
            ))
        }
    }
}

/// Look for a build newer than `this_build` in `source`, and stage it: `None` when this is the
/// newest. A build already staged and `ready` is kept as it is, never staged again over itself
/// (Restart to Update may be handing it over). Slow (it downloads, hashes and unpacks): run it off
/// the window's thread.
pub fn check(
    source: &dyn Source,
    fetch: &dyn livtk_update::fetch::Fetch,
    folders: &Folders,
    this_build: u64,
    ready: Option<&Staged>,
) -> livtk_update::Result<Option<Staged>> {
    let Some(found) = newer(source, identity::APP_ID, this_build)? else {
        return Ok(None);
    };
    if let Some(r) = ready.filter(|r| r.manifest.build == found.manifest.build) {
        return Ok(Some(r.clone()));
    }
    let here = livtk_update::fetch::download(&found, fetch, &folders.cache.join("downloads"))?;
    stage(&here, &folders.cache.join("staging")).map(Some)
}

/// The app bundle program `exe` runs from: `…/Liv.app` for `…/Liv.app/Contents/MacOS/liv`.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app")
        .then(|| app.to_path_buf())
}

/// What a Liv started by an update shows once: the build and its notes, written by the Liv that
/// handed over, read and removed by the new one.
fn notes_file(folders: &Folders) -> PathBuf {
    folders.state.join("updated.txt")
}

/// The file a freshly installed Liv writes its build number to once it has started.
fn health_file(folders: &Folders) -> PathBuf {
    folders.state.join("healthy")
}

/// Hand over to `staged`: its own program finishes the update once this Liv (running from the
/// bundle of `exe`) has quit, which the caller then does. Its notes are kept for it to show.
pub fn install(staged: &Staged, exe: &Path, folders: &Folders) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("installing updates here comes with Liv's installers (TAS-143)".into());
    }
    let installed =
        bundle_of(exe).ok_or("updates install into Liv.app: this Liv runs outside it")?;
    let program = staged.app.join("Contents/MacOS").join(identity::SHORT);
    std::fs::create_dir_all(&folders.state).map_err(|e| e.to_string())?;
    let note = format!(
        "{}\n{}",
        staged.manifest.build,
        staged.manifest.notes.trim()
    );
    std::fs::write(notes_file(folders), note).map_err(|e| e.to_string())?;
    Command::new(program)
        .args(
            Finish {
                pid: std::process::id(),
                installed,
                staged: staged.app.clone(),
                health: health_file(folders),
                build: staged.manifest.build,
            }
            .args(),
        )
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            format!(
                "could not hand over to build {}: {e}",
                staged.manifest.build
            )
        })
}

/// How a staged Liv is asked to finish an update: `liv --finish-update --pid … --installed …
/// --staged … --health … --build …`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finish {
    pub pid: u32,
    pub installed: PathBuf,
    pub staged: PathBuf,
    pub health: PathBuf,
    pub build: u64,
}

pub const FINISH: &str = "--finish-update";

impl Finish {
    pub fn args(&self) -> Vec<std::ffi::OsString> {
        let mut a: Vec<std::ffi::OsString> = vec![FINISH.into()];
        for (k, v) in [
            ("--pid", self.pid.to_string().into()),
            ("--installed", self.installed.clone().into_os_string()),
            ("--staged", self.staged.clone().into_os_string()),
            ("--health", self.health.clone().into_os_string()),
            ("--build", self.build.to_string().into()),
        ] {
            a.push(k.into());
            a.push(v);
        }
        a
    }

    /// The request in `args` (after the program's name), if it is one.
    pub fn parse(args: &[std::ffi::OsString]) -> Option<Finish> {
        if args.first()? != FINISH {
            return None;
        }
        let value = |name: &str| {
            args.iter()
                .position(|a| a == name)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        Some(Finish {
            pid: value("--pid")?.to_str()?.parse().ok()?,
            installed: value("--installed")?.into(),
            staged: value("--staged")?.into(),
            health: value("--health")?.into(),
            build: value("--build")?.to_str()?.parse().ok()?,
        })
    }

    /// Finish it: wait for the old Liv to quit, check the staged build is the one asked for,
    /// swap it in, start it, and roll back if it doesn't say it started well. Whatever fails once
    /// the old Liv has quit, a Liv is left running. Runs without a window.
    pub fn run(&self) -> Result<Outcome, String> {
        self.run_with(&mut Open {
            program: identity::SHORT.into(),
            health_var: HEALTH_VAR.into(),
        })
    }

    /// [`Finish::run`], starting apps with `launcher`.
    pub fn run_with(&self, launcher: &mut dyn Launcher) -> Result<Outcome, String> {
        if !exited(self.pid, Duration::from_secs(30)) {
            // It never quit: it runs as it was, and nothing changed.
            return Err(format!(
                "Liv (pid {}) is still running; nothing changed",
                self.pid
            ));
        }
        match livtk_update::stage::identity(&self.staged) {
            Ok((id, build)) if id == identity::APP_ID && build == self.build => {}
            found => {
                let _ = launcher.start(&self.installed, None);
                return Err(format!(
                    "the staged app isn't Liv build {}: {found:?}; the installed Liv is started",
                    self.build
                ));
            }
        }
        swap(
            &Swap {
                installed: self.installed.clone(),
                staged: self.staged.clone(),
                health: self.health.clone(),
                build: self.build,
                wait: Duration::from_secs(20),
            },
            launcher,
        )
        .map_err(|e| e.to_string())
    }
}

/// Started by an update: say so where it asked, so the update keeps this build. Whether this start
/// followed an update.
pub fn report_started(build: u64) -> bool {
    let Some(path) = std::env::var_os(HEALTH_VAR).map(PathBuf::from) else {
        return false;
    };
    std::fs::write(path, build.to_string()).is_ok()
}

/// What build `build` was updated with, to show once: its notes (maybe empty) when this start
/// followed an update to it. The file is removed, so they show once.
pub fn notes_after_update(folders: &Folders, build: u64) -> Option<String> {
    let path = notes_file(folders);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let (first, notes) = text.split_once('\n').unwrap_or((&text, ""));
    (first.trim().parse::<u64>().ok()? == build).then(|| notes.trim().to_owned())
}
