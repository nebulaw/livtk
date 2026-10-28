//! Liv lives beside the app in use (`livtk.app`) and never touches it: nothing of Liv's identity
//! or folders is livtk.app's.

use std::path::Path;

use liv::identity;

/// livtk.app's identifier and folder name, as it ships today.
const LIVTK_ID: &str = "com.github.nebulaw.livtk";
const LIVTK_DIR: &str = "livtk";

fn named(p: &Path, name: &str) -> bool {
    p.components().any(|c| c.as_os_str() == name)
}

#[test]
fn liv_has_its_own_identifier_and_name() {
    assert_ne!(identity::APP_ID, LIVTK_ID);
    assert_ne!(identity::NAME, "livtk");
}

#[test]
fn every_folder_liv_writes_is_its_own() {
    let folders = [
        identity::config_dir(),
        identity::state_dir(),
        identity::updates_dir(),
        identity::log_file().and_then(|f| f.parent().map(Path::to_path_buf)),
    ];
    for f in folders {
        let f = f.expect("a home folder in tests");
        assert!(named(&f, identity::DIR), "{} is Liv's", f.display());
        assert!(!named(&f, LIVTK_DIR), "{} is not livtk.app's", f.display());
    }
    // And its update channel is not where livtk.app's releases go.
    assert_ne!(
        identity::updates_dir(),
        livtk_update::paths::source(),
        "its own update channel"
    );
}
