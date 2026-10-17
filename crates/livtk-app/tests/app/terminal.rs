//! The terminal without a window: what keys and the mouse send a shell, the colours, and a real
//! shell started in a folder, showing what it prints and told of a new size.

use std::time::{Duration, Instant};

use gpui_kit::Keystroke;
use livtk_app::terminal::alacritty_terminal::term::TermMode;
use livtk_app::terminal::alacritty_terminal::term::color::Colors;
use livtk_app::terminal::alacritty_terminal::vte::ansi::{Color, NamedColor};
use livtk_app::terminal::colors::{self, ANSI};
use livtk_app::terminal::{Shell, Size, keys};

fn sent(key: &str) -> Option<Vec<u8>> {
    keys::to_bytes(&Keystroke::parse(key).unwrap(), false)
}

#[test]
fn keys_send_what_an_xterm_sends() {
    assert_eq!(sent("enter"), Some(b"\r".to_vec()));
    assert_eq!(sent("escape"), Some(b"\x1b".to_vec()), "Esc is the shell's");
    assert_eq!(sent("backspace"), Some(b"\x7f".to_vec()));
    assert_eq!(sent("shift-tab"), Some(b"\x1b[Z".to_vec()));
    assert_eq!(sent("ctrl-c"), Some(vec![0x03]));
    assert_eq!(sent("ctrl-d"), Some(vec![0x04]));
    assert_eq!(sent("ctrl-["), Some(vec![0x1b]));
    assert_eq!(sent("up"), Some(b"\x1b[A".to_vec()));
    let app = keys::to_bytes(&Keystroke::parse("up").unwrap(), true);
    assert_eq!(
        app,
        Some(b"\x1bOA".to_vec()),
        "a full-screen program's cursor keys"
    );
    assert_eq!(
        sent("shift-up"),
        Some(b"\x1b[1;2A".to_vec()),
        "modifiers as xterm encodes"
    );
    assert_eq!(sent("ctrl-right"), Some(b"\x1b[1;5C".to_vec()));
    assert_eq!(sent("pageup"), Some(b"\x1b[5~".to_vec()));
    assert_eq!(sent("f1"), Some(b"\x1bOP".to_vec()));
    assert_eq!(sent("f5"), Some(b"\x1b[15~".to_vec()));
}

#[test]
fn option_and_command_move_by_words_and_lines_and_other_command_keys_are_the_apps() {
    assert_eq!(sent("alt-left"), Some(b"\x1bb".to_vec()));
    assert_eq!(sent("alt-backspace"), Some(b"\x1b\x7f".to_vec()));
    assert_eq!(sent("cmd-left"), Some(vec![0x01]));
    assert_eq!(sent("cmd-backspace"), Some(vec![0x15]));
    assert_eq!(sent("cmd-k"), None, "the palette's");
    assert_eq!(sent("cmd-b"), None);
}

#[test]
fn typed_characters_are_sent_as_the_keyboard_makes_them() {
    let mut k = Keystroke::parse("a").unwrap();
    k.key_char = Some("a".into());
    assert_eq!(keys::to_bytes(&k, false), Some(b"a".to_vec()));
    // Option composes, as in Terminal.app: what the layout shows is typed.
    let mut k = Keystroke::parse("alt-2").unwrap();
    k.key_char = Some("@".into());
    assert_eq!(keys::to_bytes(&k, false), Some(b"@".to_vec()));
    let mut k = Keystroke::parse("e").unwrap();
    k.key_char = Some("é".into());
    assert_eq!(keys::to_bytes(&k, false), Some("é".as_bytes().to_vec()));
}

#[test]
fn the_mouse_is_reported_in_the_encoding_the_program_asked_for() {
    let sgr = TermMode::SGR_MOUSE | TermMode::MOUSE_REPORT_CLICK;
    assert_eq!(
        keys::mouse(0, 4, 2, true, sgr),
        Some(b"\x1b[<0;5;3M".to_vec())
    );
    assert_eq!(
        keys::mouse(0, 4, 2, false, sgr),
        Some(b"\x1b[<0;5;3m".to_vec())
    );
    assert_eq!(
        keys::mouse(64, 0, 0, true, sgr),
        Some(b"\x1b[<64;1;1M".to_vec())
    );
    let old = TermMode::MOUSE_REPORT_CLICK;
    assert_eq!(
        keys::mouse(0, 4, 2, true, old),
        Some(vec![0x1b, b'[', b'M', 32, 37, 35])
    );
    assert_eq!(
        keys::mouse(0, 4, 2, false, old).map(|b| b[3]),
        Some(35),
        "a release is 3"
    );
    assert_eq!(
        keys::mouse(0, 300, 2, true, old),
        None,
        "past what it can tell"
    );
}

#[test]
fn colours_follow_the_palette_and_the_256() {
    let c = Colors::default();
    assert_eq!(colors::rgb(Color::Named(NamedColor::Red), &c), ANSI[1]);
    assert_eq!(
        colors::rgb(Color::Named(NamedColor::BrightWhite), &c),
        ANSI[15]
    );
    assert_eq!(
        colors::rgb(Color::Named(NamedColor::Background), &c),
        colors::BACKGROUND
    );
    assert_eq!(
        colors::rgb(Color::Indexed(16), &c),
        0x000000,
        "the cube's black"
    );
    assert_eq!(colors::rgb(Color::Indexed(21), &c), 0x0000ff);
    assert_eq!(colors::rgb(Color::Indexed(196), &c), 0xff0000);
    assert_eq!(colors::rgb(Color::Indexed(232), &c), 0x080808, "the greys");
    assert_eq!(colors::rgb(Color::Indexed(255), &c), 0xeeeeee);
    assert_eq!(colors::dim(0x9999cc), 0x666688);
}

const SIZE: Size = Size {
    columns: 80,
    lines: 24,
    cell_width: 8,
    cell_height: 17,
};

/// Wait until the screen shows `what`.
fn wait_for(shell: &Shell, what: &str) -> Vec<String> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let text = shell.screen_text();
        if text.iter().any(|l| l.contains(what)) {
            return text;
        }
        assert!(Instant::now() < until, "{what:?} never came: {text:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_shell_starts_in_its_folder_and_shows_what_it_prints() {
    let dir = tempfile::tempdir().unwrap();
    let name = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let (shell, _events) = Shell::spawn_program(
        dir.path(),
        SIZE,
        "/bin/sh",
        &[
            "-c".into(),
            "printf 'in %s as %s\\n' \"$(basename \"$PWD\")\" \"$TERM\"; sleep 5".into(),
        ],
    )
    .unwrap();
    wait_for(&shell, &format!("in {name} as xterm-256color"));
}

#[test]
fn what_is_typed_reaches_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let (shell, _events) = Shell::spawn_program(
        dir.path(),
        SIZE,
        "/bin/sh",
        &["-c".into(), "read line; echo \"got $line\"; sleep 5".into()],
    )
    .unwrap();
    shell.write(b"hello\r".to_vec());
    wait_for(&shell, "got hello");
}

#[test]
fn a_new_size_reaches_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _events) = Shell::spawn_program(
        dir.path(),
        SIZE,
        "/bin/sh",
        &["-c".into(), "read go; stty size; sleep 5".into()],
    )
    .unwrap();
    shell.resize(Size {
        columns: 100,
        lines: 30,
        ..SIZE
    });
    shell.write(b"\r".to_vec());
    wait_for(&shell, "30 100");
}
