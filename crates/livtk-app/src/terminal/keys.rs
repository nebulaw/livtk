//! What a key sends the shell: the bytes an xterm sends for it, as Terminal.app and iTerm do on
//! macOS. Letters and what the keyboard composes are sent as typed (Option makes characters, as
//! in Terminal.app, so every keyboard layout types what it shows); Control makes control
//! characters; the arrows and the rest send their escape sequences, in the program's cursor mode,
//! with the modifiers encoded as xterm encodes them. Option and Command with the arrows and
//! Backspace move and delete by words and lines, as in a Mac text field. Other Command keys are
//! the app's: `None`. And what a program that asked for the mouse is told of it.

use alacritty_terminal::term::TermMode;
use gpui_kit::Keystroke;

/// The bytes for `k`; `app_cursor` while the program asked for application cursor keys (as
/// full-screen programs do). `None` for a key the shell is not sent.
pub fn to_bytes(k: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let m = &k.modifiers;
    let key = k.key.as_str();
    // Command: a Mac text field's line moves; anything else is the app's.
    if m.platform {
        return match key {
            "left" => Some(vec![0x01]),      // Ctrl+A: the line's start
            "right" => Some(vec![0x05]),     // Ctrl+E: its end
            "backspace" => Some(vec![0x15]), // Ctrl+U: the line before the cursor
            _ => None,
        };
    }
    // Option: a word at a time.
    if m.alt && !m.control && !m.shift {
        match key {
            "left" => return Some(b"\x1bb".to_vec()),
            "right" => return Some(b"\x1bf".to_vec()),
            "backspace" => return Some(b"\x1b\x7f".to_vec()),
            _ => {}
        }
    }
    // xterm's modifier parameter: 1, plus 1 for Shift, 2 for Option, 4 for Control.
    let param = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.control);
    let modified = param > 1;
    let cursor = |end: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{param}{}", end as char).into_bytes()
        } else if app_cursor {
            vec![0x1b, b'O', end]
        } else {
            vec![0x1b, b'[', end]
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{code};{param}~").into_bytes()
        } else {
            format!("\x1b[{code}~").into_bytes()
        }
    };
    let function = |end: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{param}{}", end as char).into_bytes()
        } else {
            vec![0x1b, b'O', end]
        }
    };
    let bytes = match key {
        "enter" => vec![b'\r'],
        "escape" => vec![0x1b],
        "backspace" if m.control => vec![0x08],
        "backspace" => vec![0x7f],
        "tab" if m.shift => b"\x1b[Z".to_vec(),
        "tab" => vec![b'\t'],
        "space" if m.control => vec![0x00],
        "up" => cursor(b'A'),
        "down" => cursor(b'B'),
        "right" => cursor(b'C'),
        "left" => cursor(b'D'),
        "home" => cursor(b'H'),
        "end" => cursor(b'F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "f1" => function(b'P'),
        "f2" => function(b'Q'),
        "f3" => function(b'R'),
        "f4" => function(b'S'),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        _ if m.control => return control(key),
        "space" => vec![b' '],
        _ => match &k.key_char {
            Some(c) => c.as_bytes().to_vec(),
            None if key.chars().count() == 1 => key.as_bytes().to_vec(),
            None => return None,
        },
    };
    Some(bytes)
}

/// Control and `key`: its control character (Ctrl+C, 0x03), as a terminal makes them.
fn control(key: &str) -> Option<Vec<u8>> {
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let byte = match c {
        'a'..='z' => c as u8 - b'a' + 1,
        '@' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '7' => 0x1f,
        '/' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    };
    Some(vec![byte])
}

/// The mouse, told to a program that asked for it: button `code` (0 left, 1 middle, 2 right, 64
/// and 65 the wheel up and down; 32 more while it moves, plus the modifiers' bits) at `column`
/// and `line` (from 0), pressed or let go. In SGR's encoding when the program asked for it, else
/// in the old one, which cannot tell a place past 223 and so says nothing there.
pub fn mouse(
    code: u8,
    column: usize,
    line: usize,
    pressed: bool,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{code};{};{}{end}", column + 1, line + 1).into_bytes());
    }
    // The old encoding tells a release as button 3, whichever it was.
    let code = if pressed { code } else { 3 | (code & !3) };
    let at = |v: usize| u8::try_from(v + 1 + 32).ok();
    Some(vec![0x1b, b'[', b'M', 32 + code, at(column)?, at(line)?])
}
