//! Agent files: the agents a Claude session can run as (livtk's, the project's, the person's),
//! as Claude Code reads them from their markdown files. Read, written and removed here; where
//! Claude Code's folder is, the caller says.

use std::path::{Path, PathBuf};

use crate::run::Stage;

/// The tools an agent may be limited to, as Claude Code names them.
pub const TOOLS: [&str; 10] = [
    "Read",
    "Edit",
    "Write",
    "Bash",
    "Grep",
    "Glob",
    "WebFetch",
    "WebSearch",
    "NotebookEdit",
    "TodoWrite",
];

/// The models an agent may name; none means the session's own.
pub const MODELS: [&str; 3] = ["sonnet", "opus", "haiku"];

/// Where an agent file lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Place {
    /// livtk's own, from its plugin: copied, never changed.
    Livtk,
    /// The project's, in `.claude/agents/`: shared with it.
    Project,
    /// The person's, in `~/.claude/agents/`: theirs in every project.
    Yours,
}

impl Place {
    pub const ALL: [Place; 3] = [Place::Livtk, Place::Project, Place::Yours];

    pub const fn label(self) -> &'static str {
        match self {
            Place::Livtk => "livtk's",
            Place::Project => "This project's",
            Place::Yours => "Yours",
        }
    }

    pub const fn editable(self) -> bool {
        !matches!(self, Place::Livtk)
    }
}

/// An agent: what Claude Code reads from its file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Agent {
    pub name: String,
    pub description: String,
    /// The tools it may use; empty means every tool its session has.
    pub tools: Vec<String>,
    /// `sonnet`, `opus` or `haiku`; none, the session's own.
    pub model: Option<String>,
    /// The stage of a task's work it suits, when its file says (`stage: build`): the one
    /// suggested for that stage.
    pub stage: Option<Stage>,
    /// Its instructions.
    pub body: String,
    /// The frontmatter's other keys (color, permissionMode, hooks…), as written: kept on save.
    pub extra: String,
}

/// An agent and where its file is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentFile {
    pub place: Place,
    pub path: PathBuf,
    pub agent: Agent,
}

/// The folder of a place's agents, for the project at `root`, Claude Code's folder being `home`.
pub fn folder(place: Place, root: &Path, home: &Path) -> Option<PathBuf> {
    match place {
        Place::Project => Some(root.join(".claude/agents")),
        Place::Yours => Some(home.join("agents")),
        Place::Livtk => livtk_plugin(home).map(|p| p.join("agents")),
    }
}

/// Where Claude Code installed livtk's plugin, as its plugin record says.
fn livtk_plugin(home: &Path) -> Option<PathBuf> {
    let record = std::fs::read_to_string(home.join("plugins/installed_plugins.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&record).ok()?;
    v["plugins"]
        .as_object()?
        .iter()
        .find(|(name, _)| name.starts_with("livtk@"))
        .and_then(|(_, installs)| installs.as_array()?.first()?["installPath"].as_str())
        .map(PathBuf::from)
}

/// Every agent the project at `root` can run as, by place, then by name.
pub fn agents(root: &Path, home: &Path) -> Vec<AgentFile> {
    let mut out = Vec::new();
    for place in Place::ALL {
        let Some(dir) = folder(place, root, home) else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut here: Vec<AgentFile> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .filter_map(|path| {
                let text = std::fs::read_to_string(&path).ok()?;
                Some(AgentFile {
                    place,
                    agent: parse(&text)?,
                    path,
                })
            })
            .collect();
        here.sort_by(|a, b| a.agent.name.cmp(&b.agent.name));
        out.extend(here);
    }
    out
}

/// An agent file's text as an agent: `name`, `description`, `tools` and `model` from its
/// frontmatter, in any of the YAML forms Claude Code reads (a plain, quoted or block value; tools
/// as a comma list, a `[flow]` list or one item a line), every other key kept as written, and
/// its body. `None` when it has no frontmatter or no name.
pub fn parse(text: &str) -> Option<Agent> {
    let rest = text.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    let (head, body) = (&rest[..end], &rest[end + 4..]);
    let mut agent = Agent {
        body: body.trim_start_matches(['\r', '\n']).trim_end().to_owned(),
        ..Agent::default()
    };
    let mut extra = Vec::new();
    for (key, lines) in entries(head) {
        let first = lines[0].split_once(':').map_or("", |(_, v)| v).trim();
        let more = &lines[1..];
        match key.as_str() {
            "name" => agent.name = scalar(first, more),
            "description" => agent.description = scalar(first, more),
            "model" => {
                let m = scalar(first, more);
                agent.model = (!m.is_empty() && m != "inherit").then_some(m);
            }
            "tools" => agent.tools = list(first, more),
            "stage" => agent.stage = scalar(first, more).parse().ok(),
            _ => extra.extend(lines.iter().map(|l| (*l).to_owned())),
        }
    }
    agent.extra = extra.join("\n");
    (!agent.name.is_empty()).then_some(agent)
}

/// The frontmatter's keys, each with its lines: the key's own, then those that continue it
/// (indented, or a `- ` item under it).
fn entries(head: &str) -> Vec<(String, Vec<&str>)> {
    let mut out: Vec<(String, Vec<&str>)> = Vec::new();
    for line in head.lines() {
        let continues = line.starts_with([' ', '\t']) || line.starts_with("- ");
        match (continues, out.last_mut()) {
            (true, Some((_, lines))) => lines.push(line),
            _ => {
                if let Some((key, _)) = line.split_once(':')
                    && !key.trim().is_empty()
                    && !key.trim_start().starts_with('#')
                {
                    out.push((key.trim().to_owned(), vec![line]));
                }
            }
        }
    }
    out
}

/// A YAML scalar: plain, `"double"` (as JSON reads it) or `'single'` quoted, or a block (`|`,
/// `>`) from the lines under it, folded to one line.
fn scalar(first: &str, more: &[&str]) -> String {
    if first.starts_with('|') || first.starts_with('>') || (first.is_empty() && !more.is_empty()) {
        let joiner = if first.starts_with('|') { "\n" } else { " " };
        return more
            .iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(joiner);
    }
    if first.starts_with('"') {
        return serde_json::from_str(first).unwrap_or_else(|_| first.trim_matches('"').to_owned());
    }
    if let Some(inner) = first.strip_prefix('\'').and_then(|f| f.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    first.to_owned()
}

/// A YAML list of names: `a, b`, `[a, b]`, or one `- a` a line under the key.
fn list(first: &str, more: &[&str]) -> Vec<String> {
    let items: Vec<String> = if first.is_empty() {
        more.iter()
            .filter_map(|l| l.trim().strip_prefix('-'))
            .map(|t| t.trim().to_owned())
            .collect()
    } else {
        first
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|t| t.trim().to_owned())
            .collect()
    };
    items
        .into_iter()
        .map(|t| t.trim_matches(['"', '\'']).to_owned())
        .filter(|t| !t.is_empty())
        .collect()
}

/// An agent as its file's text: its own keys first (the description always double quoted, so
/// any text is valid YAML), then every key it had that livtk does not edit, as it was.
pub fn render(agent: &Agent) -> String {
    let description =
        serde_json::to_string(&agent.description.replace('\n', " ")).unwrap_or_default();
    let mut text = format!("---\nname: {}\ndescription: {description}\n", agent.name);
    if !agent.tools.is_empty() {
        text.push_str(&format!("tools: {}\n", agent.tools.join(", ")));
    }
    if let Some(m) = &agent.model {
        text.push_str(&format!("model: {m}\n"));
    }
    if let Some(s) = agent.stage {
        text.push_str(&format!("stage: {}\n", s.as_str()));
    }
    if !agent.extra.trim().is_empty() {
        text.push_str(agent.extra.trim_end());
        text.push('\n');
    }
    text.push_str("---\n\n");
    text.push_str(agent.body.trim());
    text.push('\n');
    text
}

/// Whether `name` is one Claude Code takes: lowercase letters, digits and hyphens.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
}

/// Write `agent` to `place` for the project at `root`: a new file, or `replacing` the one it
/// was. Refused, writing nothing, for livtk's own place, a bad name, an empty description, or a
/// name another file there has.
pub fn save(
    root: &Path,
    home: &Path,
    place: Place,
    agent: &Agent,
    replacing: Option<&Path>,
) -> Result<PathBuf, String> {
    if !place.editable() {
        return Err("livtk's own agents are copied, never changed.".into());
    }
    if !valid_name(&agent.name) {
        return Err("A name is lowercase letters, digits and hyphens, like code-reviewer.".into());
    }
    if agent.description.trim().is_empty() {
        return Err("Say when to use it: Claude picks agents by their description.".into());
    }
    let dir = folder(place, root, home).ok_or("There is no folder for livtk's own agents.")?;
    let path = dir.join(format!("{}.md", agent.name));
    if path.exists() && replacing != Some(path.as_path()) {
        return Err(format!(
            "{} has an agent called {} already.",
            place.label(),
            agent.name
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".{}.md.tmp", agent.name));
    std::fs::write(&tmp, render(agent)).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    // Renamed: the old file goes.
    if let Some(old) = replacing
        && old != path
    {
        let _ = std::fs::remove_file(old);
    }
    Ok(path)
}

/// Remove the agent file `file`; never one of livtk's own.
pub fn remove(file: &AgentFile) -> Result<(), String> {
    if !file.place.editable() {
        return Err("livtk's own agents cannot be removed.".into());
    }
    std::fs::remove_file(&file.path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_file_round_trips() {
        let agent = Agent {
            name: "code-reviewer".into(),
            description: "Reviews a change for bugs.".into(),
            tools: vec!["Read".into(), "Grep".into()],
            model: Some("sonnet".into()),
            stage: Some(Stage::Review),
            body: "You review code.\n\n- Be brief.".into(),
            extra: String::new(),
        };
        assert_eq!(parse(&render(&agent)), Some(agent));
        assert_eq!(parse("no frontmatter"), None);
        assert_eq!(
            parse("---\ndescription: x\n---\nbody"),
            None,
            "a name is needed"
        );
    }

    #[test]
    fn saving_checks_first_and_writes_a_file_claude_code_reads() {
        let root = tempfile::tempdir().unwrap();
        let mut a = Agent {
            name: "Bad Name".into(),
            description: "Reviews code.".into(),
            ..Agent::default()
        };
        assert!(
            save(root.path(), root.path(), Place::Project, &a, None).is_err(),
            "a bad name"
        );
        a.name = "reviewer".into();
        a.description.clear();
        assert!(
            save(root.path(), root.path(), Place::Project, &a, None).is_err(),
            "no description"
        );
        a.description = "Reviews code.".into();
        assert!(
            save(root.path(), root.path(), Place::Livtk, &a, None).is_err(),
            "livtk's own place"
        );
        let path = save(root.path(), root.path(), Place::Project, &a, None).unwrap();
        assert_eq!(path, root.path().join(".claude/agents/reviewer.md"));
        assert_eq!(
            parse(&std::fs::read_to_string(&path).unwrap()),
            Some(a.clone())
        );
        assert!(
            save(root.path(), root.path(), Place::Project, &a, None).is_err(),
            "a clash"
        );
        // Saved again in place, renamed: one file, the new name.
        a.name = "critic".into();
        let renamed = save(root.path(), root.path(), Place::Project, &a, Some(&path)).unwrap();
        assert!(!path.exists() && renamed.exists());
        let file = AgentFile {
            place: Place::Project,
            path: renamed.clone(),
            agent: a,
        };
        remove(&file).unwrap();
        assert!(!renamed.exists());
        assert!(
            remove(&AgentFile {
                place: Place::Livtk,
                ..file
            })
            .is_err()
        );
    }

    #[test]
    fn every_yaml_form_reads_and_other_keys_survive_a_save() {
        let text = "---\nname: reviewer\ndescription: >\n  Reviews code: bugs,\n  and style.\ntools:\n  - Read\n  - Grep\ncolor: blue\nhooks:\n  Stop:\n    - command: echo hi\nmodel: 'opus'\n---\n\nYou review.\n";
        let a = parse(text).unwrap();
        assert_eq!(a.description, "Reviews code: bugs, and style.");
        assert_eq!(a.tools, ["Read", "Grep"]);
        assert_eq!(a.model.as_deref(), Some("opus"));
        assert!(a.extra.contains("color: blue") && a.extra.contains("    - command: echo hi"));
        // Written back: valid for a colon in the description, the other keys as they were.
        let back = render(&a);
        assert!(
            back.contains("description: \"Reviews code: bugs, and style.\""),
            "{back}"
        );
        assert!(
            back.contains("hooks:\n  Stop:\n    - command: echo hi"),
            "{back}"
        );
        assert_eq!(parse(&back), Some(a));
        // A flow list and a double-quoted name.
        let b = parse("---\nname: \"x\"\ndescription: d\ntools: [Read, \"Bash\"]\n---\nb").unwrap();
        assert_eq!(
            (b.name.as_str(), b.tools.clone()),
            ("x", vec!["Read".to_owned(), "Bash".to_owned()])
        );
    }

    #[test]
    fn names_are_what_claude_code_takes() {
        assert!(valid_name("code-reviewer"));
        assert!(valid_name("a2"));
        for bad in ["", "Code", "a b", "-x", "a_b"] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
