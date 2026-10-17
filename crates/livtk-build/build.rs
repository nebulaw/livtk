//! The build's identity, from git: the number of commits on HEAD (later builds of main have
//! larger numbers), its short hash, and whether what the binaries are built from had changes.
//! Outside a git checkout: build 0, commit "unknown". In a shallow clone the count is wrong (it
//! counts only what was fetched), so the build is 0 there too, with a warning: it must never sort
//! above a real build. Written as constants into OUT_DIR.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    // Reading never takes git's locks or rewrites its index (status would), so it never makes
    // itself run again.
    let out = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    let here = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = git(&here, &["rev-parse", "--show-toplevel"]).map(PathBuf::from);
    let mut build = 0;
    let mut commit = "unknown".to_owned();
    let mut modified = false;
    if let Some(root) = &root {
        commit = git(root, &["rev-parse", "--short=7", "HEAD"]).unwrap_or(commit);
        if git(root, &["rev-parse", "--is-shallow-repository"]).as_deref() == Some("true") {
            println!(
                "cargo::warning=a shallow clone counts only the commits it fetched: build 0 \
                 (git fetch --unshallow gives the build its number)"
            );
        } else {
            build = git(root, &["rev-list", "--count", "HEAD"])
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0);
        }
        // What the binaries are built from, tracked files only: not the task log beside them,
        // which changes all the time, nor files git does not know (Finder's .DS_Store).
        modified = git(
            root,
            &[
                "status",
                "--porcelain",
                "--untracked-files=no",
                "--",
                "crates",
                "scripts",
                "Cargo.toml",
                "Cargo.lock",
            ],
        )
        .is_some_and(|s| !s.is_empty());
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let describe = format!(
        "{version} (build {build}, {commit}{})",
        if modified { ", modified" } else { "" }
    );
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("identity.rs");
    let code = format!(
        "pub const BUILD: u64 = {build};\n\
         pub const COMMIT: &str = {commit:?};\n\
         pub const MODIFIED: bool = {modified};\n\
         pub const DESCRIBE: &str = {describe:?};\n"
    );
    if let Err(e) = std::fs::write(&out, code) {
        println!("cargo::error=cannot write {}: {e}", out.display());
    }

    // Again when a commit is made or checked out, a shallow clone is deepened, or a source
    // changes. Only paths that exist are watched (cargo runs this again on every build while a
    // watched path is missing, so `shallow` going away is seen too). A worktree keeps its HEAD
    // and index in its own git directory, and the refs and `shallow` in the common one.
    if let Some(root) = &root {
        let dir = |arg: &str| git(root, &["rev-parse", arg]).map(PathBuf::from);
        let own = dir("--absolute-git-dir");
        let common =
            dir("--git-common-dir").map(|d| if d.is_absolute() { d } else { root.join(d) });
        let watched = own
            .iter()
            .flat_map(|d| [d.join("HEAD"), d.join("index")])
            .chain(
                common
                    .iter()
                    .flat_map(|d| [d.join("packed-refs"), d.join("refs"), d.join("shallow")]),
            )
            .chain(["crates", "scripts", "Cargo.toml", "Cargo.lock"].map(|p| root.join(p)));
        for path in watched.filter(|p| p.exists()) {
            println!("cargo::rerun-if-changed={}", path.display());
        }
    }
}
