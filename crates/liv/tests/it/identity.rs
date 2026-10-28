//! Liv lives beside the app in use (`livtk.app`) and never touches it: its identity is its own and
//! written in one place, and every folder it writes is its own, on every platform.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

/// Each platform's folders, exactly: which folder holds what.
#[test]
fn each_platform_s_folders_are_exactly_its_convention() {
    let mac = Platform::apple(Path::new("/Users/u"));
    assert_eq!(
        mac.config,
        Path::new("/Users/u/Library/Application Support/com.github.nebulaw.liv")
    );
    assert_eq!(
        mac.state, mac.config,
        "settings beside state, not in ~/Library/Preferences"
    );
    assert_eq!(
        mac.cache,
        Path::new("/Users/u/Library/Caches/com.github.nebulaw.liv")
    );
    assert_eq!(
        mac.logs,
        Path::new("/Users/u/Library/Logs/com.github.nebulaw.liv")
    );

    let linux = Platform::xdg(
        Path::new("/h/.config"),
        Path::new("/h/.local/state"),
        Path::new("/h/.cache"),
    );
    assert_eq!(linux.config, Path::new("/h/.config/liv"));
    assert_eq!(linux.state, Path::new("/h/.local/state/liv"));
    assert_eq!(linux.cache, Path::new("/h/.cache/liv"));
    assert_eq!(linux.logs, Path::new("/h/.local/state/liv/logs"));

    let roaming = Path::new("C:/Users/u/AppData/Roaming");
    let local = Path::new("C:/Users/u/AppData/Local");
    let win = Platform::windows(roaming, local);
    assert_eq!(
        win.config,
        roaming.join("nebulaw").join("liv").join("config"),
        "preferences roam"
    );
    for (what, dir, name) in [
        ("state", &win.state, "state"),
        ("logs", &win.logs, "logs"),
        ("cache", &win.cache, "cache"),
    ] {
        assert_eq!(
            *dir,
            local.join("nebulaw").join("liv").join(name),
            "{what} stays on this machine"
        );
    }
}

/// Liv's folders are named as etcetera's app strategies name an app's, on the same base folders.
#[test]
fn the_naming_is_etcetera_s() {
    use etcetera::AppStrategy;
    use etcetera::BaseStrategy;
    use etcetera::app_strategy::{Apple, Windows, Xdg};
    let args = identity::strategy_args;

    let apple = Apple::new(args()).unwrap();
    let mac = Platform::apple(apple.home_dir());
    assert_eq!(mac.state, apple.data_dir());
    assert_eq!(mac.cache, apple.cache_dir());

    let xdg = Xdg::new(args()).unwrap();
    let base = etcetera::base_strategy::Xdg::new().unwrap();
    let linux = Platform::xdg(
        &base.config_dir(),
        &base.state_dir().unwrap(),
        &base.cache_dir(),
    );
    assert_eq!(linux.config, xdg.config_dir());
    assert_eq!(Some(linux.state.clone()), xdg.state_dir());
    assert_eq!(linux.cache, xdg.cache_dir());

    let windows = Windows::new(args()).unwrap();
    let base = etcetera::base_strategy::Windows::new().unwrap();
    let win = Platform::windows(&base.data_dir(), &base.cache_dir());
    assert_eq!(win.config, windows.config_dir());
    assert_eq!(win.cache, windows.cache_dir());
}

/// On this platform, in a fixed environment: every folder Liv writes is outside every folder
/// livtk.app writes, worked out by livtk.app's own functions in that environment.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn liv_writes_nothing_of_livtk_app_s() {
    let e = env(&[("HOME", "/h")]);
    let platform = if cfg!(target_os = "macos") {
        Platform::apple(Path::new("/h"))
    } else {
        Platform::xdg(
            Path::new("/h/.config"),
            Path::new("/h/.local/state"),
            Path::new("/h/.cache"),
        )
    };
    let liv = Folders::of(&e, platform);
    let livtk = [
        livtk_core::registry::config_dir_with(&e).unwrap(),
        livtk_update::paths::base_with(&e).unwrap(),
        livtk_update::paths::source_with(&e).unwrap(),
        PathBuf::from("/h/Library/Logs/livtk"),
    ];
    for (what, p) in [
        ("config", &liv.config),
        ("state", &liv.state),
        ("cache", &liv.cache),
        ("updates", &liv.updates),
        ("log", &liv.log),
    ] {
        for of in &livtk {
            assert!(
                !p.starts_with(of),
                "Liv's {what} ({}) is in livtk.app's {}",
                p.display(),
                of.display()
            );
            assert!(
                !of.starts_with(p),
                "livtk.app's {} is in Liv's {what}",
                of.display()
            );
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
