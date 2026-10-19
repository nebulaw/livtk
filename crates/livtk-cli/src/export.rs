//! `livtk export`: every node written as a markdown file, for a developer who wants their work
//! greppable and diffable on disk. A copy derived from the log, never read back: editing an
//! exported file changes nothing in livtk. It writes what this client can read, wherever the log
//! lives, so it stays an export when the log moves to a server.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use livtk_core::event::Op;
use livtk_core::{Kind, Rel, Workspace};

/// The default folder: inside `.livtk/`, kept out of git by a `.gitignore` of its own.
pub const DEFAULT_DIR: &str = ".livtk/export";

/// Write every node under `dir` (from the project's folder when relative), only the files whose
/// text changed; with `check`, write nothing and fail if any would change.
pub fn run(ws: &Workspace, dir: Option<&Path>, check: bool) -> Result<()> {
    let root = ws.dir().parent().map(Path::to_path_buf).unwrap_or_default();
    let (dir, own) = match dir {
        Some(d) if d.is_absolute() => (d.to_path_buf(), false),
        Some(d) => (std::env::current_dir()?.join(d), false),
        None => (root.join(DEFAULT_DIR), true),
    };
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    if own {
        files.push((
            dir.join(".gitignore"),
            "# livtk export: derived, never the source of truth\n*\n".into(),
        ));
    }
    files.push((dir.join("README.md"), README.into()));
    for kind in [Kind::Task, Kind::Lesson, Kind::Decision, Kind::Finding] {
        for s in ws.list(kind, &[])? {
            files.push((
                dir.join(format!("{kind}s")).join(format!("{}.md", s.key)),
                page(ws, &s.key)?,
            ));
        }
    }
    let stale: Vec<&PathBuf> = files
        .iter()
        .filter(|(path, text)| fs::read_to_string(path).ok().as_deref() != Some(text.as_str()))
        .map(|(path, _)| path)
        .collect();
    if check {
        if stale.is_empty() {
            println!("{} is current", dir.display());
            return Ok(());
        }
        bail!(
            "{} is stale: {} file(s) differ from the log; run `livtk export` again",
            dir.display(),
            stale.len()
        );
    }
    let written = stale.len();
    for (path, text) in &files {
        if !stale.contains(&path) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
    }
    println!(
        "exported {} node(s) to {} ({written} file(s) written)",
        files.len() - 1 - usize::from(own),
        dir.display()
    );
    Ok(())
}

const README: &str = "# livtk export\n\n\
This folder is a copy of the project's livtk nodes, written by `livtk export`. It is derived \
from the event log and never read back: editing a file here changes nothing in livtk. Change \
work with the `livtk` CLI or the app, then export again.\n";

/// A node as a markdown page: its fields as frontmatter, its body, its links and its notes.
fn page(ws: &Workspace, key: &str) -> Result<String> {
    let n = ws.node(key)?;
    let mut out = String::from("---\n");
    let field = |out: &mut String, name: &str, value: &str| {
        let _ = writeln!(out, "{name}: {}", yaml_str(value));
    };
    field(&mut out, "key", &n.key);
    field(&mut out, "kind", n.kind.as_str());
    field(&mut out, "title", &n.title);
    field(&mut out, "state", n.state.as_str());
    if n.kind == Kind::Task {
        let _ = writeln!(out, "priority: {}", n.priority);
    }
    field(&mut out, "created", &n.created);
    field(&mut out, "created_by", &n.created_by);
    if !n.props.is_empty() {
        let _ = writeln!(out, "props: {}", serde_json::to_string(&n.props)?);
    }
    out.push_str("---\n\n");
    let _ = writeln!(out, "# {}\n", n.title);
    if !n.body.trim().is_empty() {
        out.push_str(n.body.trim_end());
        out.push_str("\n\n");
    }
    let links = ws.links(key)?;
    let mut groups = vec![("Waits for", links.depends_on), ("Blocks", links.blocks)];
    for (rel, out_label, in_label) in [
        (Rel::Parent, "Part of", "Parts"),
        (Rel::Relates, "Related to", "Related from"),
        (Rel::Supersedes, "Replaces", "Replaced by"),
        (Rel::LearnedFrom, "Learned from", "Taught"),
    ] {
        let (to, from) = ws.related(key, rel)?;
        groups.push((out_label, to));
        groups.push((in_label, from));
    }
    let groups: Vec<_> = groups.into_iter().filter(|(_, b)| !b.is_empty()).collect();
    let docs = ws.docs_of(key)?;
    if !groups.is_empty() || !docs.is_empty() {
        out.push_str("## Links\n\n");
        for (label, briefs) in groups {
            for b in briefs {
                let _ = writeln!(out, "- {label}: {} {} ({})", b.key, b.title, b.state);
            }
        }
        for (path, _) in docs {
            let _ = writeln!(out, "- Doc: {path}");
        }
        out.push('\n');
    }
    let notes: Vec<_> = ws
        .history(Some(key))?
        .into_iter()
        .filter_map(|e| match e.op {
            Op::NodeNote { data, .. } => Some((e.at, e.by, data.text)),
            _ => None,
        })
        .collect();
    if !notes.is_empty() {
        out.push_str("## Notes\n\n");
        for (at, by, text) in notes {
            let _ = writeln!(out, "**{at}, {by}**\n\n{}\n", text.trim_end());
        }
    }
    Ok(out.trim_end().to_owned() + "\n")
}

/// A string as YAML reads it back unchanged: quoted, as JSON quotes it.
fn yaml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}
