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

use crate::event::{EdgeData, Event, NodeData, NodePatch, NoteData, Op};
use crate::log::{self, Log};
use crate::model::{Actor, Key, Kind, Priority, Rel, State, default_prefix, validate_prefix};
use crate::store::{Applied, Brief, Node, Snapshot, Store, Summary};
use crate::{Error, Result};

pub const DIR_NAME: &str = ".livtk";
const CONFIG_VERSION: u32 = 1;
const TITLE_MAX: usize = 200;
const NOTE_MAX: usize = 20_000;

/// `.livtk/config.toml`, committed with the project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    /// Key prefix for new nodes, as in `LIV-12`.
    pub prefix: String,
    #[serde(default, skip_serializing_if = "Hooks::is_default")]
    pub hooks: Hooks,
}

impl Config {
    /// Read and validate `config.toml` in a `.livtk/` directory, without opening the store.
    pub fn load(dir: &Path) -> Result<Config> {
        load_config(dir)
    }
}

/// What the Claude Code hooks run for this project. Every field is optional.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hooks {
    /// Formatter run on a file Claude edited; `{file}` is replaced by its path.
    pub format: Vec<String>,
    /// File extensions the formatter applies to, without the dot.
    pub format_extensions: Vec<String>,
    /// Check run before an agent in `check_agents` may stop, such as `cargo check`.
    pub check: Vec<String>,
    /// Agent types the check applies to.
    pub check_agents: Vec<String>,
}

impl Default for Hooks {
    fn default() -> Self {
        Hooks {
            format: Vec::new(),
            format_extensions: Vec::new(),
            check: Vec::new(),
            check_agents: vec!["livtk:implementer".to_owned()],
        }
    }
}

impl Hooks {
    fn is_default(&self) -> bool {
        *self == Hooks::default()
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
    CreateTask {
        title: String,
        body: String,
        priority: Option<u8>,
        depends_on: Vec<String>,
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
    /// Add a note to a node's history. Changes no state.
    Note {
        node: String,
        text: String,
    },
}

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
    log: Log,
    store: Store,
    warnings: Vec<String>,
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
                validate_prefix(&p)?;
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
        };
        let text = toml::to_string(&config).map_err(|e| Error::Config {
            path: dir.join("config.toml"),
            msg: e.to_string(),
        })?;
        fs::write(dir.join("config.toml"), text)?;
        fs::write(dir.join(".gitignore"), "livtk.db*\nlock\n")?;
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
        let store = Store::open(&dir.join("livtk.db"))?;
        let mut ws = Workspace {
            dir: dir.to_path_buf(),
            config,
            log: Log::new(dir.join("events.jsonl")),
            store,
            warnings: Vec::new(),
        };
        if !ws.store.has_current_schema()? {
            let _lock = ws.lock()?;
            if !ws.store.has_current_schema()? {
                ws.store.reset_schema()?;
            }
            ws.catch_up()?;
        }
        ws.refresh()?;
        Ok(ws)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Bring the store up to date with the log. Cheap when nothing changed (one stat and two
    /// small reads). Returns whether anything changed. Long-lived readers such as the app call
    /// this when the log file changes.
    pub fn refresh(&mut self) -> Result<bool> {
        let applied = self.store.applied()?;
        let len = self.log.len()?;
        if len == applied.mark.len && self.log.mark(len)? == applied.mark {
            return Ok(false);
        }
        let _lock = self.lock()?;
        self.catch_up()?;
        Ok(true)
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

    /// The events about one node, or the whole log, oldest first.
    pub fn history(&self, reference: Option<&str>) -> Result<Vec<Event>> {
        match reference {
            Some(r) => self.store.history(self.resolve(r)?),
            None => self.store.all_events(),
        }
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        self.store.snapshot()
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
            Command::CreateTask {
                title,
                body,
                priority,
                depends_on,
            } => {
                let title = clean_title(&title)?;
                let priority = priority
                    .map(Priority::new)
                    .transpose()?
                    .unwrap_or(Priority::DEFAULT);
                let mut deps = Vec::new();
                for r in &depends_on {
                    let d = self.resolve(r)?;
                    if !deps.contains(&d) {
                        deps.push(d);
                    }
                }
                let node = Uuid::now_v7();
                let key = Key::new(
                    &self.config.prefix,
                    self.store.next_number(&self.config.prefix)?,
                );
                let mut ops = vec![Op::NodeCreate {
                    node,
                    data: NodeData {
                        kind: Kind::Task,
                        key: key.to_string(),
                        title,
                        body,
                        state: Kind::Task.initial_state(),
                        priority,
                        props: serde_json::Map::new(),
                    },
                }];
                ops.extend(deps.into_iter().map(|dst| Op::EdgeAdd {
                    data: EdgeData {
                        src: node,
                        dst,
                        rel: Rel::DependsOn,
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
                // The acknowledgement covered the plan as it was; a changed plan needs a new one.
                if n.state == State::Acked {
                    patch.state = Some(State::Planned);
                }
                Ok(vec![Op::NodeSet {
                    node: n.id,
                    data: patch,
                }])
            }
            Command::Move { node, to } => {
                let n = self.node(&node)?;
                self.check_move(&n, to, by)?;
                Ok(vec![Op::NodeSet {
                    node: n.id,
                    data: NodePatch {
                        state: Some(to),
                        ..NodePatch::default()
                    },
                }])
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
                if self.store.edge_exists(s.id, rel, d.id)? {
                    return Ok(Vec::new());
                }
                if rel == Rel::DependsOn && self.store.reaches(d.id, s.id, Rel::DependsOn)? {
                    return Err(Error::Refused(format!(
                        "{} already depends on {} (directly or through others); this would make a cycle",
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
                let (s, d) = (self.node(&src)?, self.node(&dst)?);
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
            return Err(Error::Refused(format!(
                "only a human can acknowledge {}; ask the developer to run `livtk ack {}` or to acknowledge it in the app",
                n.key, n.key
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
    validate_prefix(&config.prefix).map_err(|e| bad(e.to_string()))?;
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
