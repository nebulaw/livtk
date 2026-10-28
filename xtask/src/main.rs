//! The project's own tasks, run as `cargo xtask <task>`.
//!
//! `icons`: the app's icons, made from the one master, `assets/brand/logo.png`. Nothing is drawn
//! by hand: change the master (or a setting below) and run it again. What it writes is committed,
//! so a build never needs it; a test fails when the committed files no longer match the master.
//!
//! `fixture`: a project of any number of events, made from a real log, to measure how livtk holds
//! as projects grow (see `fixture.rs`).
//!
//! Today it makes the macOS icon. Each platform is one output of the same rendering:
//! - macOS (`livtk.icns`): Apple's icon grid, the art in a rounded square with its shadow.
//! - Windows, later: an `.ico` of 16, 20, 24, 32, 40, 48, 64 and 256 (256 stored as PNG),
//!   embedded in the `.exe` by `winresource` from `build.rs`, Windows targets only.
//! - Linux, later: `hicolor` PNGs (16 to 512) named after the app id,
//!   `com.github.nebulaw.livtk.png`, with a `com.github.nebulaw.livtk.desktop` beside them.

mod fixture;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use image::imageops::{self, FilterType};
use image::{ImageFormat, Rgba, RgbaImage};

fn main() -> Result<()> {
    let task = std::env::args().nth(1).unwrap_or_default();
    match task.as_str() {
        "icons" => {
            let brand = brand_dir();
            for (name, bytes) in icons(&image::open(brand.join(MASTER))?.to_rgba8())? {
                std::fs::write(brand.join(name), bytes)?;
                println!("wrote assets/brand/{name}");
            }
            Ok(())
        }
        "fixture" => fixture::run(&std::env::args().skip(2).collect::<Vec<_>>()),
        _ => bail!(
            "usage: cargo xtask icons | fixture --events N --out DIR"
        ),
    }
}

/// Where the brand's files live, the master and what is made from it.
fn brand_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/brand")
}

const MASTER: &str = "logo.png";

/// The macOS icon file.
pub const ICNS: &str = "livtk.icns";
/// The icon the app shows itself (About): 128 points at 2x.
pub const APP_PNG: &str = "livtk-256.png";

// ---- Apple's icon grid (macOS 11 and later), on a 1024 canvas ----------------------------

const CANVAS: u32 = 1024;
/// The rounded square: 824 wide, 100 in from each edge.
const TILE: f32 = 824.0;
const MARGIN: f32 = 100.0;
const RADIUS: f32 = 185.4;
/// The tile's shadow below it: black, at this opacity, this far down, this soft.
const SHADOW_OPACITY: f32 = 0.3;
const SHADOW_DROP: i64 = 12;
const SHADOW_BLUR: f32 = 14.0;

// ---- Small sizes ------------------------------------------------------------------------

/// At this many pixels and below, the close-up art is mush: these sizes show a tighter crop.
const SMALL_UP_TO: u32 = 32;
/// The crop small sizes show, as a square in the master's pixels: centred on the smiley.
const SMALL_CROP: Crop = Crop {
    x: 270,
    y: 20,
    size: 640,
};

#[derive(Clone, Copy)]
struct Crop {
    x: u32,
    y: u32,
    size: u32,
}

/// Every image in `livtk.icns`: its type, and its size in pixels. The ten of an `.iconset`:
/// 16, 32, 128, 256 and 512 points, each at 1x and 2x. The 16 and 32 at 1x are `ic04` and
/// `ic05`, ARGB as `iconutil` writes them: macOS misreads them as PNG (`icp4`, `icp5`).
const ICNS_ENTRIES: [(&[u8; 4], u32); 10] = [
    (b"ic04", 16),
    (b"ic11", 32),
    (b"ic05", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic13", 256),
    (b"ic08", 256),
    (b"ic14", 512),
    (b"ic09", 512),
    (b"ic10", 1024),
];

/// Every file made from `master`, by name.
pub fn icons(master: &RgbaImage) -> Result<Vec<(&'static str, Vec<u8>)>> {
    let large = tile(master, None);
    let small = tile(master, Some(SMALL_CROP));
    let at = |px: u32| -> &RgbaImage { if px <= SMALL_UP_TO { &small } else { &large } };
    let mut entries = Vec::new();
    for (kind, px) in ICNS_ENTRIES {
        let img = shrink(at(px), px);
        let data = if matches!(kind, b"ic04" | b"ic05") {
            argb(&img)
        } else {
            png(&img)?
        };
        entries.push((*kind, data));
    }
    Ok(vec![
        (ICNS, icns(&entries)),
        (APP_PNG, png(&shrink(&large, 256))?),
    ])
}

/// The master (or a crop of it) in Apple's rounded square on a transparent 1024 canvas, its
/// shadow below.
fn tile(master: &RgbaImage, crop: Option<Crop>) -> RgbaImage {
    let art = match crop {
        Some(c) => imageops::crop_imm(master, c.x, c.y, c.size, c.size).to_image(),
        None => master.clone(),
    };
    let side = TILE.round() as u32;
    let art = imageops::resize(&art, side, side, FilterType::Lanczos3);
    // How much of each pixel the rounded square covers, smoothed at its edge.
    let cover = |x: u32, y: u32| -> f32 {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let half = TILE / 2.0;
        let (cx, cy) = (MARGIN + half, MARGIN + half);
        let (qx, qy) = (
            (px - cx).abs() - (half - RADIUS),
            (py - cy).abs() - (half - RADIUS),
        );
        let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        let d = outside + qx.max(qy).min(0.0) - RADIUS;
        (0.5 - d).clamp(0.0, 1.0)
    };
    // The shadow: the square's coverage, dropped, softened, darkened.
    let mut shadow = RgbaImage::new(CANVAS, CANVAS);
    for (x, y, p) in shadow.enumerate_pixels_mut() {
        let a = y
            .checked_sub(SHADOW_DROP as u32)
            .map_or(0.0, |sy| cover(x, sy));
        *p = Rgba([0, 0, 0, (a * 255.0).round() as u8]);
    }
    let shadow = imageops::blur(&shadow, SHADOW_BLUR);
    let mut out = RgbaImage::new(CANVAS, CANVAS);
    for (x, y, p) in out.enumerate_pixels_mut() {
        let s = f32::from(shadow.get_pixel(x, y)[3]) / 255.0 * SHADOW_OPACITY;
        let c = cover(x, y);
        let src = if c > 0.0 {
            let ax = (x as f32 - MARGIN).clamp(0.0, TILE - 1.0) as u32;
            let ay = (y as f32 - MARGIN).clamp(0.0, TILE - 1.0) as u32;
            *art.get_pixel(ax, ay)
        } else {
            Rgba([0, 0, 0, 0])
        };
        // The tile over its shadow (the tile's art is opaque; its edge is its coverage).
        let a = c + s * (1.0 - c);
        let rgb = |i: usize| -> u8 {
            if a <= 0.0 {
                0
            } else {
                (f32::from(src[i]) * c / a).round().clamp(0.0, 255.0) as u8
            }
        };
        *p = Rgba([rgb(0), rgb(1), rgb(2), (a * 255.0).round() as u8]);
    }
    out
}

/// `img` at `px` square, resized with its colours weighted by their alpha, so no dark fringe
/// creeps in from the transparent pixels around the tile.
fn shrink(img: &RgbaImage, px: u32) -> RgbaImage {
    let mut pre = img.clone();
    for p in pre.pixels_mut() {
        let a = f32::from(p[3]) / 255.0;
        for i in 0..3 {
            p[i] = (f32::from(p[i]) * a).round() as u8;
        }
    }
    let mut out = imageops::resize(&pre, px, px, FilterType::Lanczos3);
    for p in out.pixels_mut() {
        let a = f32::from(p[3]) / 255.0;
        for i in 0..3 {
            p[i] = if a > 0.0 {
                (f32::from(p[i]) / a).round().clamp(0.0, 255.0) as u8
            } else {
                0
            };
        }
    }
    out
}

fn png(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)
        .context("encoding a PNG")?;
    Ok(out.into_inner())
}

/// An image as `ic04` and `ic05` hold it: `ARGB`, then its alpha, red, green and blue planes,
/// each compressed as `.icns` compresses: a byte under 128 copies that many plus one bytes; one
/// of 128 or more repeats the next byte that many less 125 times.
fn argb(img: &RgbaImage) -> Vec<u8> {
    let mut out = b"ARGB".to_vec();
    for channel in [3, 0, 1, 2] {
        let plane: Vec<u8> = img.pixels().map(|p| p[channel]).collect();
        out.extend(rle(&plane));
    }
    out
}

fn rle(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let run = data[i..]
            .iter()
            .take(130)
            .take_while(|&&b| b == data[i])
            .count();
        if run >= 3 {
            out.push(u8::try_from(run + 125).unwrap_or(u8::MAX));
            out.push(data[i]);
            i += run;
            continue;
        }
        // Literal bytes, up to 128, until a run of three begins.
        let start = i;
        while i < data.len() && i - start < 128 {
            if i + 2 < data.len() && data[i] == data[i + 1] && data[i] == data[i + 2] {
                break;
            }
            i += 1;
        }
        out.push(u8::try_from(i - start - 1).unwrap_or(0));
        out.extend_from_slice(&data[start..i]);
    }
    out
}

/// An `.icns` file: `icns`, its length, then each image as its type, its length and its PNG.
/// Lengths are big-endian and count their own 8-byte header.
fn icns(entries: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let body: usize = entries.iter().map(|(_, d)| d.len() + 8).sum();
    let mut out = Vec::with_capacity(body + 8);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&u32::try_from(body + 8).unwrap_or(u32::MAX).to_be_bytes());
    for (kind, data) in entries {
        out.extend_from_slice(kind);
        out.extend_from_slice(
            &u32::try_from(data.len() + 8)
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        out.extend_from_slice(data);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn master() -> &'static RgbaImage {
        static MASTER_IMG: std::sync::OnceLock<RgbaImage> = std::sync::OnceLock::new();
        MASTER_IMG.get_or_init(|| {
            image::open(brand_dir().join(MASTER))
                .map(|i| i.to_rgba8())
                .unwrap_or_else(|e| panic!("the master: {e}"))
        })
    }

    /// What the master makes, made once for every test.
    fn made() -> &'static [(&'static str, Vec<u8>)] {
        static MADE: std::sync::OnceLock<Vec<(&'static str, Vec<u8>)>> = std::sync::OnceLock::new();
        MADE.get_or_init(|| icons(master()).unwrap_or_else(|e| panic!("making the icons: {e}")))
    }

    /// The images an `.icns` holds: each one's type and its decoded size.
    fn entries(file: &[u8]) -> Vec<(String, u32, u32)> {
        assert_eq!(&file[..4], b"icns");
        let total = u32::from_be_bytes([file[4], file[5], file[6], file[7]]) as usize;
        assert_eq!(total, file.len(), "the header says the file's length");
        let mut at = 8;
        let mut out = Vec::new();
        while at < file.len() {
            let kind = String::from_utf8_lossy(&file[at..at + 4]).into_owned();
            let len = u32::from_be_bytes([file[at + 4], file[at + 5], file[at + 6], file[at + 7]])
                as usize;
            let data = &file[at + 8..at + len];
            let (w, h) = if data.starts_with(b"ARGB") {
                let px = unrle(&data[4..]).len() / 4;
                let side = (px as f64).sqrt() as u32;
                (side, side)
            } else {
                let img = image::load_from_memory_with_format(data, ImageFormat::Png)
                    .unwrap_or_else(|e| panic!("{kind}: {e}"));
                (img.width(), img.height())
            };
            out.push((kind, w, h));
            at += len;
        }
        out
    }

    fn unrle(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < data.len() {
            let b = data[i];
            if b < 128 {
                out.extend_from_slice(&data[i + 1..i + 2 + usize::from(b)]);
                i += 2 + usize::from(b);
            } else {
                out.extend(std::iter::repeat_n(data[i + 1], usize::from(b) - 125));
                i += 2;
            }
        }
        out
    }

    #[test]
    fn compressed_planes_read_back_as_they_were() {
        let samples: [&[u8]; 5] = [
            &[],
            &[7; 300],
            &[1, 2, 3, 4, 5],
            &[1, 1, 2, 2, 2, 2, 3, 1, 1, 1],
            &(0..=255).chain(0..=255).collect::<Vec<u8>>(),
        ];
        for s in samples {
            assert_eq!(unrle(&rle(s)), s);
        }
    }

    #[test]
    fn the_icns_has_every_size_apple_asks_for() {
        let made = made();
        let (_, icns) = made
            .iter()
            .find(|(n, _)| *n == ICNS)
            .unwrap_or_else(|| panic!("no icns"));
        let got = entries(icns);
        let want: Vec<(String, u32, u32)> = ICNS_ENTRIES
            .iter()
            .map(|(k, px)| (String::from_utf8_lossy(*k).into_owned(), *px, *px))
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn the_tile_is_transparent_outside_its_rounded_square() {
        let t = tile(master(), None);
        // The canvas's corners and the square's own corner, cut away by its radius.
        for (x, y) in [(0, 0), (1023, 0), (110, 110), (913, 110)] {
            assert_eq!(t.get_pixel(x, y)[3], 0, "({x}, {y}) is clear");
        }
        // Its middle is the art, whole.
        assert_eq!(t.get_pixel(512, 512)[3], 255);
        // Below it, its shadow: some, never much.
        let below = t.get_pixel(512, 930)[3];
        assert!(below > 0 && below < 128, "the shadow below: {below}");
    }

    #[test]
    fn the_committed_icons_are_what_the_master_makes() {
        for (name, bytes) in made() {
            let committed = std::fs::read(brand_dir().join(name))
                .unwrap_or_else(|e| panic!("assets/brand/{name}: {e}"));
            assert!(
                committed == *bytes,
                "assets/brand/{name} is out of date: run `cargo xtask icons`"
            );
        }
    }
}
