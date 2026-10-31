//! Building Liv's app bundle: the binary built here, locked to Cargo.lock; `cargo bundle` making
//! the bundle from `[package.metadata.bundle]` in `crates/liv` (no Info.plist or file copying of
//! ours); then what cargo-bundle leaves to us: the build number the updater checks, a check that
//! the updater would take the bundle, and the ad-hoc signature (until Developer ID signing,
//! TAS-143). Liv's identity and channel are read from the built binary, so they are written in
//! one place.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::{output, run_ok};

/// A bundle built.
pub struct Built {
    pub app: PathBuf,
    pub id: Identity,
    /// What `liv --version` said.
    pub describe: String,
}

/// Build Liv with `profile` and the build-time variables `env`, and bundle it.
pub fn bundle(root: &Path, target: &Path, profile: &str, env: &[(&str, &str)]) -> Result<Built> {
    let mut build = Command::new("cargo");
    build
        .current_dir(root)
        .args(["build", "-q", "--locked", "--profile", profile, "-p", "liv"]);
    for (k, v) in env {
        build.env(k, v);
    }
    run_ok(&mut build)?;
    let built = target.join(profile).join("liv");
    let describe = output(Command::new(&built).arg("--version"))?;
    let mut id = Identity::parse(&describe)?;
    id.who(&output(Command::new(&built).arg("--identity"))?)?;
    // From the crate's folder: cargo-bundle reads the metadata's paths (the icon) from where it
    // runs, and skips what it can't find without a word.
    run_ok(
        Command::new("cargo")
            .current_dir(root.join("crates/liv"))
            .args(["bundle", "--package", "liv", "--profile", profile])
            .args(["--format", "osx", "--binary-path"])
            .arg(&built),
    )?;
    let app = target.join(profile).join("bundle/osx/Liv.app");
    let plist = app.join("Contents/Info.plist");
    if !std::fs::read_to_string(&plist)?.contains("<key>CFBundleIconFile</key>") {
        bail!(
            "{} has no icon: check `icon` in crates/liv's bundle metadata",
            app.display()
        );
    }
    // cargo-bundle stamps CFBundleVersion with the time; the updater orders and checks builds
    // by their number, so the bundle carries the binary's.
    run_ok(
        Command::new("plutil")
            .args(["-replace", "CFBundleVersion", "-string"])
            .arg(id.build.to_string())
            .arg(&plist),
    )?;
    // The bundle as the updater will read it: what it would refuse is never signed or published.
    let (app_id, build) = livtk_update::stage::identity(&app)?;
    if app_id != id.app_id || build != id.build {
        bail!(
            "the bundle says {app_id} build {build}, the binary {} build {}",
            id.app_id,
            id.build
        );
    }
    // Signed ad hoc, as macOS asks of every app on Apple silicon; nothing leaves this machine.
    run_ok(
        Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(&app),
    )?;
    Ok(Built { app, id, describe })
}

/// The build's identity, from `liv --version`: "liv 0.4.0 (build 87, abc1234)", with
/// ", modified" when its sources had changes; and from `liv --identity`, who Liv is.
pub struct Identity {
    pub version: String,
    pub build: u64,
    pub commit: String,
    pub modified: bool,
    pub app_id: String,
    pub name: String,
    /// Its local update channel.
    pub updates: Option<PathBuf>,
}

impl Identity {
    pub fn parse(s: &str) -> Result<Identity> {
        let rest = s
            .strip_prefix("liv ")
            .context("`liv --version` names Liv")?;
        let (version, paren) = rest.split_once(" (").context("a build in parentheses")?;
        let inner = paren.trim_end_matches(')');
        let mut parts = inner.split(", ");
        let build = parts
            .next()
            .and_then(|b| b.strip_prefix("build "))
            .and_then(|b| b.parse().ok())
            .context("a build number")?;
        let commit = parts.next().context("a commit")?.to_owned();
        Ok(Identity {
            version: version.to_owned(),
            build,
            commit,
            modified: parts.any(|p| p == "modified"),
            app_id: String::new(),
            name: String::new(),
            updates: None,
        })
    }

    /// Take who Liv is from `liv --identity`: `key=value`, a line each.
    pub fn who(&mut self, said: &str) -> Result<()> {
        for line in said.lines() {
            match line.split_once('=') {
                Some(("app_id", v)) => self.app_id = v.to_owned(),
                Some(("name", v)) => self.name = v.to_owned(),
                Some(("updates", v)) => self.updates = Some(PathBuf::from(v)),
                _ => {}
            }
        }
        if self.app_id.is_empty() || self.name.is_empty() {
            bail!("`liv --identity` names no app id or name: {said}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_line_gives_the_build_its_commit_and_whether_it_was_modified() {
        let mut id = Identity::parse("liv 0.4.0 (build 87, abc1234)").unwrap();
        assert_eq!(
            (id.version.as_str(), id.build, id.commit.as_str()),
            ("0.4.0", 87, "abc1234")
        );
        assert!(!id.modified);
        assert!(
            Identity::parse("liv 0.4.0 (build 88, def5678, modified)")
                .unwrap()
                .modified
        );
        assert!(
            Identity::parse("livtk-app 0.4.0 (build 1, a)").is_err(),
            "only Liv's"
        );

        // Who it is, and its channel, come from the binary.
        assert!(id.who("updates=/x").is_err(), "an id and a name are needed");
        id.who("app_id=com.example.liv\nname=Liv\nupdates=/s/liv/updates")
            .unwrap();
        assert_eq!(id.app_id, "com.example.liv");
        assert_eq!(id.updates.as_deref(), Some(Path::new("/s/liv/updates")));
    }
}
