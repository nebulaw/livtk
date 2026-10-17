//! Rules about how the app looks that the developer asked for, checked against the source and
//! the icon files, so a later change cannot quietly break them.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use livtk_app::ui::icons::Icon;

#[test]
fn every_icon_is_a_credited_boxicon_of_24_by_24() {
    for &i in Icon::ALL {
        let svg = std::str::from_utf8(i.svg()).expect("text");
        assert!(svg.starts_with("<svg"), "{}", i.file());
        assert!(
            svg.contains(r#"viewBox="0 0 24 24""#),
            "{}: 24 by 24",
            i.file()
        );
        assert!(
            svg.contains("Boxicons"),
            "{}: keeps its credit (CC BY 4.0)",
            i.file()
        );
        assert!(
            svg.contains("currentColor"),
            "{}: tinted by the app",
            i.file()
        );
        assert!(
            !svg.contains("<script") && !svg.contains("href="),
            "{}",
            i.file()
        );
    }
}

#[test]
fn icons_are_named_once() {
    let mut files: Vec<&str> = Icon::ALL.iter().map(|i| i.file()).collect();
    files.sort_unstable();
    let count = files.len();
    files.dedup();
    assert_eq!(files.len(), count, "two names for one icon file");
}

/// The app bundle carries the identity the app gives itself, or macOS would not match the two.
#[test]
fn the_app_bundle_is_the_app_it_says_it_is() {
    let script = include_str!("../../../../scripts/bundle-app");
    assert!(
        script.contains(&format!("id={}", livtk_app::ui::APP_ID)),
        "scripts/bundle-app names {}",
        livtk_app::ui::APP_ID
    );
}
