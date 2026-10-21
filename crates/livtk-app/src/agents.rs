//! Agents, outside the window: the Claude sessions working in a project (as Claude Code's own
//! `claude agents --json` lists them) and the agent files they can run as (livtk's, the
//! project's, the person's), read, written and removed. Plain IO, so it is tested without GPUI.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// How long `claude agents --json` may take before it is given up on.
const LIST_TIMEOUT: Duration = Duration::from_secs(10);

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

/// A Claude session, as Claude Code lists it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    #[serde(default)]
    pub pid: Option<u32>,
    pub cwd: PathBuf,
    /// `interactive` or `background`.
    #[serde(default)]
    pub kind: String,
    /// Milliseconds since the epoch.
    #[serde(default)]
    pub started_at: i64,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// While its process lives: `busy`, `waiting` or `idle`.
    #[serde(default)]
    pub status: Option<String>,
    /// A background session's id, for `claude stop`.
    #[serde(default)]
    pub id: Option<String>,
    /// A background session's state: working, blocked, done, failed or stopped.
    #[serde(default)]
    pub state: Option<String>,
}

/// Where a session stands, as people say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Standing {
    NeedsYou,
    Working,
    Idle,
    Finished,
}

impl Standing {
    pub const fn label(self) -> &'static str {
        match self {
            Standing::NeedsYou => "Needs you",
            Standing::Working => "Working",
            Standing::Idle => "Idle",
            Standing::Finished => "Finished",
        }
    }
}

impl Session {
    pub fn standing(&self) -> Standing {
        match (self.status.as_deref(), self.state.as_deref()) {
            (Some("waiting"), _) | (_, Some("blocked")) => Standing::NeedsYou,
            (Some("busy"), _) | (_, Some("working")) => Standing::Working,
            (_, Some("done" | "failed" | "stopped")) => Standing::Finished,
            _ => Standing::Idle,
        }
    }

    pub fn is_background(&self) -> bool {
        self.kind == "background"
    }

    /// What it is called: its name, else the start of its session id.
    pub fn title(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            self.session_id.as_deref().map_or_else(
                || "A session".to_owned(),
                |s| format!("Session {}", &s[..8.min(s.len())]),
            )
        })
    }

    /// A stable key for the list and the selection.
    pub fn key(&self) -> String {
        self.session_id
            .clone()
            .or_else(|| self.id.clone())
            .or_else(|| self.pid.map(|p| p.to_string()))
            .unwrap_or_default()
    }
}

/// Where to find `claude` and Claude Code's folder instead of looking: for tests, which cannot
/// set the environment safely.
static PATHS: std::sync::RwLock<Option<(PathBuf, PathBuf)>> = std::sync::RwLock::new(None);

/// Use `claude` as the command and `home` as Claude Code's folder from now on.
pub fn use_paths(claude: PathBuf, home: PathBuf) {
    *PATHS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((claude, home));
}

fn paths() -> Option<(PathBuf, PathBuf)> {
    PATHS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// The `claude` command: as `use_paths` said, `LIVTK_CLAUDE` if set, else the first found on the PATH or where its
/// installers put it (an app started from the Finder has a short PATH).
pub fn claude() -> Option<PathBuf> {
    if let Some((claude, _)) = paths() {
        return Some(claude);
    }
    if let Some(p) = std::env::var_os("LIVTK_CLAUDE") {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join("claude"))
        .chain(
            home.iter()
                .flat_map(|h| [h.join(".local/bin/claude"), h.join(".claude/local/claude")]),
        )
        .chain(["/opt/homebrew/bin/claude", "/usr/local/bin/claude"].map(PathBuf::from))
        .find(|p| p.is_file())
}

/// The sessions working under `root` (the project's folder, its worktrees included), Needs you
/// first, then the newest.
pub fn sessions(root: &Path) -> Result<Vec<Session>, String> {
    let claude =
        claude().ok_or("The claude command is not installed, or not where livtk looks.")?;
    let mut child = Command::new(&claude)
        .args(["agents", "--json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("claude agents would not start: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > LIST_TIMEOUT => {
                let _ = child.kill();
                return Err("claude agents took too long to answer.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut out = String::new();
    child
        .stdout
        .take()
        .ok_or("claude agents said nothing")?
        .read_to_string(&mut out)
        .map_err(|e| e.to_string())?;
    let mut all: Vec<Session> = serde_json::from_str(&out)
        .map_err(|e| format!("claude agents said something unexpected: {e}"))?;
    // Claude Code reports real paths; the project's may go through a link (/var, /private/var).
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let root = real(root);
    all.retain(|s| real(&s.cwd).starts_with(&root));
    all.sort_by(|a, b| {
        a.standing()
            .cmp(&b.standing())
            .then(b.started_at.cmp(&a.started_at))
    });
    Ok(all)
}

/// Stop background session `id`.
pub fn stop(id: &str) -> Result<(), String> {
    let claude = claude().ok_or("The claude command is not installed.")?;
    let out = Command::new(claude)
        .args(["stop", id])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

// ---- agent files --------------------------------------------------------------------------------

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

/// Claude Code's own folder: `LIVTK_CLAUDE_HOME` if set (for tests), else `~/.claude`.
pub fn claude_home() -> Option<PathBuf> {
    if let Some((_, home)) = paths() {
        return Some(home);
    }
    std::env::var_os("LIVTK_CLAUDE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude")))
}

/// The folder of a place's agents, for the project at `root`.
pub fn folder(place: Place, root: &Path) -> Option<PathBuf> {
    match place {
        Place::Project => Some(root.join(".claude/agents")),
        Place::Yours => claude_home().map(|h| h.join("agents")),
        Place::Livtk => livtk_plugin().map(|p| p.join("agents")),
    }
}

/// Where Claude Code installed livtk's plugin, as its plugin record says.
fn livtk_plugin() -> Option<PathBuf> {
    let record =
        std::fs::read_to_string(claude_home()?.join("plugins/installed_plugins.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&record).ok()?;
    v["plugins"]
        .as_object()?
        .iter()
        .find(|(name, _)| name.starts_with("livtk@"))
        .and_then(|(_, installs)| installs.as_array()?.first()?["installPath"].as_str())
        .map(PathBuf::from)
}

/// Every agent the project at `root` can run as, by place, then by name.
pub fn agents(root: &Path) -> Vec<AgentFile> {
    let mut out = Vec::new();
    for place in Place::ALL {
        let Some(dir) = folder(place, root) else {
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
    let dir = folder(place, root).ok_or("There is no home folder to put it in.")?;
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
            save(root.path(), Place::Project, &a, None).is_err(),
            "a bad name"
        );
        a.name = "reviewer".into();
        a.description.clear();
        assert!(
            save(root.path(), Place::Project, &a, None).is_err(),
            "no description"
        );
        a.description = "Reviews code.".into();
        assert!(
            save(root.path(), Place::Livtk, &a, None).is_err(),
            "livtk's own place"
        );
        let path = save(root.path(), Place::Project, &a, None).unwrap();
        assert_eq!(path, root.path().join(".claude/agents/reviewer.md"));
        assert_eq!(
            parse(&std::fs::read_to_string(&path).unwrap()),
            Some(a.clone())
        );
        assert!(
            save(root.path(), Place::Project, &a, None).is_err(),
            "a clash"
        );
        // Saved again in place, renamed: one file, the new name.
        a.name = "critic".into();
        let renamed = save(root.path(), Place::Project, &a, Some(&path)).unwrap();
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

    #[test]
    fn a_session_reads_as_people_say_it() {
        let s = |status: Option<&str>, state: Option<&str>| Session {
            pid: None,
            cwd: PathBuf::new(),
            kind: String::new(),
            started_at: 0,
            session_id: None,
            name: None,
            status: status.map(str::to_owned),
            id: None,
            state: state.map(str::to_owned),
        };
        assert_eq!(s(Some("waiting"), None).standing(), Standing::NeedsYou);
        assert_eq!(s(None, Some("blocked")).standing(), Standing::NeedsYou);
        assert_eq!(s(Some("busy"), None).standing(), Standing::Working);
        assert_eq!(s(Some("idle"), None).standing(), Standing::Idle);
        assert_eq!(s(None, Some("done")).standing(), Standing::Finished);
    }
}
