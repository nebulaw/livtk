//! Which build of livtk this is: its version, its build number (commits on HEAD, so a later build
//! of main has a larger one), its commit, and whether its sources had changes. Updates are ordered
//! by the build number; people read `DESCRIBE`, as `--version` and About print it.

include!(concat!(env!("OUT_DIR"), "/identity.rs"));

/// The workspace's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(args: &[&str]) -> Option<String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(env!("CARGO_MANIFEST_DIR"))
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// Built in a full checkout, as these tests are: the number is git's count of commits, the
    /// commit HEAD's, so a later commit has a larger number.
    #[test]
    fn a_build_in_a_checkout_knows_its_number_and_commit() {
        if git(&["rev-parse", "--is-shallow-repository"]).as_deref() != Some("false") {
            return; // Not a full checkout: nothing to compare with.
        }
        let count: u64 = git(&["rev-list", "--count", "HEAD"])
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            BUILD == count || BUILD + 1 == count,
            "the build ({BUILD}) is HEAD's count ({count}), or one behind if a commit came since"
        );
        assert_eq!(COMMIT.len(), 7);
        assert!(COMMIT.chars().all(|c| c.is_ascii_hexdigit()), "{COMMIT}");
        assert!(DESCRIBE.starts_with(&format!("{VERSION} (build {BUILD}, {COMMIT}")));
        assert_eq!(DESCRIBE.ends_with(", modified)"), MODIFIED);
    }
}
