//! `liv`: Liv as a macOS app bundle, `Liv.app`, beside livtk.app and never in its place
//! (proposal 0003 §5).
//!
//!   cargo xtask liv                     build Liv.app in the target folder (the quick profile)
//!   cargo xtask liv --install           then move it to ~/Applications
//!   cargo xtask liv --release [NOTES]   from a release build, and publish it to Liv's own update
//!                                       channel (`~/Library/Application Support/liv/updates`)
//!
//! Its identifier, folders and channel are Liv's own (`crates/liv/src/identity.rs`), so building,
//! installing or releasing it never touches livtk.app.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use livtk_update::{Arch, FORMAT, Manifest, Platform};
use sha2::{Digest, Sha256};

/// Liv's identifier and folder name, as `liv::identity` has them; a test in `crates/liv` holds
/// that they are not livtk.app's.
const APP_ID: &str = "com.github.nebulaw.liv";
const DIR: &str = "liv";

pub fn run(args: &[String]) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("an app bundle is for macOS; elsewhere run `cargo run -p liv`");
    }
    let mut install = false;
    let mut release: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--install" => install = true,
            "--release" => release = Some(it.next().cloned().unwrap_or_default()),
            _ => bail!("usage: cargo xtask liv [--install] [--release [NOTES]]"),
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let target = target_dir(&root)?;
    let profile = if release.is_some() {
        "release"
    } else {
        "quick"
    };
    run_ok(Command::new("cargo").current_dir(&root).args([
        "build",
        "-q",
        "--locked",
        "--profile",
        profile,
        "-p",
        "liv",
    ]))?;
    let built = target.join(profile).join("liv");
    let identity = output(Command::new(&built).arg("--version"))?;
    let id = Identity::parse(&identity)?;

    let app = target.join("Liv.app");
    let _ = std::fs::remove_dir_all(&app);
    std::fs::create_dir_all(app.join("Contents/MacOS"))?;
    std::fs::create_dir_all(app.join("Contents/Resources"))?;
    std::fs::copy(&built, app.join("Contents/MacOS/liv"))?;
    std::fs::copy(
        root.join("assets/brand/livtk.icns"),
        app.join("Contents/Resources/liv.icns"),
    )?;
    std::fs::write(app.join("Contents/Info.plist"), id.plist())?;
    // Signed ad hoc, as macOS asks of every app on Apple silicon; nothing leaves this machine.
    run_ok(
        Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(&app),
    )?;
    println!("built {}: {identity}", app.display());

    if let Some(notes) = release {
        publish(&app, &id, notes)?;
    }
    if install {
        let to = PathBuf::from(std::env::var("HOME").context("a home folder")?)
            .join("Applications/Liv.app");
        let _ = std::fs::remove_dir_all(&to);
        std::fs::create_dir_all(to.parent().context("a parent")?)?;
        // Moved, not copied: two bundles with one identifier leave macOS to pick either.
        std::fs::rename(&app, &to)?;
        println!("installed {}", to.display());
    }
    Ok(())
}

/// The build's identity, from `liv --version`: "liv 0.4.0 (build 87, abc1234)", with
/// ", modified" when its sources had changes.
struct Identity {
    version: String,
    build: u64,
    commit: String,
    modified: bool,
}

impl Identity {
    fn parse(s: &str) -> Result<Identity> {
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
        })
    }

    fn plist(&self) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>{APP_ID}</string>
  <key>CFBundleName</key><string>Liv</string>
  <key>CFBundleDisplayName</key><string>Liv</string>
  <key>CFBundleExecutable</key><string>liv</string>
  <key>CFBundleIconFile</key><string>liv</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{}</string>
  <key>CFBundleVersion</key><string>{}</string>
  <key>LivtkCommit</key><string>{}</string>
  <key>LivtkModified</key><{}/>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#,
            self.version, self.build, self.commit, self.modified
        )
    }
}

/// Put `app` in Liv's update channel: the bundle zipped as macOS zips apps, and its manifest
/// beside it, last, so the channel never names an artifact that is not all there.
fn publish(app: &Path, id: &Identity, notes: String) -> Result<()> {
    if id.modified {
        eprintln!(
            "liv: the sources have uncommitted changes; build {} is HEAD's number",
            id.build
        );
    }
    let channel = std::env::var_os("LIV_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| livtk_update::paths::data_dir(DIR))
        .context("Liv's state folder")?
        .join("updates");
    std::fs::create_dir_all(&channel)?;
    let zip = format!("liv-{}.zip", id.build);
    let _ = std::fs::remove_file(channel.join(&zip));
    run_ok(
        Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(app)
            .arg(channel.join(&zip)),
    )?;
    let bytes = std::fs::read(channel.join(&zip))?;
    let manifest = Manifest {
        format: FORMAT,
        app: APP_ID.to_owned(),
        version: id.version.clone(),
        build: id.build,
        commit: id.commit.clone(),
        platform: Platform::this().context("a platform updates know")?,
        arch: Arch::this().context("an architecture updates know")?,
        artifact: zip,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        size: bytes.len() as u64,
        notes,
        signature: None,
    };
    let path = channel.join(format!("liv-{}.json", id.build));
    std::fs::write(&path, serde_json::to_string_pretty(&manifest)?)?;
    println!(
        "released Liv build {} ({}) to {}",
        id.build,
        id.commit,
        channel.display()
    );
    Ok(())
}

/// Where cargo builds, as cargo itself says, whatever sets it.
fn target_dir(root: &Path) -> Result<PathBuf> {
    let meta = output(Command::new("cargo").current_dir(root).args([
        "metadata",
        "--no-deps",
        "--format-version",
        "1",
    ]))?;
    let v: serde_json::Value = serde_json::from_str(&meta)?;
    Ok(PathBuf::from(
        v["target_directory"]
            .as_str()
            .context("cargo's target folder")?,
    ))
}

fn run_ok(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("running {cmd:?}"))?;
    if !status.success() {
        bail!("{cmd:?} failed: {status}");
    }
    Ok(())
}

fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    if !out.status.success() {
        bail!("{cmd:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_line_gives_the_build_its_commit_and_whether_it_was_modified() {
        let id = Identity::parse("liv 0.4.0 (build 87, abc1234)").unwrap();
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
        assert!(id.plist().contains(APP_ID));
    }
}
