//! `fixture`: a project of any size, made from a real log, to measure how livtk holds as projects
//! grow (TAS-125). The real log is copied over and over, each copy ("generation") with its own
//! ids and keys, so the sizes of texts, the mix of events and the links between them stay as
//! they are in real use; only the count grows.
//!
//!   cargo xtask fixture --events N --out DIR [--from PATH]
//!
//! `--from` is a project's `.livtk/events.jsonl` (this repository's own by default); its config
//! is copied beside the made log. The projection is not made: the first `livtk` command there
//! builds it, which is itself one of the things measured.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// How far apart the keys of one generation are from the next: TAS-7 of generation 2 is
/// TAS-200007. Above any real project's count.
const KEY_STEP: u64 = 100_000;

pub fn run(args: &[String]) -> Result<()> {
    let mut events: Option<usize> = None;
    let mut out: Option<PathBuf> = None;
    let mut from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.livtk/events.jsonl");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().with_context(|| format!("{a} wants a value"));
        match a.as_str() {
            "--events" => events = Some(value()?.parse().context("--events is a count")?),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--from" => from = PathBuf::from(value()?),
            _ => bail!("usage: cargo xtask fixture --events N --out DIR [--from PATH]"),
        }
    }
    let (Some(events), Some(out)) = (events, out) else {
        bail!("usage: cargo xtask fixture --events N --out DIR [--from PATH]");
    };
    let template =
        std::fs::read_to_string(&from).with_context(|| format!("reading {}", from.display()))?;
    let lines: Vec<&str> = template.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        bail!("{} has no events", from.display());
    }

    let dir = out.join(".livtk");
    if dir.join("events.jsonl").exists() {
        bail!("{} already has a log; pick an empty folder", out.display());
    }
    std::fs::create_dir_all(&dir)?;
    let src = from.parent().context("the log is in a folder")?;
    for name in ["config.toml", ".gitignore", ".gitattributes"] {
        if src.join(name).exists() {
            std::fs::copy(src.join(name), dir.join(name))?;
        }
    }
    let mut w = std::io::BufWriter::new(std::fs::File::create(dir.join("events.jsonl"))?);
    let mut line = String::new();
    for i in 0..events {
        let g = (i / lines.len()) as u64;
        line.clear();
        remap(lines[i % lines.len()], g, &mut line);
        w.write_all(line.as_bytes())?;
        w.write_all(b"\n")?;
    }
    w.flush()?;
    let generations = events.div_ceil(lines.len());
    println!(
        "wrote {events} events ({generations} generation(s) of {}'s {}) to {}",
        from.display(),
        lines.len(),
        dir.join("events.jsonl").display()
    );
    Ok(())
}

/// `line` as generation `g` has it: each uuid's first eight hex digits xored with `g` (so every
/// id stays unique, and stays the same id wherever it is named), and each `"key":"PRE-n"` moved
/// up by `g` steps. Generation 0 is the line as it is.
fn remap(line: &str, g: u64, out: &mut String) {
    if g == 0 {
        out.push_str(line);
        return;
    }
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if is_uuid(&b[i..]) {
            // A run of hex is ASCII, so these are char boundaries.
            let head = u32::from_str_radix(&line[i..i + 8], 16).unwrap_or(0);
            let _ = write!(out, "{:08x}", head ^ g as u32);
            out.push_str(&line[i + 8..i + 36]);
            i += 36;
        } else if b[i..].starts_with(b"\"key\":\"") {
            let start = i + 7;
            let dash = line[start..].find('-').map(|d| start + d);
            let end = line[start..].find('"').map(|e| start + e);
            match (dash, end) {
                (Some(d), Some(e)) if d < e => match line[d + 1..e].parse::<u64>() {
                    Ok(n) => {
                        let _ = write!(out, "{}{}", &line[i..=d], n + g * KEY_STEP);
                        i = e;
                    }
                    Err(_) => {
                        out.push_str(&line[i..start]);
                        i = start;
                    }
                },
                _ => {
                    out.push_str(&line[i..start]);
                    i = start;
                }
            }
        } else {
            let ch = line[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
}

/// Whether `b` starts with a lowercase uuid: 8-4-4-4-12 hex digits.
fn is_uuid(b: &[u8]) -> bool {
    if b.len() < 36 {
        return false;
    }
    b[..36].iter().enumerate().all(|(i, c)| match i {
        8 | 13 | 18 | 23 => *c == b'-',
        _ => c.is_ascii_digit() || (b'a'..=b'f').contains(c),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"id":"01a143ef-3009-7258-bd22-b6bc84fdbf47","op":"node.create","node":"01a143ef-3008-7301-8cb9-e73b0570a24f","data":{"key":"TAS-12","title":"ü"}}"#;

    #[test]
    fn a_generation_has_its_own_ids_and_keys_and_the_rest_as_it_was() {
        let mut first = String::new();
        remap(LINE, 0, &mut first);
        assert_eq!(first, LINE);

        let mut second = String::new();
        remap(LINE, 2, &mut second);
        assert_eq!(
            second,
            r#"{"id":"01a143ed-3009-7258-bd22-b6bc84fdbf47","op":"node.create","node":"01a143ed-3008-7301-8cb9-e73b0570a24f","data":{"key":"TAS-200012","title":"ü"}}"#
        );
    }
}
