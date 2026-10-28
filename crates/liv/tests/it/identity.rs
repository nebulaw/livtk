//! Liv lives beside the app in use (`livtk.app`) and never touches it: nothing of Liv's identity
//! or folders is livtk.app's. Worked out in a fixed environment, never the machine's, and compared
//! with livtk.app's own folder functions where they can be.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use liv::identity::{self, Folders};

/// An environment holding only `vars`.
fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<PathBuf> {
    let map: HashMap<String, PathBuf> = vars
        .iter()
        .map(|(k, v)| ((*k).to_owned(), PathBuf::from(v)))
        .collect();
    move |k| map.get(k).cloned()
}

/// Whether `p` is `of` or inside it.
fn within(p: &Path, of: &Path) -> bool {
    p.starts_with(of)
}

#[test]
fn liv_has_its_own_identifier_and_name() {
    assert_ne!(identity::APP_ID, "com.github.nebulaw.livtk");
    assert_ne!(identity::NAME, "livtk");
}

#[test]
fn every_folder_liv_writes_is_its_own_and_none_of_livtk_apps() {
    let e = env(&[("HOME", "/Users/someone")]);
    let liv = Folders::of(&e).unwrap();

    // livtk.app's folders in the same environment, from its own functions.
    let livtk_config = livtk_core::registry::config_dir_with(&e).unwrap();
    let livtk_data = livtk_update::paths::base_with(&e).unwrap();
    let livtk_updates = livtk_update::paths::source_with(&e).unwrap();
    // Its log (livtk-app's applog), which no function outside the app gives.
    let livtk_logs = PathBuf::from("/Users/someone/Library/Logs/livtk");

    for (what, p) in [
        ("config", &liv.config),
        ("state", &liv.state),
        ("updates", &liv.updates),
        ("log", &liv.log),
    ] {
        for of in [&livtk_config, &livtk_data, &livtk_updates, &livtk_logs] {
            assert!(
                !within(p, of),
                "Liv's {what} ({}) is in livtk.app's {}",
                p.display(),
                of.display()
            );
        }
    }
    assert_ne!(liv.updates, livtk_updates, "its own update channel");
}

#[test]
fn liv_reads_only_its_own_overrides_and_they_move_everything_it_writes() {
    let livtk_only = env(&[
        ("HOME", "/h"),
        ("LIVTK_CONFIG_DIR", "/elsewhere/config"),
        ("LIVTK_UPDATES_DIR", "/elsewhere/updates"),
    ]);
    assert_eq!(
        Folders::of(&livtk_only),
        Folders::of(&env(&[("HOME", "/h")])),
        "livtk's overrides move nothing of Liv's"
    );

    let moved = Folders::of(&env(&[
        ("HOME", "/h"),
        ("LIV_CONFIG_DIR", "/t/config"),
        ("LIV_STATE_DIR", "/t/state"),
    ]))
    .unwrap();
    assert_eq!(moved.config, Path::new("/t/config"));
    assert_eq!(moved.state, Path::new("/t/state"));
    assert_eq!(moved.updates, Path::new("/t/state/updates"));
    assert_eq!(
        moved.log,
        Path::new("/t/state/liv.log"),
        "the log moves with the state"
    );
}

#[test]
fn without_a_home_folder_liv_has_no_folders() {
    assert_eq!(Folders::of(&env(&[])), None);
}
