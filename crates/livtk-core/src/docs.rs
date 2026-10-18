//! The project's docs: the markdown files under one folder of it (`docs/`, unless its config
//! names another), read as a tree; each named by its first heading. A doc is known by its path
//! from the project's folder, `/` between the parts (`docs/guide/setup.md`): how the log links a
//! node to it, and how a link inside a doc reaches another.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// The folder docs are read from when the config names none.
pub const DEFAULT_DIR: &str = "docs";

/// A folder of docs: its name, its path, the folders in it, then its docs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub path: String,
    pub folders: Vec<Folder>,
    pub docs: Vec<Doc>,
}

/// A doc: its path from the project's folder, and what it is called (its first heading, else
/// its file's name without `.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doc {
    pub path: String,
    pub title: String,
}

impl Folder {
    /// Every doc in it, its folders' first, in the order the tree shows them.
    pub fn all(&self) -> Vec<&Doc> {
        let mut out: Vec<&Doc> = Vec::new();
        for f in &self.folders {
            out.extend(f.all());
        }
        out.extend(&self.docs);
        out
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty() && self.folders.iter().all(Folder::is_empty)
    }
}

/// The docs under `dir` (a path from the project's folder `root`): folders first, then docs; a
/// folder's README or index first among its docs; the rest by name, case aside. Hidden folders
/// and files, and folders with no docs in them, are left out.
pub fn tree(root: &Path, dir: &str) -> Folder {
    let mut top = read_folder(root, dir);
    top.name = dir.to_owned();
    top
}

fn read_folder(root: &Path, path: &str) -> Folder {
    let mut folder = Folder {
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        path: path.to_owned(),
        ..Folder::default()
    };
    let Ok(entries) = fs::read_dir(root.join(path)) else {
        return folder;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let child = format!("{path}/{name}");
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            let sub = read_folder(root, &child);
            if !sub.is_empty() {
                folder.folders.push(sub);
            }
        } else if is_doc(&name) {
            let text = fs::read_to_string(entry.path()).unwrap_or_default();
            folder.docs.push(Doc {
                title: title_of(&text).unwrap_or_else(|| stem(&name).to_owned()),
                path: child,
            });
        }
    }
    folder.folders.sort_by_key(|f| f.name.to_lowercase());
    folder.docs.sort_by_key(|d| {
        let name = d.path.rsplit('/').next().unwrap_or_default().to_lowercase();
        let first = matches!(stem(&name), "readme" | "index");
        (!first, name)
    });
    folder
}

/// Whether a file is a doc: markdown.
pub fn is_doc(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

/// A doc's first heading (`# Title`, any level), outside code blocks.
pub fn title_of(text: &str) -> Option<String> {
    let mut fenced = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let hashes = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
            let title = t[hashes..].trim().trim_end_matches('#').trim();
            if !title.is_empty() {
                return Some(title.to_owned());
            }
        }
    }
    None
}

/// `path`, given relative to the project's folder `root` (or inside it, absolute), as a doc's
/// path: `/` between the parts, no `.` or `..` left. `None` if it leads out of the project.
pub fn normalize(root: &Path, path: &str) -> Option<String> {
    let p = Path::new(path);
    let relative: PathBuf = if p.is_absolute() {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        p.strip_prefix(&root).ok()?.to_path_buf()
    } else {
        p.to_path_buf()
    };
    let mut parts: Vec<String> = Vec::new();
    for c in relative.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Where a link inside a doc leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Another doc (by its path), and the heading anchored to, if any.
    Doc {
        path: String,
        anchor: Option<String>,
    },
    /// A heading in the same doc.
    Anchor(String),
    /// A page on the web, or anything else a browser opens.
    Web(String),
}

/// What `href`, written in the doc at `from`, leads to.
pub fn resolve(from: &str, href: &str) -> Option<Target> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    if let Some(anchor) = href.strip_prefix('#') {
        return Some(Target::Anchor(anchor.to_owned()));
    }
    if href.contains("://") || href.starts_with("mailto:") {
        return Some(Target::Web(href.to_owned()));
    }
    let (file, anchor) = match href.split_once('#') {
        Some((f, a)) => (f, Some(a.to_owned())),
        None => (href, None),
    };
    // From the project's folder when it starts with `/`, else from the doc's own folder.
    let base = match file.strip_prefix('/') {
        Some(_) => String::new(),
        None => from
            .rsplit_once('/')
            .map_or(String::new(), |(d, _)| d.to_owned()),
    };
    let joined = if base.is_empty() {
        file.trim_start_matches('/').to_owned()
    } else {
        format!("{base}/{file}")
    };
    let path = normalize(Path::new(""), &joined)?;
    Some(Target::Doc { path, anchor })
}
