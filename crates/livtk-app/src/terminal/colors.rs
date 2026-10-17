//! The terminal's colours, as `0xRRGGBB`: the sixteen named ones tuned to the app's dark (its
//! red, green, yellow and indigo where it has them), the 6×6×6 cube and the greys of the 256,
//! and true colour as the program gives it. A program that sets a colour of its own (OSC 4, 10,
//! 11) has it.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

/// The screen's own background and text, and the cursor.
pub const BACKGROUND: u32 = 0x08090a;
pub const FOREGROUND: u32 = 0xd0d6e0;
pub const CURSOR: u32 = 0xf7f8f8;

/// Black, red, green, yellow, blue, magenta, cyan, white; then their bright kin.
pub const ANSI: [u32; 16] = [
    0x23252a, 0xeb5757, 0x4cb782, 0xf2c94c, 0x828fff, 0xb46ce0, 0x4cc7d2, 0xd0d6e0, //
    0x62666d, 0xff7b7b, 0x6fd6a0, 0xffdc7a, 0xa3adff, 0xd08cf2, 0x7ee0e8, 0xf7f8f8,
];

/// `c` as the screen shows it, `colors` holding what the program set.
pub fn rgb(c: Color, colors: &Colors) -> u32 {
    match c {
        Color::Spec(Rgb { r, g, b }) => pack(r, g, b),
        Color::Indexed(i) => indexed(i, colors),
        Color::Named(n) => named(n, colors),
    }
}

fn indexed(i: u8, colors: &Colors) -> u32 {
    if let Some(Rgb { r, g, b }) = colors[usize::from(i)] {
        return pack(r, g, b);
    }
    match i {
        0..=15 => ANSI[usize::from(i)],
        // The cube: each of red, green, blue in six steps, the first black.
        16..=231 => {
            let n = i - 16;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            pack(step(n / 36), step((n / 6) % 6), step(n % 6))
        }
        // The greys, from near black to near white.
        _ => {
            let v = 8 + (i - 232) * 10;
            pack(v, v, v)
        }
    }
}

fn named(n: NamedColor, colors: &Colors) -> u32 {
    if let Some(Rgb { r, g, b }) = colors[n] {
        return pack(r, g, b);
    }
    let i = n as usize;
    match n {
        NamedColor::Foreground | NamedColor::BrightForeground => FOREGROUND,
        NamedColor::Background => BACKGROUND,
        NamedColor::Cursor => CURSOR,
        NamedColor::DimForeground => dim(FOREGROUND),
        NamedColor::DimBlack
        | NamedColor::DimRed
        | NamedColor::DimGreen
        | NamedColor::DimYellow
        | NamedColor::DimBlue
        | NamedColor::DimMagenta
        | NamedColor::DimCyan
        | NamedColor::DimWhite => dim(ANSI[i - NamedColor::DimBlack as usize]),
        _ if i < 16 => ANSI[i],
        _ => FOREGROUND,
    }
}

/// A colour two thirds as bright, for faint text.
pub fn dim(c: u32) -> u32 {
    let part = |shift: u32| (((c >> shift) & 0xff) * 2 / 3) << shift;
    part(16) | part(8) | part(0)
}

fn pack(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}
