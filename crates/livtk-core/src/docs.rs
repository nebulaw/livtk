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
            let title = t[hashes..].trim();
            // A closing run of `#` goes only when a space comes before it: `# Using C#` keeps it.
            let bare = title.trim_end_matches('#');
            let title = if bare.len() < title.len() && bare.ends_with(char::is_whitespace) {
                bare.trim()
            } else {
                title
            };
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
        let root = real(root);
        real(p).strip_prefix(&root).ok()?.to_path_buf()
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

/// `p` with its links resolved as far as it exists: the part that is there made real, the rest
/// (a file gone, say) joined on as written. So a path through a linked folder (`/tmp`, which is
/// `/private/tmp`) matches its project's however much of it is left.
fn real(p: &Path) -> PathBuf {
    if let Ok(r) = p.canonicalize() {
        return r;
    }
    match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) if parent != p => real(parent).join(name),
        _ => p.to_path_buf(),
    }
}

/// `%20` and its kind decoded, as a link's destination writes what a path holds.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(&a), Some(&b)) = (bytes.get(i + 1), bytes.get(i + 2))
            && let (Some(h), Some(l)) = (hex(a), hex(b))
        {
            out.push(u8::try_from(h * 16 + l).unwrap_or(b'?'));
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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
    // A query means nothing to a file; spaces and the like come encoded.
    let file = decode(file.split_once('?').map_or(file, |(f, _)| f));
    let file = file.as_str();
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

/// Every link a doc's markdown makes, as written: `[text](href)`, `<href>`, and reference
/// definitions (`[name]: href`). Links in code are left out.
pub fn links_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.starts_with("    ") {
            continue;
        }
        // `[name]: href`
        if t.starts_with('[')
            && let Some((_, rest)) = t.split_once("]:")
        {
            if let Some(href) = rest.split_whitespace().next() {
                out.push(href.trim_matches(['<', '>']).to_owned());
            }
            continue;
        }
        let mut code = false;
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'`' => code = !code,
                // `<https://…>` or `<other.md>`, not an HTML tag.
                b'<' if !code => {
                    if let Some(end) = line[i + 1..].find('>') {
                        let inner = &line[i + 1..i + 1 + end];
                        if !inner.contains(char::is_whitespace)
                            && (inner.contains("://")
                                || is_doc(inner.split('#').next().unwrap_or("")))
                        {
                            out.push(inner.to_owned());
                        }
                        i += end + 1;
                    }
                }
                b']' if !code && bytes.get(i + 1) == Some(&b'(') => {
                    let start = i + 2;
                    let mut depth = 1;
                    let mut j = start;
                    while j < bytes.len() {
                        match bytes[j] {
                            b'(' => depth += 1,
                            b')' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                    // The destination, without a title after it (`(href "title")`).
                    let dest = line[start..j.min(line.len())].trim();
                    let href = dest.split_whitespace().next().unwrap_or_default();
                    if !href.is_empty() {
                        out.push(href.trim_matches(['<', '>']).to_owned());
                    }
                    i = j;
                }
                _ => {}
            }
            i += 1;
        }
    }
    out
}

/// The docs among `docs` (paths, from `root`) that link to the doc at `path`.
pub fn linking_to<'a>(root: &Path, docs: &[&'a Doc], path: &str) -> Vec<&'a Doc> {
    docs.iter()
        .copied()
        .filter(|d| d.path != path)
        .filter(|d| {
            let text = fs::read_to_string(root.join(&d.path)).unwrap_or_default();
            links_in(&text).iter().any(|href| {
                matches!(resolve(&d.path, href), Some(Target::Doc { path: p, .. }) if p == path)
            })
        })
        .collect()
}

/// A doc a search found: which, and up to three of its lines with the query in them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub path: String,
    pub title: String,
    pub lines: Vec<String>,
}

/// The docs among `docs` whose title or text holds `query`, case aside: those it names first,
/// then the rest, each with the lines it is in.
pub fn search(root: &Path, docs: &[&Doc], query: &str) -> Vec<Found> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut named = Vec::new();
    let mut rest = Vec::new();
    for d in docs {
        let text = fs::read_to_string(root.join(&d.path)).unwrap_or_default();
        let lines: Vec<String> = text
            .lines()
            .filter(|l| l.to_lowercase().contains(&q))
            .map(|l| l.trim().trim_start_matches('#').trim().to_owned())
            .filter(|l| !l.is_empty())
            .take(3)
            .collect();
        let in_title = d.title.to_lowercase().contains(&q) || d.path.to_lowercase().contains(&q);
        if !in_title && lines.is_empty() {
            continue;
        }
        let found = Found {
            path: d.path.clone(),
            title: d.title.clone(),
            lines,
        };
        if in_title {
            named.push(found);
        } else {
            rest.push(found);
        }
    }
    named.extend(rest);
    named
}
