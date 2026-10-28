//! Liv lives beside the app in use (`livtk.app`) and never touches it: its identity is its own and
//! written in one place, and every folder it writes is its own, on every platform.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use etcetera::app_strategy::{Apple, Windows, Xdg};
use liv::identity::{self, Folders, Platform};

/// An environment holding only `vars`.
fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<PathBuf> {
    let map: HashMap<String, PathBuf> = vars
        .iter()
        .map(|(k, v)| ((*k).to_owned(), PathBuf::from(v)))
        .collect();
    move |k| map.get(k).cloned()
}

#[test]
fn liv_has_its_own_identifier_and_name() {
    assert_ne!(identity::APP_ID, "com.github.nebulaw.livtk");
    assert_ne!(identity::NAME, "livtk");
}

/// The identifier etcetera names Liv's macOS folders by is the bundle's.
#[test]
fn identifier_is_the_platform_s_args() {
    let args = identity::strategy_args();
    assert_eq!(args.bundle_id(), identity::APP_ID);
    assert_eq!(args.unixy_name(), identity::SHORT);
}

/// What `cargo bundle` reads agrees with `identity.rs`: one identity, held together.
#[test]
fn the_bundle_s_metadata_agrees_with_liv_s_identity() {
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let doc: toml::Table = toml::from_str(&manifest).unwrap();
    let bundle = &doc["package"]["metadata"]["bundle"];
    assert_eq!(bundle["identifier"].as_str(), Some(identity::APP_ID));
    assert_eq!(bundle["name"].as_str(), Some(identity::NAME));
    assert_eq!(
        bundle["linux"]["startup_wm_class"].as_str(),
        Some(identity::SHORT),
        "the window's app id is the desktop file's name"
    );
}

/// livtk.app's folders on this machine, from its own functions.
fn livtk_folders() -> Vec<PathBuf> {
    let home = etcetera::home_dir().unwrap();
    [
        livtk_core::registry::config_dir(),
        livtk_update::paths::base(),
        livtk_update::paths::source(),
        Some(home.join("Library/Logs/livtk")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Each platform's convention, worked out here: every folder is named for Liv and none is in
/// livtk.app's.
#[test]
fn every_platform_s_folders_are_liv_s_own() {
    let args = identity::strategy_args;
    let platforms = [
        ("macOS", Platform::apple(&Apple::new(args()).unwrap())),
        ("Linux", Platform::xdg(&Xdg::new(args()).unwrap())),
        ("Windows", Platform::windows(&Windows::new(args()).unwrap())),
    ];
    let livtk = livtk_folders();
    for (os, p) in platforms {
        for (what, dir) in [
            ("config", &p.config),
            ("state", &p.state),
            ("cache", &p.cache),
            ("logs", &p.logs),
        ] {
            assert!(
                dir.components().any(|c| {
                    let c = c.as_os_str();
                    c == identity::APP_ID || c == identity::SHORT
                }),
                "{os}'s {what} ({}) is named for Liv",
                dir.display()
            );
            for of in &livtk {
                assert!(
                    !dir.starts_with(of),
                    "{os}'s {what} ({}) is in livtk.app's {}",
                    dir.display(),
                    of.display()
                );
            }
        }
    }
}

fn base() -> Platform {
    Platform {
        config: "/p/config".into(),
        state: "/p/state".into(),
        cache: "/p/cache".into(),
        logs: "/p/logs".into(),
    }
}

#[test]
fn without_overrides_liv_uses_the_platform_s_folders() {
    let f = Folders::of(&env(&[]), base());
    assert_eq!(f.config, Path::new("/p/config"));
    assert_eq!(f.state, Path::new("/p/state"));
    assert_eq!(f.cache, Path::new("/p/cache"));
    assert_eq!(f.updates, Path::new("/p/state/updates"));
    assert_eq!(f.log, Path::new("/p/logs/liv.log"));
}

#[test]
fn liv_reads_only_its_own_overrides_and_they_move_everything_it_writes() {
    let livtk_only = env(&[
        ("LIVTK_CONFIG_DIR", "/elsewhere/config"),
        ("LIVTK_UPDATES_DIR", "/elsewhere/updates"),
    ]);
    assert_eq!(
        Folders::of(&livtk_only, base()),
        Folders::of(&env(&[]), base()),
        "livtk's overrides move nothing of Liv's"
    );

    let moved = Folders::of(
        &env(&[
            ("LIV_CONFIG_DIR", "/t/config"),
            ("LIV_STATE_DIR", "/t/state"),
        ]),
        base(),
    );
    assert_eq!(moved.config, Path::new("/t/config"));
    assert_eq!(moved.state, Path::new("/t/state"));
    assert_eq!(moved.cache, Path::new("/t/state/cache"));
    assert_eq!(moved.updates, Path::new("/t/state/updates"));
    assert_eq!(
        moved.log,
        Path::new("/t/state/liv.log"),
        "the log moves with the state"
    );
}
