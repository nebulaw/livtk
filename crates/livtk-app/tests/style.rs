//! Rules about how the app looks that the developer asked for, checked against the source and
//! the icon files, so a later change cannot quietly break them.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use livtk_app::ui::icons::Icon;

const UI: &str = include_str!("../src/ui/mod.rs");
const ICONS: &str = include_str!("../src/ui/icons.rs");

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

/// The developer's rule: the cursor never changes on hover; only while dragging.
#[test]
fn the_cursor_changes_only_while_dragging() {
    for source in [UI, ICONS] {
        for banned in [
            ".cursor_pointer()",
            ".cursor_text()",
            ".cursor_grab()",
            ".cursor_default()",
            "CursorStyle::PointingHand",
            "CursorStyle::OpenHand",
            "CursorStyle::IBeam",
        ] {
            assert!(!source.contains(banned), "found {banned}");
        }
    }
    let changes: Vec<&str> = UI.lines().filter(|l| l.contains(".cursor(")).collect();
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert!(
        changes[0].contains("when(dragging"),
        "the one change is while dragging: {}",
        changes[0]
    );
}

/// No element grows or zooms on hover.
#[test]
fn nothing_scales_on_hover() {
    for (i, line) in UI.lines().enumerate() {
        if line.contains(".hover(") {
            for banned in [
                "scale",
                ".size(",
                ".w(",
                ".h(",
                "zoom",
                "with_transformation",
            ] {
                assert!(!line.contains(banned), "line {}: {line}", i + 1);
            }
        }
    }
}

/// The developer's rule for the dock's room: it is space at the end of what scrolls, so rows
/// and cards pass under the dock. Padding on a list or column clips them short of it instead,
/// which a headless test cannot see (it reports layout, not clipping), so the source is held
/// to it.
#[test]
fn the_docks_room_is_space_at_the_end_never_padding() {
    assert!(
        !UI.contains("pb(px(DOCK_SPACE))") && !UI.contains("pb(px(DOCK_HEIGHT"),
        "the dock's room is padding somewhere"
    );
    assert!(
        UI.matches("div().h(px(DOCK_SPACE)).flex_none()").count() >= 2,
        "the inbox and each board column end with the room"
    );
    assert!(
        UI.matches("+ DOCK_ROWS").count() >= 2,
        "the virtualised lists, tasks and the feed, end with rows of room"
    );
}
