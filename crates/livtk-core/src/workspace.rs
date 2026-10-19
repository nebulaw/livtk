//! The public API. The CLI, the hooks and the app all go through a `Workspace`, so every rule is
//! enforced in one place.
//!
//! Writing follows one path: take the lock, catch up with the log, validate the command, append
//! its events to the log (the commit point), apply them to the store, release the lock. A crash
//! anywhere loses nothing; the next open catches up from the log.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::event::{DocData, EdgeData, Event, NodeData, NodePatch, NoteData, Op};
use crate::log::{self, Log, Mark};
use crate::model::{
    Actor, Key, Kind, Priority, Rel, State, default_prefix, validate_project_prefix,
};
#[cfg(feature = "embed")]
use crate::search::Model;
use crate::search::{
    CANDIDATES, DEFAULT_MODEL, Embed, Hit, cosine, cosine_bar, fts_query, fts_query_typing, fuse,
    matched_terms, models_dir, query_terms, text_hash,
};
use crate::store::{Applied, Brief, Node, Snapshot, Store, Summary};

/// Recall injects at most this many items into a prompt.
pub const RECALL_MAX: usize = 3;
use crate::{Error, Result};

pub const DIR_NAME: &str = ".livtk";
const CONFIG_VERSION: u32 = 1;
const TITLE_MAX: usize = 200;
const NOTE_MAX: usize = 20_000;

/// `.livtk/config.toml`, committed with the project. A name it does not know (a misspelt table
/// or key) is an error, so nothing is switched off or on by a typo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    /// Key prefix for new nodes, as in `LIV-12`.
    pub prefix: String,
    #[serde(default, skip_serializing_if = "Hooks::is_default")]
    pub hooks: Hooks,
    #[serde(default, skip_serializing_if = "Memory::is_default")]
    pub memory: Memory,
    #[serde(default, skip_serializing_if = "Gates::is_default")]
    pub gates: Gates,
    /// Commands run on signals (`[[on]]`): see `on`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<crate::on::On>,
    /// The folder the project's docs are in, from its folder: `docs` unless given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    /// What the session-start brief says, by section; everything unless given.
    #[serde(default, skip_serializing_if = "Sections::is_default")]
    pub session_start: Sections,
    /// What is added before each prompt, by section; everything unless given.
    #[serde(default, skip_serializing_if = "Sections::is_default")]
    pub user_prompt_submit: Sections,
    /// Kinds the project declares beside the built-in four (`[[kinds]]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<crate::KindSpec>,
    /// Relations the project declares beside the built-in five (`[[relations]]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<crate::RelSpec>,
}

/// The hooks a project may switch off with `[hooks] disable`. The guard (pre-tool-use) is not
/// among them: it keeps the human's gates, and the file is one Claude could edit.
pub const HOOK_NAMES: [&str; 4] = [
    "session-start",
    "user-prompt-submit",
    "subagent-start",
    "subagent-stop",
];
/// What the session-start brief is made of, as a reader sees it.
pub const SESSION_SECTIONS: [&str; 5] = ["intro", "tasks", "gates", "protocol", "rules"];
/// What is added before a prompt: the memory recalled for it, and the nudge to propose a lesson.
pub const PROMPT_SECTIONS: [&str; 2] = ["memory", "nudge"];

/// The sections of what a hook says that a project keeps: all of them unless `include` names
/// some.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sections {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
}

impl Sections {
    fn is_default(&self) -> bool {
        self.include.is_none()
    }

    /// Whether `section` is said.
    pub fn has(&self, section: &str) -> bool {
        self.include
            .as_ref()
            .is_none_or(|i| i.iter().any(|s| s == section))
    }

    /// The first name in `include` that is not one of `known`.
    fn unknown(&self, known: &[&str]) -> Option<String> {
        self.include
            .iter()
            .flatten()
            .find(|s| !known.contains(&s.as_str()))
            .cloned()
    }
}

impl Config {
    /// Whether the hook `name` (`session-start`, ...) runs: not switched off in `[hooks]`.
    pub fn hook_on(&self, name: &str) -> bool {
        !self.hooks.disable.iter().any(|h| h == name)
    }
}

/// Moves this project keeps for the human, beyond the built-in ones (acknowledging, and
/// deciding what binds). The file is in the repository, where Claude could change it, so it can
/// only add gates: an edit to it can make Claude stricter, never freer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gates {
    /// Each `kind: from -> to`, or `kind: -> to` for a move into `to` from anywhere.
    pub human: Vec<String>,
}

impl Gates {
    fn is_default(&self) -> bool {
        self.human.is_empty()
    }

    /// The gates, read; an entry that names no legal move is an error, so a typo never
    /// silently gates nothing.
    pub fn parse(&self) -> std::result::Result<Vec<Gate>, String> {
        self.human.iter().map(|e| Gate::parse(e)).collect()
    }
}

/// One move kept for the human by the project's config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    pub kind: Kind,
    /// `None`: from any state.
    pub from: Option<State>,
    pub to: State,
}

impl Gate {
    fn parse(entry: &str) -> std::result::Result<Gate, String> {
        let form = || format!("`{entry}`: write it as `kind: from -> to`, or `kind: -> to`");
        let (kind, mv) = entry.split_once(':').ok_or_else(form)?;
        let (from, to) = mv.split_once("->").ok_or_else(form)?;
        let kind: Kind = kind
            .trim()
            .parse()
            .map_err(|_| format!("`{entry}`: no kind `{}`", kind.trim()))?;
        let state = |s: &str| -> std::result::Result<State, String> {
            match s.parse::<State>() {
                Ok(st) if kind.states().contains(&st) => Ok(st),
                _ => Err(format!(
                    "`{entry}`: a {kind} has no state `{s}`; its states: {}",
                    names(kind.states())
                )),
            }
        };
        let to = state(to.trim())?;
        let from = match from.trim() {
            "" => None,
            f => Some(state(f)?),
        };
        match from {
            Some(f) if !kind.can_move(f, to) => Err(format!(
                "`{entry}`: a {kind} cannot move from {f} to {to}; from {f} it can move to: {}",
                names(kind.next_states(f))
            )),
            None if !kind.states().iter().any(|f| kind.can_move(*f, to)) => {
                Err(format!("`{entry}`: no {kind} ever moves to {to}"))
            }
            _ => Ok(Gate { kind, from, to }),
        }
    }

    /// Whether this gate keeps moving a `kind` from `from` to `to` for the human.
    pub fn holds(&self, kind: Kind, from: State, to: State) -> bool {
        self.kind == kind && self.to == to && self.from.is_none_or(|f| f == from)
    }
}

fn names(states: &[State]) -> String {
    let names: Vec<&str> = states.iter().map(|s| s.as_str()).collect();
    names.join(", ")
}

/// Memory settings. Every field is optional.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Memory {
    /// The model2vec model in the models directory; `none` turns ranking by meaning off.
    pub model: String,
}

impl Default for Memory {
    fn default() -> Self {
        Memory {
            model: DEFAULT_MODEL.to_owned(),
        }
    }
}

impl Memory {
    fn is_default(&self) -> bool {
        *self == Memory::default()
    }
}

impl Config {
    /// Read and validate `config.toml` in a `.livtk/` directory, without opening the store.
    pub fn load(dir: &Path) -> Result<Config> {
        load_config(dir)
    }
}

/// The `[hooks]` table of a project's config. livtk runs no project's commands any more:
/// formatting and build checks belong to the project's own hooks. The old fields are still read,
/// for one release, only to say so; they do nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hooks {
    /// The hooks this project switches off, because it covers them itself: any of `HOOK_NAMES`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disable: Vec<String>,
    /// Deprecated, ignored: was a formatter run on files Claude edited.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub format: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub format_extensions: Vec<String>,
    /// Deprecated, ignored: was a check run before an agent stopped.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub check: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub check_agents: Vec<String>,
}

impl Hooks {
    fn is_default(&self) -> bool {
        *self == Hooks::default()
    }

    /// What the config still asks of livtk that it no longer does, and where it went.
    pub fn deprecated(&self) -> Option<String> {
        let mut gone = Vec::new();
        if !self.format.is_empty() || !self.format_extensions.is_empty() {
            gone.push("format");
        }
        // `check_agents` alone ran nothing (it only named who a check applied to): no warning.
        if !self.check.is_empty() {
            gone.push("check");
        }
        (!gone.is_empty()).then(|| {
            format!(
                "[hooks] {} in .livtk/config.toml is ignored: livtk no longer runs a project's formatter or build check. They belong in the project's own Claude Code hooks (PostToolUse to format, SubagentStop or Stop to check). The developer removes the field (Claude cannot edit .livtk/); it becomes an error in the next release",
                gone.join(" and ")
            )
        })
    }
}

/// The nearest `.livtk/` directory in `start` or one of its parents.
pub fn find_dir(start: &Path) -> Result<PathBuf> {
    let start = start.canonicalize()?;
    start
        .ancestors()
        .map(|d| d.join(DIR_NAME))
        .find(|d| d.join("config.toml").is_file())
        .ok_or(Error::NotFound)
}

/// A change to the workspace. Node references accept a key (`LIV-12`), a bare number (`12`), or
/// a node id.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Create a node of any kind, with links from it to existing nodes.
    Create {
        kind: Kind,
        title: String,
        body: String,
        /// Tasks only; other kinds keep the default.
        priority: Option<u8>,
        links: Vec<(Rel, String)>,
    },
    Edit {
        node: String,
        title: Option<String>,
        body: Option<String>,
    },
    Move {
        node: String,
        to: State,
    },
    SetPriority {
        node: String,
        priority: u8,
    },
    Link {
        src: String,
        rel: Rel,
        dst: String,
    },
    Unlink {
        src: String,
        rel: Rel,
        dst: String,
    },
    /// Link a node to the doc at `path` (from the project's folder). Any number each way.
    DocLink {
        node: String,
        path: String,
    },
    DocUnlink {
        node: String,
        path: String,
    },
    /// Add a note to a node's history. Changes no state.
    Note {
        node: String,
        text: String,
    },
    /// Ratify a lesson (human only). `always` makes it a standing rule shown at every session
    /// start; otherwise it is recalled when relevant. On a ratified lesson, changes only that.
    Ratify {
        node: String,
        always: bool,
    },
}

/// What a search draws from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every node.
    All,
    /// What binds or informs Claude: ratified lessons that are not standing rules, accepted
    /// decisions, findings.
    Recall,
}

/// One line of the activity feed.
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    pub event: Event,
    /// The node the event is about; `None` only if the log names a node that never applied.
    pub node: Option<Brief>,
    /// For a link, the node at its other end.
    pub other: Option<Brief>,
}

const ACTIVITY_SEEN: &str = "activity_seen";

/// The most unseen events [`Workspace::unseen_activity`] counts.
pub const UNSEEN_CAP: usize = 100;

/// A node's dependencies in both directions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Links {
    /// What this node waits for.
    pub depends_on: Vec<Brief>,
    /// What waits for this node.
    pub blocks: Vec<Brief>,
}

pub struct Workspace {
    dir: PathBuf,
    config: Config,
    /// The config's gates, read.
    gates: Vec<Gate>,
    log: Log,
    store: Store,
    warnings: Vec<String>,
    models_dir: Option<PathBuf>,
    /// The embedding model: not looked for yet (`None`), absent (`Some(None)`), or loaded.
    model: Option<Option<Box<dyn Embed>>>,
    /// Texts embedded by this workspace so far; stored vectors are reused, never recounted.
    embedded: usize,
    /// The log position this workspace last showed its reader. Another process applies its own
    /// writes to the shared store, so "the store had to catch up" says nothing about whether this
    /// reader has seen them; this does.
    seen: Mark,
    /// The vectors search ranks meaning with, as of a store mark: a search per keystroke must
    /// not re-read every text and vector (10,000 nodes cost 29 ms that way, 20 MB read).
    vectors: Option<Vectors>,
}

/// Vectors for one scope and model, valid while the store stays at `mark`.
struct Vectors {
    mark: Mark,
    recall_only: bool,
    model: String,
    by_node: Vec<(Uuid, Vec<f32>)>,
}

impl Workspace {
    /// Create `.livtk/` in `project` and open it.
    pub fn init(project: &Path, prefix: Option<&str>) -> Result<Workspace> {
        let project = project.canonicalize()?;
        let dir = project.join(DIR_NAME);
        if dir.join("config.toml").exists() {
            return Err(Error::AlreadyInitialised(project));
        }
        let prefix = match prefix {
            Some(p) => {
                let p = p.trim().to_ascii_uppercase();
                validate_project_prefix(&p)?;
                p
            }
            None => default_prefix(
                &project
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
        };
        fs::create_dir_all(&dir)?;
        let config = Config {
            version: CONFIG_VERSION,
            prefix,
            hooks: Hooks::default(),
            memory: Memory::default(),
            gates: Gates::default(),
            on: Vec::new(),
            docs: None,
            session_start: Sections::default(),
            user_prompt_submit: Sections::default(),
            kinds: Vec::new(),
            relations: Vec::new(),
        };
        let text = toml::to_string(&config).map_err(|e| Error::Config {
            path: dir.join("config.toml"),
            msg: e.to_string(),
        })?;
        fs::write(dir.join("config.toml"), text)?;
        fs::write(
            dir.join(".gitignore"),
            "livtk.db*\nlock\non.lock\non-state.json*\n",
        )?;
        fs::write(dir.join(".gitattributes"), "events.jsonl merge=union\n")?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("events.jsonl"))?;
        Workspace::open_dir(&dir)
    }

    /// Open the workspace that holds `start`: the nearest `.livtk/` in it or a parent.
    pub fn open(start: &Path) -> Result<Workspace> {
        Workspace::open_dir(&find_dir(start)?)
    }

    /// Open a `.livtk/` directory directly.
    pub fn open_dir(dir: &Path) -> Result<Workspace> {
        let config = load_config(dir)?;
        let deprecated = config.hooks.deprecated();
        let store = Store::open(&dir.join("livtk.db"))?;
        // Checked as the config loaded.
        let gates = config.gates.parse().unwrap_or_default();
        let mut ws = Workspace {
            dir: dir.to_path_buf(),
            config,
            gates,
            log: Log::new(dir.join("events.jsonl")),
            store,
            warnings: deprecated.into_iter().collect(),
            models_dir: models_dir(),
            model: None,
            embedded: 0,
            seen: Mark::default(),
            vectors: None,
        };
        if !ws.store.has_current_schema()? {
            let _lock = ws.lock()?;
            if !ws.store.has_current_schema()? {
                ws.store.reset_schema()?;
            }
            ws.catch_up()?;
        }
        ws.check_kinds()?;
        ws.refresh()?;
        Ok(ws)
    }

    /// Every kind the project's nodes are of is still declared: a kind taken out of the config
    /// while nodes of it exist would orphan them, so livtk refuses and names them.
    fn check_kinds(&self) -> Result<()> {
        for (kind, keys) in self.store.kinds_in_use()? {
            if Kind::named(&kind).is_none() {
                let shown: Vec<&str> = keys.iter().take(5).map(String::as_str).collect();
                return Err(Error::Config {
                    path: self.dir.join("config.toml"),
                    msg: format!(
                        "nodes of kind `{kind}` exist ({}{}) but no [[kinds]] declares it; declare it again",
                        shown.join(", "),
                        if keys.len() > 5 { ", ..." } else { "" }
                    ),
                });
            }
        }
        Ok(())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The project's own folder, which holds `.livtk`.
    pub fn root(&self) -> &Path {
        self.dir.parent().unwrap_or(&self.dir)
    }

    /// The folder the project's docs are in, from its folder.
    pub fn docs_dir(&self) -> String {
        self.config
            .docs
            .as_deref()
            .and_then(|d| crate::docs::normalize(Path::new(""), d))
            .unwrap_or_else(|| crate::docs::DEFAULT_DIR.to_owned())
    }

    /// The project's docs, as a tree.
    pub fn docs(&self) -> crate::docs::Folder {
        crate::docs::tree(self.root(), &self.docs_dir())
    }

    /// The docs a node is linked to, by path; each with its title, `None` once it is gone.
    pub fn docs_of(&self, node: &str) -> Result<Vec<(String, Option<String>)>> {
        let n = self.node(node)?;
        Ok(self
            .store
            .docs_of(n.id)?
            .into_iter()
            .map(|path| {
                let file = self.root().join(&path);
                let title = file.is_file().then(|| {
                    std::fs::read_to_string(&file)
                        .ok()
                        .and_then(|t| crate::docs::title_of(&t))
                        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(&path).to_owned())
                });
                (path, title)
            })
            .collect())
    }

    /// The nodes linked to the doc at `path`, by key.
    pub fn doc_nodes(&self, path: &str) -> Result<Vec<Node>> {
        let path = crate::docs::normalize(self.root(), path)
            .ok_or_else(|| Error::Invalid(format!("{path} is outside the project")))?;
        let mut nodes = Vec::new();
        for id in self.store.doc_nodes(&path)? {
            if let Some(n) = self.store.node(id)? {
                nodes.push(n);
            }
        }
        nodes.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(nodes)
    }

    /// The nodes that mention the doc at `path` in their title, plan or notes, but are not
    /// linked to it (those are its links).
    pub fn doc_mentions(&self, path: &str) -> Result<Vec<Node>> {
        let linked = self.store.doc_nodes(path)?;
        let mut nodes = Vec::new();
        for id in self.store.mentioning(path)? {
            if linked.contains(&id) {
                continue;
            }
            if let Some(n) = self.store.node(id)? {
                nodes.push(n);
            }
        }
        Ok(nodes)
    }

    /// Every doc some node is linked to, with how many nodes.
    pub fn linked_docs(&self) -> Result<Vec<(String, usize)>> {
        self.store.linked_docs()
    }

    /// Whether only a human may move a `kind` from `from` to `to` here: a built-in gate, or one
    /// the project's config adds.
    pub fn human_only(&self, kind: Kind, from: State, to: State) -> bool {
        kind.human_only(from, to) || self.gate(kind, from, to).is_some()
    }

    /// The gates this project's config adds to the built-in ones.
    pub fn gates(&self) -> &[Gate] {
        &self.gates
    }

    fn gate(&self, kind: Kind, from: State, to: State) -> Option<&Gate> {
        self.gates.iter().find(|g| g.holds(kind, from, to))
    }

    /// Bring the store up to date with the log, and say whether anything changed since this
    /// workspace last looked, whoever changed it: another process (which applies its own writes
    /// to the shared store), a git pull, or a rebuild. Its own writes count as seen. Cheap when
    /// nothing changed (a stat, two small reads and one small query), so long-lived readers such
    /// as the app call it on a timer.
    pub fn refresh(&mut self) -> Result<bool> {
        let applied = self.store.applied()?;
        let len = self.log.len()?;
        if len != applied.mark.len || self.log.mark(len)? != applied.mark {
            let _lock = self.lock()?;
            self.catch_up()?;
        }
        let now = self.store.applied()?.mark;
        let changed = now != self.seen;
        self.seen = now;
        Ok(changed)
    }

    /// Whether the log holds what the store has not applied: what `refresh` would catch up,
    /// found without taking the lock or applying anything. A reader that must not wait (the
    /// app's window) checks this and leaves the catching up to another thread.
    pub fn stale(&self) -> Result<bool> {
        let applied = self.store.applied()?;
        let len = self.log.len()?;
        Ok(len != applied.mark.len || self.log.mark(len)? != applied.mark)
    }

    /// Rebuild the store from the log.
    pub fn rebuild(&mut self) -> Result<()> {
        let _lock = self.lock()?;
        if let Some(w) = self.log.repair_tail()? {
            self.warnings.push(w);
        }
        self.rebuild_locked()
    }

    /// Validate a command and record it. Returns the events written; an empty list means the
    /// command changed nothing.
    pub fn execute(&mut self, cmd: Command, by: &Actor) -> Result<Vec<Event>> {
        let _lock = self.lock()?;
        self.catch_up()?;
        let applied = self.store.applied()?;
        let mut ops = self.lower(cmd, by)?;
        if ops.is_empty() {
            return Ok(Vec::new());
        }
        let mut last = applied.max_id;
        let events: Vec<Event> = ops
            .drain(..)
            .map(|op| {
                let e = Event::new(last, by, op);
                last = Some(e.id);
                e
            })
            .collect();
        let lines = events
            .iter()
            .map(serde_json::to_string)
            .collect::<serde_json::Result<Vec<_>>>()?;
        let len = self.log.append(&lines)?;
        let applied = Applied {
            mark: self.log.mark(len)?,
            max_id: last,
        };
        let warnings = self.store.apply(&events, applied, false)?;
        self.warnings.extend(warnings);
        self.seen = applied.mark;
        Ok(events)
    }

    /// Warnings gathered since the last call: skipped log lines, events that could not apply.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    // ---- queries --------------------------------------------------------------------------

    pub fn resolve(&self, reference: &str) -> Result<Uuid> {
        self.store
            .resolve(reference, &self.config.prefix)?
            .ok_or_else(|| Error::UnknownNode(reference.trim().to_owned()))
    }

    pub fn node(&self, reference: &str) -> Result<Node> {
        let id = self.resolve(reference)?;
        self.store
            .node(id)?
            .ok_or_else(|| Error::UnknownNode(reference.to_owned()))
    }

    /// Tasks in the given states, or every task when `states` is empty. Work in hand first.
    pub fn tasks(&self, states: &[State]) -> Result<Vec<Summary>> {
        self.store.list(Kind::Task, states)
    }

    /// The best task to start now: acknowledged and waiting on nothing.
    pub fn next(&self) -> Result<Option<Node>> {
        self.store.next()
    }

    pub fn links(&self, reference: &str) -> Result<Links> {
        let id = self.resolve(reference)?;
        let (depends_on, blocks) = self.store.links(id, Rel::DependsOn)?;
        Ok(Links { depends_on, blocks })
    }

    /// The nodes `reference` links to over `rel`, and the nodes linking to it.
    pub fn related(&self, reference: &str, rel: Rel) -> Result<(Vec<Brief>, Vec<Brief>)> {
        self.store.links(self.resolve(reference)?, rel)
    }

    /// The events about one node, or the whole log, oldest first.
    pub fn history(&self, reference: Option<&str>) -> Result<Vec<Event>> {
        match reference {
            Some(r) => self.store.history(self.resolve(r)?),
            None => self.store.all_events(),
        }
    }

    /// A node's key as it stands now (a merge may have renumbered it since it was made).
    pub fn key_of(&self, id: Uuid) -> Result<Option<String>> {
        Ok(self.store.brief(id)?.map(|b| b.key))
    }

    /// The newest event's id: one cheap look, to tell whether anything was written since.
    pub fn newest_event(&self) -> Result<Option<Uuid>> {
        Ok(self.store.events_page(None, 1)?.first().map(|e| e.id))
    }

    /// How many events each day holds, by who, what and the state moved to: what the
    /// activity grid draws.
    pub fn activity_by_day(&self) -> Result<Vec<crate::store::DayEvents>> {
        self.store.events_by_day()
    }

    /// The activity feed between `from` and `to` (UTC, RFC 3339; `to` excluded): up to `limit`
    /// events, newest first, before `before` or from the newest.
    pub fn activity_in(
        &self,
        from: &str,
        to: &str,
        before: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<Activity>> {
        let mut out = Vec::new();
        for event in self.store.events_page_in(from, to, before, limit)? {
            let (node, other) = event.op.subjects();
            out.push(Activity {
                node: self.store.brief(node)?,
                other: match other {
                    Some(o) => self.store.brief(o)?,
                    None => None,
                },
                event,
            });
        }
        Ok(out)
    }

    /// The events after `after` (or the whole log), oldest first: what a reader of signals
    /// that stopped there has not seen.
    pub fn events_after(&self, after: Option<Uuid>) -> Result<Vec<Event>> {
        self.store.events_after(after)
    }

    /// The activity feed: up to `limit` events, newest first, before `before` or from the
    /// newest, each with the node it is about and, for a link, the other end.
    pub fn activity(&self, before: Option<Uuid>, limit: usize) -> Result<Vec<Activity>> {
        let mut out = Vec::new();
        for event in self.store.events_page(before, limit)? {
            let (node, other) = event.op.subjects();
            out.push(Activity {
                node: self.store.brief(node)?,
                other: match other {
                    Some(o) => self.store.brief(o)?,
                    None => None,
                },
                event,
            });
        }
        Ok(out)
    }

    /// The newest event the human has seen in the activity feed. Local to this machine, and
    /// kept by a rebuild.
    pub fn activity_seen(&self) -> Result<Option<Uuid>> {
        Ok(self.store.meta(ACTIVITY_SEEN)?.and_then(|v| v.parse().ok()))
    }

    pub fn set_activity_seen(&self, id: Uuid) -> Result<()> {
        self.store.set_meta(ACTIVITY_SEEN, &id.to_string())
    }

    /// Events by anyone but the human since they last looked at the feed, up to
    /// [`UNSEEN_CAP`]: a count for a badge, which shows more as "99+".
    pub fn unseen_activity(&self) -> Result<usize> {
        // The log names the actor as `Actor` displays it.
        self.store
            .count_events_after(self.activity_seen()?, &Actor::Human.to_string(), UNSEEN_CAP)
    }

    /// Every link between nodes, as (source, relation, destination).
    pub fn edges(&self) -> Result<Vec<(Uuid, Rel, Uuid)>> {
        self.store.edges()
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        self.store.snapshot()
    }

    /// Nodes of a kind in the given states, or in every state when `states` is empty.
    pub fn list(&self, kind: Kind, states: &[State]) -> Result<Vec<Summary>> {
        self.store.list(kind, states)
    }

    /// Everything waiting for the human: plans to acknowledge, lessons to ratify, decisions to
    /// accept.
    pub fn inbox(&self) -> Result<Vec<Summary>> {
        let mut out = self.store.list(Kind::Task, &[State::Planned])?;
        out.extend(self.store.list(Kind::Lesson, &[State::Proposed])?);
        out.extend(self.store.list(Kind::Decision, &[State::Proposed])?);
        // What waits on a gate the config adds; a gate from any state guards a move but makes
        // nothing wait.
        for g in &self.gates {
            if let Some(from) = g.from {
                for s in self.store.list(g.kind, &[from])? {
                    if !out.iter().any(|o| o.id == s.id) {
                        out.push(s);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Ratified lessons marked as standing rules.
    pub fn always_rules(&self) -> Result<Vec<Brief>> {
        self.store.always_rules()
    }

    /// Use models from `dir` instead of the default models directory; `None` turns ranking by
    /// meaning off.
    pub fn set_models_dir(&mut self, dir: Option<PathBuf>) {
        self.models_dir = dir;
        self.model = None;
        self.vectors = None;
    }

    /// The model search ranks meaning with, and whether it is installed and loads.
    pub fn model_status(&mut self) -> (String, Option<PathBuf>, bool) {
        let name = self.config.memory.model.clone();
        let path = self.models_dir.as_ref().map(|d| d.join(&name));
        let model = self.take_model();
        let loaded = model.is_some();
        self.model = Some(model);
        (name, path, loaded)
    }

    /// How many texts this workspace has embedded since it was opened. Stored vectors whose text
    /// is unchanged are reused and not counted.
    pub fn embedded(&self) -> usize {
        self.embedded
    }

    /// Ranked search: keywords and meaning, fused. Embeds any node whose text is new or changed
    /// since it was last embedded, and keeps the vectors.
    pub fn search(&mut self, query: &str, scope: Scope, limit: usize) -> Result<Vec<Hit>> {
        self.ranked(query, scope, limit, false)
    }

    /// Load the model and the vectors search ranks with, ahead of the first query: the command
    /// palette does this as it opens, so typing never waits for them.
    pub fn prepare_search(&mut self) -> Result<()> {
        let model = self.take_model();
        let prepared = model
            .as_deref()
            .map(|m| self.rank_by_meaning(m, "", false))
            .transpose();
        self.model = Some(model);
        prepared.map(|_| ())
    }

    /// Search as it is typed, for the command palette. A query that is a key (`LIV-12`,
    /// `liv-12`, `12`) brings that node first. The word being typed matches as a prefix, unless
    /// the query ends in a space. Meaning ranks only what clears the model's relevance bar, so a
    /// half-typed word brings no noise.
    pub fn find(&mut self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let mut hits = Vec::new();
        if let Ok(id) = self.resolve(query)
            && let Some(n) = self.store.node(id)?
        {
            hits.push(Hit {
                id,
                key: n.key,
                kind: n.kind,
                state: n.state,
                title: n.title,
                // Above any fused score, which stays under 1.
                score: 1.0,
                cosine: None,
                terms: 0,
            });
        }
        let typing = !query.ends_with(char::is_whitespace);
        for h in self.ranked(query, Scope::All, limit, typing)? {
            if !hits.iter().any(|k| k.id == h.id) {
                hits.push(h);
            }
        }
        hits.truncate(limit);
        Ok(hits)
    }

    /// Keywords and meaning, fused. `typing` reads the last word as a prefix and keeps only
    /// confident meaning matches.
    fn ranked(
        &mut self,
        query: &str,
        scope: Scope,
        limit: usize,
        typing: bool,
    ) -> Result<Vec<Hit>> {
        let terms = query_terms(query);
        let recall_only = scope == Scope::Recall;
        let fts = if typing {
            fts_query_typing(&terms)
        } else {
            fts_query(&terms)
        };
        let keyword = match fts {
            Some(q) => self.store.keyword(&q, recall_only, CANDIDATES)?,
            None => Vec::new(),
        };
        let model = self.take_model();
        let ranked = model
            .as_deref()
            // A letter or two typed so far carries no meaning worth ranking.
            .filter(|_| !typing || !terms.is_empty())
            .map(|m| self.rank_by_meaning(m, query, recall_only))
            .transpose();
        self.model = Some(model);
        let cosines = ranked?.unwrap_or_default();
        let bar = if typing {
            cosine_bar(&self.config.memory.model)
        } else {
            f32::NEG_INFINITY
        };
        let dense: Vec<Uuid> = cosines
            .iter()
            .filter(|(_, c)| *c >= bar)
            .take(CANDIDATES)
            .map(|(id, _)| *id)
            .collect();
        let mut hits = Vec::new();
        for (id, score) in fuse(&[&keyword, &dense]).into_iter().take(limit) {
            let Some(n) = self.store.node(id)? else {
                continue;
            };
            hits.push(Hit {
                id,
                terms: matched_terms(&terms, &format!("{}\n{}", n.title, n.body)),
                cosine: cosines.iter().find(|(i, _)| *i == id).map(|(_, c)| *c),
                key: n.key,
                kind: n.kind,
                state: n.state,
                title: n.title,
                score,
            });
        }
        Ok(hits)
    }

    /// What binds or informs Claude for this prompt: at most [`RECALL_MAX`] items that pass the
    /// relevance bar, best first.
    pub fn recall(&mut self, query: &str) -> Result<Vec<Hit>> {
        let needed = query_terms(query).len().clamp(1, 2);
        let bar = cosine_bar(&self.config.memory.model);
        Ok(self
            .search(query, Scope::Recall, CANDIDATES)?
            .into_iter()
            .filter(|h| h.cosine.is_some_and(|c| c >= bar) || h.terms >= needed)
            .take(RECALL_MAX)
            .collect())
    }

    /// Every candidate's cosine to the query, best first.
    fn rank_by_meaning(
        &mut self,
        model: &dyn Embed,
        query: &str,
        recall_only: bool,
    ) -> Result<Vec<(Uuid, f32)>> {
        // Any write, by any process, moves the mark; vectors themselves never do.
        let mark = self.store.applied()?.mark;
        let vectors = match self.vectors.take() {
            Some(v)
                if v.mark == mark && v.recall_only == recall_only && v.model == model.name() =>
            {
                v
            }
            _ => Vectors {
                mark,
                recall_only,
                model: model.name().to_owned(),
                by_node: self.current_vectors(model, recall_only)?,
            },
        };
        let q = model.embed(&[query.to_owned()])?.pop().unwrap_or_default();
        let mut cosines: Vec<(Uuid, f32)> = vectors
            .by_node
            .iter()
            .map(|(id, v)| (*id, cosine(&q, v)))
            .collect();
        self.vectors = Some(vectors);
        cosines.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        Ok(cosines)
    }

    /// Every node's vector in the scope: stored ones whose text is unchanged, the rest embedded
    /// now and stored.
    fn current_vectors(
        &mut self,
        model: &dyn Embed,
        recall_only: bool,
    ) -> Result<Vec<(Uuid, Vec<f32>)>> {
        let texts = self.store.texts(recall_only)?;
        let stored = self.store.vectors(recall_only)?;
        let mut vectors: Vec<(Uuid, Vec<f32>)> = Vec::with_capacity(texts.len());
        let mut missing: Vec<(Uuid, String, String)> = Vec::new();
        for (id, text) in texts {
            let hash = text_hash(model.name(), &text);
            match stored.get(&id) {
                Some((h, v)) if *h == hash => vectors.push((id, v.clone())),
                _ => missing.push((id, hash, text)),
            }
        }
        if !missing.is_empty() {
            let fresh = model.embed(&missing.iter().map(|m| m.2.clone()).collect::<Vec<_>>())?;
            self.embedded += missing.len();
            let rows: Vec<(Uuid, String, Vec<f32>)> = missing
                .into_iter()
                .zip(fresh)
                .map(|((id, hash, _), v)| (id, hash, v))
                .collect();
            self.store.put_vectors(&rows)?;
            vectors.extend(rows.into_iter().map(|(id, _, v)| (id, v)));
        }
        Ok(vectors)
    }

    /// The loaded model, loading it on first use. The caller puts it back in `self.model`.
    fn take_model(&mut self) -> Option<Box<dyn Embed>> {
        if let Some(loaded) = self.model.take() {
            return loaded;
        }
        let name = &self.config.memory.model;
        if name == "none" || name.is_empty() {
            return None;
        }
        let dir = self.models_dir.clone()?;
        #[cfg(feature = "embed")]
        {
            if !dir.join(name).exists() {
                return None;
            }
            match Model::load(&dir, name) {
                Ok(m) => Some(Box::new(m)),
                Err(e) => {
                    self.warnings
                        .push(format!("{e}; searching by keywords only"));
                    None
                }
            }
        }
        #[cfg(not(feature = "embed"))]
        {
            let _ = dir;
            None
        }
    }

    // ---- internals ------------------------------------------------------------------------

    fn lock(&self) -> Result<File> {
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.dir.join("lock"))?;
        f.lock()?;
        Ok(f)
    }

    /// Apply what the log has that the store has not. Appended events in id order apply
    /// incrementally; anything else (a rewrite, a merge, events out of order) rebuilds.
    /// The caller holds the lock.
    fn catch_up(&mut self) -> Result<()> {
        if let Some(w) = self.log.repair_tail()? {
            self.warnings.push(w);
        }
        let applied = self.store.applied()?;
        let len = self.log.len()?;
        if len == 0 && applied.mark.len == 0 {
            return Ok(());
        }
        let intact = applied.mark.len > 0
            && len >= applied.mark.len
            && self.log.mark(applied.mark.len)? == applied.mark;
        if !intact {
            return self.rebuild_locked();
        }
        if len == applied.mark.len {
            return Ok(());
        }
        let bytes = self.log.read_from(applied.mark.len)?;
        let parsed = log::parse(&bytes, applied.mark.len);
        let mut max = applied.max_id;
        for e in &parsed.events {
            if Some(e.id) <= max {
                return self.rebuild_locked();
            }
            max = Some(e.id);
        }
        self.warnings.extend(parsed.warnings);
        let applied = Applied {
            mark: self.log.mark(len)?,
            max_id: max,
        };
        let warnings = self.store.apply(&parsed.events, applied, false)?;
        self.warnings.extend(warnings);
        Ok(())
    }

    /// Replay the whole log, sorted by id and without duplicates. The caller holds the lock.
    fn rebuild_locked(&mut self) -> Result<()> {
        let bytes = self.log.read_from(0)?;
        let mut parsed = log::parse(&bytes, 0);
        parsed.events.sort_by_key(|e| e.id);
        parsed.events.dedup_by_key(|e| e.id);
        let applied = Applied {
            mark: self.log.mark(bytes.len() as u64)?,
            max_id: parsed.events.last().map(|e| e.id),
        };
        self.warnings.extend(parsed.warnings);
        let warnings = self.store.apply(&parsed.events, applied, true)?;
        self.warnings.extend(warnings);
        Ok(())
    }

    /// Check a command against the current state and turn it into operations.
    fn lower(&self, cmd: Command, by: &Actor) -> Result<Vec<Op>> {
        match cmd {
            Command::Create {
                kind,
                title,
                body,
                priority,
                links,
            } => {
                let title = clean_title(&title)?;
                let priority = match (kind, priority) {
                    (Kind::Task, Some(p)) => Priority::new(p)?,
                    (_, Some(_)) => {
                        return Err(Error::Invalid(format!("a {kind} has no priority")));
                    }
                    (_, None) => Priority::DEFAULT,
                };
                let mut targets: Vec<(Rel, Uuid)> = Vec::new();
                for (rel, r) in &links {
                    let d = self.node(r)?;
                    check_link(kind, *rel, &d)?;
                    if !targets.contains(&(*rel, d.id)) {
                        targets.push((*rel, d.id));
                    }
                }
                let prefix = kind.fixed_prefix().unwrap_or(&self.config.prefix);
                let node = Uuid::now_v7();
                let key = Key::new(prefix, self.store.next_number(prefix)?);
                let mut ops = vec![Op::NodeCreate {
                    node,
                    data: NodeData {
                        kind,
                        key: key.to_string(),
                        title,
                        body,
                        state: kind.initial_state(),
                        priority,
                        props: serde_json::Map::new(),
                    },
                }];
                ops.extend(targets.into_iter().map(|(rel, dst)| Op::EdgeAdd {
                    data: EdgeData {
                        src: node,
                        dst,
                        rel,
                    },
                }));
                Ok(ops)
            }
            Command::Edit { node, title, body } => {
                let n = self.node(&node)?;
                let mut patch = NodePatch::default();
                if let Some(t) = title {
                    let t = clean_title(&t)?;
                    if t != n.title {
                        patch.title = Some(t);
                    }
                }
                if let Some(b) = body
                    && b != n.body
                {
                    patch.body = Some(b);
                }
                if patch.is_empty() {
                    return Ok(Vec::new());
                }
                // The human approved the text as it was; changed text needs approving again.
                patch.state = n.kind.on_edit(n.state);
                Ok(vec![Op::NodeSet {
                    node: n.id,
                    data: patch,
                }])
            }
            Command::Move { node, to } => {
                let n = self.node(&node)?;
                self.check_move(&n, to, by)?;
                self.move_ops(&n, to, by)
            }
            Command::Ratify { node, always } => {
                let n = self.node(&node)?;
                if n.kind != Kind::Lesson {
                    return Err(Error::Invalid(format!(
                        "{} is a {}; only lessons are ratified (decisions are accepted)",
                        n.key, n.kind
                    )));
                }
                let scope = if always {
                    serde_json::Value::String("always".into())
                } else {
                    serde_json::Value::Null
                };
                let mut props = serde_json::Map::new();
                props.insert("scope".into(), scope.clone());
                if n.state == State::Ratified {
                    if !by.is_human() {
                        return Err(Error::Refused(human_only_reason(&n)));
                    }
                    let current = n
                        .props
                        .get("scope")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    if current == scope {
                        return Ok(Vec::new());
                    }
                    return Ok(vec![Op::NodeSet {
                        node: n.id,
                        data: NodePatch {
                            props: Some(props),
                            ..NodePatch::default()
                        },
                    }]);
                }
                self.check_move(&n, State::Ratified, by)?;
                let mut ops = self.move_ops(&n, State::Ratified, by)?;
                if let Some(Op::NodeSet { data, .. }) = ops.first_mut() {
                    data.props = Some(props);
                }
                Ok(ops)
            }
            Command::SetPriority { node, priority } => {
                let n = self.node(&node)?;
                let p = Priority::new(priority)?;
                if p.get() == n.priority {
                    return Ok(Vec::new());
                }
                Ok(vec![Op::NodeSet {
                    node: n.id,
                    data: NodePatch {
                        priority: Some(p),
                        ..NodePatch::default()
                    },
                }])
            }
            Command::Link { src, rel, dst } => {
                let (s, d) = (self.node(&src)?, self.node(&dst)?);
                if s.id == d.id {
                    return Err(Error::Invalid(format!("{} cannot link to itself", s.key)));
                }
                // Relating has no direction: made from either end, it is the same link.
                if self.store.edge_exists(s.id, rel, d.id)?
                    || (rel == Rel::Relates && self.store.edge_exists(d.id, rel, s.id)?)
                {
                    return Ok(Vec::new());
                }
                check_link(s.kind, rel, &d)?;
                if matches!(rel, Rel::DependsOn | Rel::Supersedes | Rel::Parent)
                    && self.store.reaches(d.id, s.id, rel)?
                {
                    return Err(Error::Refused(format!(
                        "{} already leads back to {} through `{rel}` links; this would make a cycle",
                        d.key, s.key
                    )));
                }
                Ok(vec![Op::EdgeAdd {
                    data: EdgeData {
                        src: s.id,
                        dst: d.id,
                        rel,
                    },
                }])
            }
            Command::DocLink { node, path } => {
                let n = self.node(&node)?;
                let path = crate::docs::normalize(self.root(), &path)
                    .ok_or_else(|| Error::Invalid(format!("{path} is outside the project")))?;
                if !self.root().join(&path).is_file() {
                    return Err(Error::Invalid(format!("there is no doc at {path}")));
                }
                // Linked already: the same link, made again.
                if self.store.docs_of(n.id)?.contains(&path) {
                    return Ok(Vec::new());
                }
                Ok(vec![Op::DocLink {
                    node: n.id,
                    data: DocData { path },
                }])
            }
            Command::DocUnlink { node, path } => {
                let n = self.node(&node)?;
                let path = crate::docs::normalize(self.root(), &path)
                    .ok_or_else(|| Error::Invalid(format!("{path} is outside the project")))?;
                // A doc gone keeps its links, to be taken away.
                if !self.store.docs_of(n.id)?.contains(&path) {
                    return Err(Error::Invalid(format!("{} is not linked to {path}", n.key)));
                }
                Ok(vec![Op::DocUnlink {
                    node: n.id,
                    data: DocData { path },
                }])
            }
            Command::Note { node, text } => {
                let n = self.node(&node)?;
                let text = text.trim();
                if text.is_empty() {
                    return Err(Error::Invalid("a note cannot be empty".into()));
                }
                if text.chars().count() > NOTE_MAX {
                    return Err(Error::Invalid(format!(
                        "a note is at most {NOTE_MAX} characters"
                    )));
                }
                Ok(vec![Op::NodeNote {
                    node: n.id,
                    data: NoteData {
                        text: text.to_owned(),
                    },
                }])
            }
            Command::Unlink { src, rel, dst } => {
                let (mut s, mut d) = (self.node(&src)?, self.node(&dst)?);
                // Relating has no direction: removed from either end.
                if rel == Rel::Relates
                    && !self.store.edge_exists(s.id, rel, d.id)?
                    && self.store.edge_exists(d.id, rel, s.id)?
                {
                    std::mem::swap(&mut s, &mut d);
                }
                if !self.store.edge_exists(s.id, rel, d.id)? {
                    return Err(Error::Invalid(format!(
                        "{} has no `{rel}` link to {}",
                        s.key, d.key
                    )));
                }
                Ok(vec![Op::EdgeRemove {
                    data: EdgeData {
                        src: s.id,
                        dst: d.id,
                        rel,
                    },
                }])
            }
        }
    }

    fn check_move(&self, n: &Node, to: State, by: &Actor) -> Result<()> {
        let from = n.state;
        if from == to {
            return Err(Error::Invalid(format!("{} is already {to}", n.key)));
        }
        if !n.kind.can_move(from, to) {
            let options: Vec<&str> = n
                .kind
                .next_states(from)
                .iter()
                .map(|s| s.as_str())
                .collect();
            return Err(Error::Refused(format!(
                "{} is {from}; from there it can move to: {}",
                n.key,
                options.join(", ")
            )));
        }
        if n.kind.human_only(from, to) && !by.is_human() {
            return Err(Error::Refused(human_only_reason(n)));
        }
        if self.gate(n.kind, from, to).is_some() && !by.is_human() {
            return Err(Error::Refused(format!(
                "this project keeps moving a {} to {to} for the human ([gates] in .livtk/config.toml); ask the developer to run `livtk move {} {to}` or to do it in the app",
                n.kind, n.key
            )));
        }
        if to == State::Doing {
            let (deps, _) = self.store.links(n.id, Rel::DependsOn)?;
            let waiting: Vec<String> = deps
                .iter()
                .filter(|d| !d.state.is_closed())
                .map(|d| format!("{} ({})", d.key, d.state))
                .collect();
            if !waiting.is_empty() {
                return Err(Error::Refused(format!(
                    "{} waits on {}; finish or cancel those first",
                    n.key,
                    waiting.join(", ")
                )));
            }
        }
        Ok(())
    }

    /// The operations of a checked move, with what it implies: ratifying a lesson retires the
    /// ratified lessons it supersedes; accepting a decision supersedes the accepted ones it
    /// replaces.
    fn move_ops(&self, n: &Node, to: State, by: &Actor) -> Result<Vec<Op>> {
        let set = |id: Uuid, state: State| Op::NodeSet {
            node: id,
            data: NodePatch {
                state: Some(state),
                ..NodePatch::default()
            },
        };
        let mut ops = vec![set(n.id, to)];
        let replaced = match (n.kind, to) {
            (Kind::Lesson, State::Ratified) => Some((State::Ratified, State::Retired)),
            (Kind::Decision, State::Accepted) => Some((State::Accepted, State::Superseded)),
            _ => None,
        };
        if let Some((from, into)) = replaced {
            let (older, _) = self.store.links(n.id, Rel::Supersedes)?;
            for b in older.iter().filter(|b| b.state == from) {
                let old = self.node(&b.key)?;
                self.check_move(&old, into, by)?;
                ops.push(set(old.id, into));
            }
        }
        Ok(ops)
    }
}

/// Why Claude may not take a step that belongs to the human.
fn human_only_reason(n: &Node) -> String {
    match n.kind {
        Kind::Lesson => format!(
            "only a human can ratify, reject or retire a lesson; ask the developer to run `livtk ratify {}` (or `reject`) or to decide in the app",
            n.key
        ),
        Kind::Decision => format!(
            "only a human can accept or reject a decision; ask the developer to run `livtk accept {}` (or `reject`) or to decide in the app",
            n.key
        ),
        _ => format!(
            "only a human can acknowledge {}; ask the developer to run `livtk ack {}` or to acknowledge it in the app",
            n.key, n.key
        ),
    }
}

/// Whether a node of `kind` may have a `rel` link to a node of kind `dst`: dependencies and
/// parents join tasks, supersedes joins two lessons or two decisions, anything may be learned
/// from or related to anything. The app offers only these.
pub fn link_allowed(kind: Kind, rel: Rel, dst: Kind) -> bool {
    match rel {
        Rel::DependsOn | Rel::Parent => kind == Kind::Task && dst == Kind::Task,
        Rel::Supersedes => matches!(kind, Kind::Lesson | Kind::Decision) && dst == kind,
        // Learned from, related to, and a project's own relations: anything to anything.
        _ => true,
    }
}

fn check_link(kind: Kind, rel: Rel, dst: &Node) -> Result<()> {
    if link_allowed(kind, rel, dst.kind) {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "a {kind} cannot have a `{rel}` link to {} (a {})",
            dst.key, dst.kind
        )))
    }
}

fn load_config(dir: &Path) -> Result<Config> {
    let path = dir.join("config.toml");
    let bad = |msg: String| Error::Config {
        path: path.clone(),
        msg,
    };
    let text = fs::read_to_string(&path)?;
    let config: Config = toml::from_str(&text).map_err(|e| bad(e.to_string()))?;
    if config.version > CONFIG_VERSION {
        return Err(bad(format!(
            "version {} is newer than this livtk understands; update livtk",
            config.version
        )));
    }
    validate_project_prefix(&config.prefix).map_err(|e| bad(e.to_string()))?;
    // The project's own kinds, known before its gates and its log are read.
    crate::model::declare_kinds(&config.kinds).map_err(|e| bad(e.to_string()))?;
    crate::model::declare_relations(&config.relations).map_err(|e| bad(e.to_string()))?;
    if let Some(k) = config.kinds.iter().find(|k| k.key == config.prefix) {
        return Err(bad(format!(
            "[[kinds]] `{}`: key `{}` is the project's own prefix",
            k.name, k.key
        )));
    }
    config
        .gates
        .parse()
        .map_err(|e| bad(format!("[gates] human: {e}")))?;
    // A typo must not quietly switch something off.
    if let Some(h) = config
        .hooks
        .disable
        .iter()
        .find(|h| !HOOK_NAMES.contains(&h.as_str()))
    {
        return Err(bad(format!(
            "[hooks] disable: no hook `{h}` can be switched off; the ones that can: {}",
            HOOK_NAMES.join(", ")
        )));
    }
    for (table, sections, known) in [
        (
            "session_start",
            &config.session_start,
            &SESSION_SECTIONS[..],
        ),
        (
            "user_prompt_submit",
            &config.user_prompt_submit,
            &PROMPT_SECTIONS[..],
        ),
    ] {
        if let Some(s) = sections.unknown(known) {
            return Err(bad(format!(
                "[{table}] include: no section `{s}`; the sections: {}",
                known.join(", ")
            )));
        }
    }
    Ok(config)
}

fn clean_title(title: &str) -> Result<String> {
    let t = title.trim();
    if t.is_empty() {
        return Err(Error::Invalid("a title cannot be empty".into()));
    }
    if t.contains(['\n', '\r']) {
        return Err(Error::Invalid(
            "a title is one line; put the rest in the body".into(),
        ));
    }
    if t.chars().count() > TITLE_MAX {
        return Err(Error::Invalid(format!(
            "a title is at most {TITLE_MAX} characters; put the rest in the body"
        )));
    }
    Ok(t.to_owned())
}
