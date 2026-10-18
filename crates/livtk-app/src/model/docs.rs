//! The Docs view: the project's docs as a tree (folders open and shut), the doc open, read from
//! disk, and the nodes linked to it. Read again whenever the view loads, so what changed on disk
//! shows. Links between nodes and docs go through the log (`Command::DocLink`).

use std::collections::HashSet;

use livtk_core::docs::Folder;
use livtk_core::{Command, Node};

use super::{Model, View};

/// What the view shows.
#[derive(Clone, Debug, Default)]
pub struct Docs {
    pub tree: Folder,
    /// The doc open, by path, and its text; `None` when it is gone from disk.
    pub open: Option<String>,
    pub text: Option<String>,
    /// The nodes linked to the open doc.
    pub linked: Vec<Node>,
    /// Folders shut, by path.
    pub shut: HashSet<String>,
    /// The tree's highlighted row.
    pub index: usize,
    /// The docs' folder (for the empty view), and the project's key prefix (for keys in docs).
    pub dir: String,
    pub prefix: String,
    /// What the docs' files looked like when last read: their paths, sizes and times.
    stamp: u64,
    /// The open doc laid out as the reader shows it: its title and its sections.
    pub page: Page,
    /// When the open doc last changed on disk.
    pub modified: Option<std::time::SystemTime>,
    /// The docs gone back from, and forward to again.
    pub back: Vec<String>,
    pub forward: Vec<String>,
    /// A heading to show once the doc is drawn (a link to `doc.md#heading`), by its slug.
    pub anchor: Option<String>,
    /// What else points at the open doc: the docs that link to it, the nodes that mention it
    /// (without being linked to it).
    pub backlinks: Vec<livtk_core::docs::Doc>,
    pub mentions: Vec<Node>,
    /// What the open doc says of itself in its frontmatter.
    pub front: Front,
    /// A search across the docs, and what it found.
    pub query: String,
    pub results: Vec<livtk_core::docs::Found>,
}

/// A doc's frontmatter, the YAML block at its very top between `---` lines: its title, its
/// status and its tags, the rest left aside.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Front {
    pub title: Option<String>,
    pub status: Option<String>,
    pub tags: Vec<String>,
}

/// `text`'s frontmatter, and the text after it. Without one, nothing and the text as it is.
pub fn front(text: &str) -> (Front, &str) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (Front::default(), text);
    };
    let Some(end) = rest.find("\n---") else {
        return (Front::default(), text);
    };
    let block = &rest[..end];
    let after = rest[end + 4..].trim_start_matches(['\r', '\n']);
    let mut front = Front::default();
    let mut lines = block.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = unquote(value.trim());
        match key.trim() {
            "title" if !value.is_empty() => front.title = Some(value.to_owned()),
            "status" if !value.is_empty() => front.status = Some(value.to_owned()),
            "tags" => {
                if let Some(list) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                    front.tags = list
                        .split(',')
                        .map(|t| unquote(t.trim()).to_owned())
                        .filter(|t| !t.is_empty())
                        .collect();
                } else if !value.is_empty() {
                    front.tags = value
                        .split(',')
                        .map(|t| unquote(t.trim()).to_owned())
                        .collect();
                } else {
                    // A list, an item a line: `  - one`.
                    while let Some(item) = lines.peek().and_then(|l| l.trim().strip_prefix("- ")) {
                        front.tags.push(unquote(item.trim()).to_owned());
                        lines.next();
                    }
                }
            }
            _ => {}
        }
    }
    (front, after)
}

fn unquote(s: &str) -> &str {
    s.trim_matches(|c| c == '"' || c == '\'')
}

/// `text` with every image a doc shows from a path of its own (`![alt](img/a.png)`) made whole
/// from `dir`, the doc's folder, so it loads wherever the app runs; web images stay as they are.
pub fn local_images(text: &str, dir: &std::path::Path) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        let fence = t.starts_with("```") || t.starts_with("~~~");
        if fence {
            fenced = !fenced;
        }
        if fence || fenced {
            out.push_str(line);
        } else {
            out.push_str(&images_in_line(line, dir));
        }
    }
    out
}

fn images_in_line(text: &str, dir: &std::path::Path) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("![") {
        let Some(close) = rest[at..].find("](").map(|i| at + i + 2) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|i| close + i) else {
            break;
        };
        let dest = rest[close..end].trim();
        let (file, title) = match dest.split_once(char::is_whitespace) {
            Some((f, t)) => (f, Some(t)),
            None => (dest, None),
        };
        out.push_str(&rest[..close]);
        let local = !file.contains("://") && !file.starts_with('/') && !file.starts_with("data:");
        if local && !file.is_empty() {
            out.push_str(&dir.join(file.trim_matches(['<', '>'])).to_string_lossy());
            if let Some(t) = title {
                out.push(' ');
                out.push_str(t);
            }
        } else {
            out.push_str(dest);
        }
        out.push(')');
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A doc as the reader lays it out: its title (its first heading, when that is a level-one
/// heading at its top), then its sections, one at each heading of levels one to three, the
/// first without one when text comes before the first heading.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub title: Option<String>,
    pub sections: Vec<Section>,
}

/// A part of a doc: its heading, and its markdown (the heading's own line included).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub heading: Option<Heading>,
    pub text: String,
}

/// A heading: its level, its text, and its slug, as `#slug` links name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub slug: String,
}

impl Page {
    /// The section a `#slug` link goes to.
    pub fn section_of(&self, slug: &str) -> Option<usize> {
        self.sections
            .iter()
            .position(|s| s.heading.as_ref().is_some_and(|h| h.slug == slug))
    }
}

/// `text` laid out as a page. A section's markdown carries the doc's link reference
/// definitions too, so a reference link works in whichever section it is.
pub fn page(text: &str) -> Page {
    let mut title = None;
    let mut sections: Vec<Section> = Vec::new();
    let mut current = Section {
        heading: None,
        text: String::new(),
    };
    let mut definitions = String::new();
    let mut slugs: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut fenced = false;
    let mut seen_text = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
        }
        if !fenced && t.starts_with('[') && t.contains("]:") {
            definitions.push_str(line);
            continue;
        }
        let heading = (!fenced && !line.starts_with("    "))
            .then(|| heading_of(t))
            .flatten()
            .filter(|(level, _)| *level <= 3);
        match heading {
            Some((1, h)) if title.is_none() && !seen_text && sections.is_empty() => {
                title = Some(h);
            }
            Some((level, h)) => {
                if !current.text.trim().is_empty() || current.heading.is_some() {
                    sections.push(current);
                }
                let base = slug(&h);
                let n = slugs.entry(base.clone()).or_insert(0);
                let slug = if *n == 0 {
                    base.clone()
                } else {
                    format!("{base}-{n}")
                };
                *n += 1;
                current = Section {
                    heading: Some(Heading {
                        level,
                        title: h,
                        slug,
                    }),
                    text: line.to_owned(),
                };
                seen_text = true;
            }
            None => {
                if !line.trim().is_empty() {
                    seen_text = true;
                }
                current.text.push_str(line);
            }
        }
    }
    if !current.text.trim().is_empty() || current.heading.is_some() {
        sections.push(current);
    }
    if !definitions.is_empty() {
        for s in &mut sections {
            s.text.push_str("\n\n");
            s.text.push_str(&definitions);
        }
    }
    Page { title, sections }
}

/// A heading line's level and text: `## Setup ##` is (2, "Setup").
fn heading_of(line: &str) -> Option<(u8, String)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) || !line[hashes..].starts_with([' ', '\t']) {
        return None;
    }
    livtk_core::docs::title_of(line).map(|t| (u8::try_from(hashes).unwrap_or(6), t))
}

/// A heading's slug, as GitHub makes it: lower case, spaces as `-`, punctuation dropped.
pub fn slug(title: &str) -> String {
    title
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// Where `query` is in `text`, case aside: the byte ranges, in order.
pub fn find_in(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let lower = text.to_lowercase();
    let q = query.to_lowercase();
    // Lower case can change a character's length; only text that keeps it is searched as is.
    if lower.len() != text.len() {
        return text
            .match_indices(query)
            .map(|(i, m)| i..i + m.len())
            .collect();
    }
    lower
        .match_indices(&q)
        .map(|(i, m)| i..i + m.len())
        .collect()
}

/// A row of the tree as shown: a folder (open or shut) or a doc, each at its depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocRow {
    Folder {
        path: String,
        name: String,
        depth: usize,
        open: bool,
    },
    Doc {
        path: String,
        title: String,
        depth: usize,
    },
}

impl DocRow {
    pub fn path(&self) -> &str {
        match self {
            DocRow::Folder { path, .. } | DocRow::Doc { path, .. } => path,
        }
    }
}

impl Docs {
    /// The tree's rows as shown: a shut folder's contents left out.
    pub fn rows(&self) -> Vec<DocRow> {
        let mut out = Vec::new();
        self.push_rows(&self.tree, 0, &mut out);
        out
    }

    fn push_rows(&self, folder: &Folder, depth: usize, out: &mut Vec<DocRow>) {
        for f in &folder.folders {
            let open = !self.shut.contains(&f.path);
            out.push(DocRow::Folder {
                path: f.path.clone(),
                name: f.name.clone(),
                depth,
                open,
            });
            if open {
                self.push_rows(f, depth + 1, out);
            }
        }
        for d in &folder.docs {
            out.push(DocRow::Doc {
                path: d.path.clone(),
                title: d.title.clone(),
                depth,
            });
        }
    }

    /// The open doc's title, as the tree names it.
    pub fn title_of(&self, path: &str) -> Option<&str> {
        self.tree
            .all()
            .into_iter()
            .find(|d| d.path == path)
            .map(|d| d.title.as_str())
    }
}

impl Model {
    /// Read the docs again: the tree, the open doc (the one remembered for the project, else
    /// the first), its text and what is linked to it.
    pub(super) fn load_docs(&mut self) {
        let Some(p) = self.projects.get(self.current) else {
            self.docs = Docs::default();
            return;
        };
        let tree = p.ws.docs();
        // The doc remembered, while it is there; else the first.
        let open = self
            .docs_open
            .get(&p.dir)
            .filter(|path| p.ws.root().join(path).is_file())
            .cloned()
            .or_else(|| tree.all().first().map(|d| d.path.clone()));
        let text = open
            .as_ref()
            .and_then(|path| std::fs::read_to_string(p.ws.root().join(path)).ok());
        let linked = open
            .as_ref()
            .map(|path| p.ws.doc_nodes(path).unwrap_or_default())
            .unwrap_or_default();
        self.docs.stamp = stamp(p.ws.root(), &p.ws.docs_dir(), open.as_deref());
        self.docs.modified = open
            .as_ref()
            .and_then(|path| std::fs::metadata(p.ws.root().join(path)).ok())
            .and_then(|m| m.modified().ok());
        let (front, body) = text.as_deref().map(front).unwrap_or_default();
        self.docs.front = front;
        let folder = open
            .as_ref()
            .and_then(|path| {
                p.ws.root()
                    .join(path)
                    .parent()
                    .map(std::path::Path::to_path_buf)
            })
            .unwrap_or_else(|| p.ws.root().to_path_buf());
        self.docs.page = page(&local_images(
            &link_keys(body, &p.ws.config().prefix),
            &folder,
        ));
        self.docs.backlinks = open
            .as_ref()
            .map(|path| {
                let all = tree.all();
                livtk_core::docs::linking_to(p.ws.root(), &all, path)
                    .into_iter()
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        self.docs.mentions = open
            .as_ref()
            .map(|path| p.ws.doc_mentions(path).unwrap_or_default())
            .unwrap_or_default();
        if !self.docs.query.is_empty() {
            self.docs.results =
                livtk_core::docs::search(p.ws.root(), &tree.all(), &self.docs.query);
        }
        self.docs.dir = p.ws.docs_dir();
        self.docs.prefix = p.ws.config().prefix.clone();
        self.docs.tree = tree;
        self.docs.open = open;
        self.docs.text = text;
        self.docs.linked = linked;
        let rows = self.docs.rows();
        self.docs.index = self.docs.index.min(rows.len().saturating_sub(1));
    }

    /// Open the doc at `path`, in the Docs view, the one open before it kept to go back to.
    pub fn open_doc(&mut self, path: &str) {
        let (path, anchor) = match path.split_once('#') {
            Some((p, a)) => (p, Some(a.to_owned())),
            None => (path, None),
        };
        if let Some(was) = self.docs.open.clone()
            && was != path
            && self.view == View::Docs
        {
            self.docs.back.push(was);
            self.docs.forward.clear();
        }
        self.docs.anchor = anchor;
        self.show_doc(path);
    }

    /// Back to the doc open before: the one left kept to go forward to.
    pub fn doc_back(&mut self) {
        if let Some(to) = self.docs.back.pop() {
            if let Some(was) = self.docs.open.clone() {
                self.docs.forward.push(was);
            }
            self.show_doc(&to);
        }
    }

    pub fn doc_forward(&mut self) {
        if let Some(to) = self.docs.forward.pop() {
            if let Some(was) = self.docs.open.clone() {
                self.docs.back.push(was);
            }
            self.show_doc(&to);
        }
    }

    /// The docs before and after the open one, in the tree's order.
    pub fn doc_neighbours(
        &self,
    ) -> (
        Option<&livtk_core::docs::Doc>,
        Option<&livtk_core::docs::Doc>,
    ) {
        let all = self.docs.tree.all();
        let Some(i) = all
            .iter()
            .position(|d| Some(&d.path) == self.docs.open.as_ref())
        else {
            return (None, None);
        };
        (
            i.checked_sub(1).and_then(|j| all.get(j).copied()),
            all.get(i + 1).copied(),
        )
    }

    /// Show the doc at `path`, as it is, outside the history.
    fn show_doc(&mut self, path: &str) {
        if let Some(p) = self.projects.get(self.current) {
            self.docs_open.insert(p.dir.clone(), path.to_owned());
        }
        // Its folders open, so the tree shows it, highlighted.
        let mut at = path;
        while let Some((parent, _)) = at.rsplit_once('/') {
            self.docs.shut.remove(parent);
            at = parent;
        }
        if self.view == View::Docs {
            self.load_docs();
        } else {
            self.set_view(View::Docs);
        }
        if let Some(i) = self.docs.rows().iter().position(|r| r.path() == path) {
            self.docs.index = i;
        }
    }

    /// In the Docs view, read the docs again if a file under them changed, came or went: by
    /// their sizes and times alone, so looking costs no reading. Whether it did.
    pub fn refresh_docs(&mut self) -> bool {
        if self.view != View::Docs {
            return false;
        }
        let Some(p) = self.projects.get(self.current) else {
            return false;
        };
        if stamp(p.ws.root(), &p.ws.docs_dir(), self.docs.open.as_deref()) == self.docs.stamp {
            return false;
        }
        self.load_docs();
        true
    }

    /// Search the docs' titles and text for `query`; empty, the tree again.
    pub fn search_docs(&mut self, query: &str) {
        self.docs.query = query.to_owned();
        self.docs.results = match self.projects.get(self.current) {
            Some(p) if !query.trim().is_empty() => {
                livtk_core::docs::search(p.ws.root(), &self.docs.tree.all(), query)
            }
            _ => Vec::new(),
        };
    }

    pub(super) fn docs_select_by(&mut self, delta: isize) {
        let n = self.docs.rows().len();
        if n > 0 {
            self.docs.index = self.docs.index.saturating_add_signed(delta).min(n - 1);
        }
    }

    /// Enter on the tree: a doc opens, a folder opens or shuts.
    pub(super) fn docs_enter(&mut self) {
        match self.docs.rows().get(self.docs.index).cloned() {
            Some(DocRow::Doc { path, .. }) => self.open_doc(&path),
            Some(DocRow::Folder { path, .. }) => self.toggle_doc_folder(&path),
            None => {}
        }
    }

    pub fn toggle_doc_folder(&mut self, path: &str) {
        if !self.docs.shut.remove(path) {
            self.docs.shut.insert(path.to_owned());
        }
    }

    /// Link the node keyed `key` to the doc at `path`, or take the link away.
    pub fn link_doc(&mut self, key: &str, path: &str, link: bool) {
        let cmd = if link {
            Command::DocLink {
                node: key.to_owned(),
                path: path.to_owned(),
            }
        } else {
            Command::DocUnlink {
                node: key.to_owned(),
                path: path.to_owned(),
            }
        };
        self.run(cmd, None);
        if self.view == View::Docs {
            self.load_docs();
        }
    }
}

/// `text` with every bare node key of the project's `prefix` (`TAS-12`) made a link to it
/// (`livtk:TAS-12`), so a click opens the node. Keys stay as they are where a link would break
/// the markdown or mean nothing: in code (fenced, indented or inline), in a link's text or
/// destination, in an autolink or a URL, and in a link's reference definition.
pub fn link_keys(text: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        let fence = t.starts_with("```") || t.starts_with("~~~");
        if fence {
            fenced = !fenced;
        }
        let indented = line.starts_with("    ") || line.starts_with('\t');
        let definition = t.starts_with('[') && t.contains("]:");
        if fence || fenced || indented || definition {
            out.push_str(line);
            continue;
        }
        link_line(line, prefix, &mut out);
    }
    out
}

fn link_line(line: &str, prefix: &str, out: &mut String) {
    let head = format!("{prefix}-");
    let bytes = line.as_bytes();
    // Where the scan is: in inline code, in a link's text, its destination, or `<…>`.
    let (mut code, mut text, mut dest, mut angle) = (false, 0usize, 0usize, false);
    // The start of the word the scan is in, to tell a URL.
    let mut word = 0;
    let mut i = 0;
    while i < line.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            word = i + 1;
        }
        if !code {
            match c {
                b'[' if dest == 0 => text += 1,
                b']' if dest == 0 && text > 0 => {
                    text -= 1;
                    if bytes.get(i + 1) == Some(&b'(') {
                        dest = 1;
                        out.push_str("](");
                        i += 2;
                        continue;
                    }
                }
                b'(' if dest > 0 => dest += 1,
                b')' if dest > 0 => dest -= 1,
                b'<' if dest == 0 => angle = true,
                b'>' => angle = false,
                _ => {}
            }
        }
        if c == b'`' {
            code = !code;
        }
        let free = !code && text == 0 && dest == 0 && !angle;
        let before = &line[word.min(i)..i];
        let in_url = before.contains("://") || before.starts_with("www.");
        let starts_key = free
            && !in_url
            && line[i..].starts_with(&head)
            && (i == 0 || {
                let before = bytes[i - 1];
                !(before.is_ascii_alphanumeric()
                    || matches!(
                        before,
                        b'/' | b'-' | b':' | b'=' | b'?' | b'&' | b'#' | b'.' | b'_'
                    ))
            });
        if starts_key {
            let digits = line[i + head.len()..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            let end = i + head.len() + digits;
            let after = bytes.get(end).copied();
            if digits > 0 && !after.is_some_and(|a| a.is_ascii_alphanumeric() || a == b'-') {
                let key = &line[i..end];
                out.push_str(&format!("[{key}](livtk:{key})"));
                i = end;
                continue;
            }
        }
        // Whole characters, so text past ASCII is copied as it is.
        let ch = line[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&line[i..i + ch]);
        i += ch;
    }
}

impl Model {
    /// Open the node keyed `key` in the detail panel, beside the doc (a link in it, or a node
    /// linked to it, clicked).
    pub fn show_node(&mut self, key: &str) {
        let Some(id) = self
            .projects
            .get(self.current)
            .and_then(|p| p.ws.node(key).ok())
            .map(|n| n.id)
        else {
            self.warn(format!("{key} is not a node of this project"));
            return;
        };
        self.selected = Some(id);
        self.open_detail();
    }
}

/// The docs under `dir` (from the project's folder `root`), and the doc `open` wherever it is,
/// as a number that changes when one does: each one's path, size and time. Hidden files and
/// folders are passed over, as the tree passes over them.
fn stamp(root: &std::path::Path, dir: &str, open: Option<&str>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut stack = vec![root.join(dir)];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut entries: Vec<_> = entries
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let Ok(meta) = e.metadata() else {
                continue;
            };
            e.path().hash(&mut h);
            if meta.is_dir() {
                stack.push(e.path());
            } else {
                meta.len().hash(&mut h);
                meta.modified().ok().hash(&mut h);
            }
        }
    }
    if let Some(path) = open
        && let Ok(meta) = std::fs::metadata(root.join(path))
    {
        path.hash(&mut h);
        meta.len().hash(&mut h);
        meta.modified().ok().hash(&mut h);
    }
    h.finish()
}

/// A callout's kind, as GitHub's alerts name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalloutKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl CalloutKind {
    pub fn name(self) -> &'static str {
        match self {
            CalloutKind::Note => "Note",
            CalloutKind::Tip => "Tip",
            CalloutKind::Important => "Important",
            CalloutKind::Warning => "Warning",
            CalloutKind::Caution => "Caution",
        }
    }
}

/// A callout read from a blockquote's markdown (`> [!NOTE] Optional title`, then its lines):
/// its kind, its own title if it gives one, and its body. `None` for a plain quote.
pub fn callout(quote: &str) -> Option<(CalloutKind, Option<String>, String)> {
    let mut lines = quote.lines().map(|l| {
        let l = l.trim_start();
        let l = l.strip_prefix('>').unwrap_or(l);
        l.strip_prefix(' ').unwrap_or(l)
    });
    let first = lines.next()?.trim();
    let marker = first.strip_prefix("[!")?;
    let (kind, after) = marker.split_once(']')?;
    let kind = match kind.to_ascii_uppercase().as_str() {
        "NOTE" => CalloutKind::Note,
        "TIP" => CalloutKind::Tip,
        "IMPORTANT" => CalloutKind::Important,
        "WARNING" => CalloutKind::Warning,
        "CAUTION" => CalloutKind::Caution,
        _ => return None,
    };
    let title = Some(after.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    let body: Vec<&str> = lines.collect();
    Some((kind, title, body.join("\n").trim().to_owned()))
}
