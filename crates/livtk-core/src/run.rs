//! Runs: Claude working on a task on its own, headless, as livtk starts it.
//!
//! A run is one Claude Code session doing one stage of one task's work (plan, build, review,
//! research), as an agent. What lasts goes in the log, as notes on the task that carry the run:
//! it started; it waits for you (a question, or commands it was refused); your answer; how it
//! ended. What only matters while it runs stays beside the log, in `.livtk/runs/`, never
//! committed: its process, the stream it writes, how far livtk has read it, the tools it may use.
//!
//! Each turn is one `claude -p` process, in its own process group, writing its stream to a file:
//! it outlives whatever started it, and a turn ends with a `result` line. [`settle`] reads what
//! the turns wrote and turns it into notes, and starts the next turn when you have answered. It
//! is safe to call from anywhere and as often as wanted: the app calls it as it ticks, the CLI
//! when it is asked about runs.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::agents::{AgentFile, Place};
use crate::event::{Ending, RunData, RunMark};
use crate::model::{Actor, Kind, State};
use crate::workspace::{Command, Workspace};
use crate::{Error, Result};

/// The stage of a task's work a run does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Plan,
    Build,
    Review,
    Research,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Plan, Stage::Build, Stage::Review, Stage::Research];

    pub const fn as_str(self) -> &'static str {
        match self {
            Stage::Plan => "plan",
            Stage::Build => "build",
            Stage::Review => "review",
            Stage::Research => "research",
        }
    }

    /// The stage a task in `state` is ready for: planning from the backlog, building once
    /// acknowledged, reviewing in review.
    pub fn for_state(kind: Kind, state: State) -> Option<Stage> {
        if kind != Kind::Task {
            return None;
        }
        match state {
            State::Backlog => Some(Stage::Plan),
            State::Acked => Some(Stage::Build),
            State::Review => Some(Stage::Review),
            _ => None,
        }
    }

    /// livtk's own agent for the stage; planning runs as the session itself, with the plan skill.
    pub const fn default_agent(self) -> Option<&'static str> {
        match self {
            Stage::Plan => None,
            Stage::Build => Some("livtk:implementer"),
            Stage::Review => Some("livtk:refuter"),
            Stage::Research => Some("livtk:researcher"),
        }
    }

    /// Whether a turn that ended, on a task now in `state`, finished the stage's work: a plan
    /// once the task is planned, a build once it is in review; a review or research is one turn.
    pub fn finished(self, state: State) -> bool {
        match self {
            Stage::Plan => state != State::Backlog,
            Stage::Build => matches!(state, State::Review | State::Done),
            Stage::Review | Stage::Research => true,
        }
    }

    /// What the run is asked first, for task `key` titled `title`.
    pub fn prompt(self, key: &str, title: &str) -> String {
        match self {
            Stage::Plan => format!(
                "Plan livtk task {key} ({title}) with the livtk:plan skill: read it with \
                 `livtk show {key}`, write the plan into it and move it to planned. If you need \
                 the human first, end your turn with your question."
            ),
            Stage::Build => format!(
                "Build livtk task {key} ({title}): it is acknowledged. Follow the livtk:work \
                 skill: move it to doing, build it to its acceptance check, run the checks, \
                 commit, and move it to review. If you need the human, end your turn with your \
                 question."
            ),
            Stage::Review => format!(
                "Review livtk task {key} ({title}): try to prove it is not done, against its \
                 acceptance check, and record your verdict on it with `livtk note`."
            ),
            Stage::Research => format!(
                "Research what livtk task {key} ({title}) asks, and record what you find with \
                 livtk."
            ),
        }
    }
}

impl std::str::FromStr for Stage {
    type Err = Error;
    fn from_str(s: &str) -> Result<Stage> {
        Stage::ALL
            .into_iter()
            .find(|st| st.as_str() == s.trim())
            .ok_or_else(|| {
                Error::Invalid(format!("no stage `{s}`: plan, build, review or research"))
            })
    }
}

/// Refused, saying why, when task `n` may not start `stage` now: every way a run starts (a new
/// one, or a stopped one continued) asks here.
fn free_to_run(ws: &Workspace, n: &crate::Node, stage: Stage) -> Result<()> {
    match waits_for(stage, n.blocked, &ws.links(&n.key)?.depends_on) {
        Some(waits) => Err(Error::Refused(format!(
            "{} {waits}: it can be built once that is done",
            n.key
        ))),
        None => Ok(()),
    }
}

/// What keeps a task from running `stage`, said as "waits for KEY, KEY": a build waits for the
/// tasks it depends on that are neither done nor canceled (`blocked`, as the store tells it).
/// Planning and reviewing never wait.
pub fn waits_for(stage: Stage, blocked: bool, depends_on: &[crate::Brief]) -> Option<String> {
    if stage != Stage::Build || !blocked {
        return None;
    }
    let open: Vec<&str> = depends_on
        .iter()
        .filter(|b| !matches!(b.state, State::Done | State::Canceled))
        .map(|b| b.key.as_str())
        .collect();
    Some(if open.is_empty() {
        "waits for unfinished work".to_owned()
    } else {
        format!("waits for {}", open.join(", "))
    })
}

/// The agent suggested for `stage`: the first agent file that says it suits the stage (the
/// project's, then yours, then livtk's), else livtk's own for the stage.
pub fn suggest(stage: Stage, files: &[AgentFile]) -> Option<String> {
    let by_file = [Place::Project, Place::Yours, Place::Livtk]
        .into_iter()
        .find_map(|place| {
            files
                .iter()
                .find(|f| f.place == place && f.agent.stage == Some(stage))
        })
        .map(AgentFile::run_name);
    by_file.or_else(|| stage.default_agent().map(str::to_owned))
}

/// A run as the log tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub node: Uuid,
    pub key: String,
    pub session: String,
    pub agent: Option<String>,
    pub stage: Stage,
    /// When it started, as the log writes times.
    pub started: String,
    /// Its latest note's mark, text and event.
    pub last: RunMark,
    pub text: String,
    pub last_id: Uuid,
    /// Your answer, while it waits for one: given since it asked.
    pub answer: Option<Answer>,
}

/// Your answer to a run that waits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Run what it asked for: the tools it named.
    Allowed(Vec<String>),
    Denied,
    /// A note of yours: the next thing it reads.
    Note(String),
}

impl Run {
    /// It waits for you, and you have not answered.
    pub fn needs_you(&self) -> bool {
        matches!(self.last, RunMark::Asks { .. }) && self.answer.is_none()
    }

    pub fn ended(&self) -> Option<Ending> {
        match self.last {
            RunMark::Ended { how } => Some(how),
            _ => None,
        }
    }

    /// What it asks to run, when it waits on a permission.
    pub fn asks_to_run(&self) -> &[String] {
        match &self.last {
            RunMark::Asks { allow } => allow,
            _ => &[],
        }
    }
}

// ---- what lives beside the log --------------------------------------------------------------

/// A run's state beside the log: what only matters while it runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Live {
    pub session: String,
    pub key: String,
    pub agent: Option<String>,
    pub stage: Stage,
    /// Where it works: the project's folder.
    pub cwd: PathBuf,
    /// The tools it may use without asking: its stage's, and what you have allowed since.
    pub allowed: Vec<String>,
    /// How far into its stream livtk has read, in bytes.
    pub read: u64,
    /// Its turn's process, while one runs.
    pub pid: Option<u32>,
    /// It was stopped, and how to say so when its turn ends.
    pub stopping: Option<Ending>,
    /// When it was asked to stop, in seconds since the epoch: a turn still going a while
    /// after is made to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_asked: Option<u64>,
}

/// The folder runs keep their state in, inside the project's `.livtk/`: never committed.
pub fn runs_dir(livtk: &Path) -> PathBuf {
    livtk.join(RUNS)
}

/// Where runs keep their state in `.livtk/`, and where builds' worktrees are in it: the path's
/// parts, so a guard can know a worktree's files from `.livtk/`'s own.
pub const RUNS: &str = "runs";
pub const WORKTREES: &str = "worktrees";

fn live_path(livtk: &Path, session: &str) -> PathBuf {
    runs_dir(livtk).join(format!("{session}.json"))
}

/// The stream a run's turns write, one JSON object a line.
pub fn stream_path(livtk: &Path, session: &str) -> PathBuf {
    runs_dir(livtk).join(format!("{session}.jsonl"))
}

fn ensure_dir(livtk: &Path) -> Result<PathBuf> {
    let dir = runs_dir(livtk);
    fs::create_dir_all(&dir)?;
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        fs::write(ignore, "*\n")?;
    }
    Ok(dir)
}

fn save(livtk: &Path, live: &Live) -> Result<()> {
    ensure_dir(livtk)?;
    let path = live_path(livtk, &live.session);
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(live)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

/// Every run with state beside the log.
pub fn lives(livtk: &Path) -> Vec<Live> {
    let Ok(entries) = fs::read_dir(runs_dir(livtk)) else {
        return Vec::new();
    };
    let mut out: Vec<Live> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| serde_json::from_slice(&fs::read(p).ok()?).ok())
        .collect();
    out.sort_by(|a, b| a.session.cmp(&b.session));
    out
}

pub fn live(livtk: &Path, session: &str) -> Option<Live> {
    serde_json::from_slice(&fs::read(live_path(livtk, session)).ok()?).ok()
}

/// Whether process `pid` is still the turn of session `session`: alive, and running it (a pid
/// can be reused, and a finished child left unreaped no longer names its session). When that
/// cannot be told (`ps` would not run, on a machine out of processes, say), it is taken to run:
/// a turn is never given up on, nor its stop skipped, for want of an answer.
pub fn running(pid: u32, session: &str) -> bool {
    // No such process, for certain: gone.
    if !alive(pid) {
        return false;
    }
    match Process::new("ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(o) if o.status.success() || !o.stdout.is_empty() => {
            String::from_utf8_lossy(&o.stdout).contains(session)
        }
        // `ps` exits 1 for a pid it does not find.
        Ok(o) if o.status.code() == Some(1) => false,
        _ => true,
    }
}

/// Whether a process `pid` exists: the system asked directly, no process started for it.
fn alive(pid: u32) -> bool {
    let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return false;
    };
    match rustix::process::test_kill_process(pid) {
        Ok(()) => true,
        // There, but not ours to signal: alive all the same.
        Err(e) => e == rustix::io::Errno::PERM,
    }
}

/// Whether the run's turn is going: it is writing.
pub fn writing(live: &Live) -> bool {
    live.pid.is_some_and(|pid| running(pid, &live.session))
}

// ---- starting, answering and stopping -------------------------------------------------------

/// What runs a turn: the `claude` command.
#[derive(Clone, Debug)]
pub struct Runner {
    pub claude: PathBuf,
}

impl Runner {
    /// `claude` as livtk finds it.
    pub fn find() -> Option<Runner> {
        crate::claude::command().map(|claude| Runner { claude })
    }

    /// Start a run on task `key`: the stage its state is ready for unless `stage` says, as
    /// `agent`, else the task's own agent, else the one suggested. Refused while another run
    /// on it goes or waits.
    pub fn start(
        &self,
        ws: &mut Workspace,
        key: &str,
        stage: Option<Stage>,
        agent: Option<String>,
        files: &[AgentFile],
        by: &Actor,
    ) -> Result<Live> {
        settle(ws, self)?;
        let _hold = Hold::wait(ws.dir())?;
        self.start_held(ws, key, stage, agent, files, by)
    }

    /// Start a run, the runs' lock already held (by `start`, or by a settle starting runs on
    /// its own).
    fn start_held(
        &self,
        ws: &mut Workspace,
        key: &str,
        stage: Option<Stage>,
        agent: Option<String>,
        files: &[AgentFile],
        by: &Actor,
    ) -> Result<Live> {
        let n = ws.node(key)?;
        if let Some(r) = ws
            .runs()?
            .into_iter()
            .find(|r| r.node == n.id && r.ended().is_none())
        {
            return Err(Error::Refused(format!(
                "{} has a run going ({}); stop it first",
                n.key,
                short(&r.session)
            )));
        }
        let stage = stage
            .or_else(|| Stage::for_state(n.kind, n.state))
            .ok_or_else(|| {
                Error::Refused(format!(
                    "{} is {}: nothing to run until it is in the backlog, acknowledged or in \
                     review; name a stage to run anyway",
                    n.key, n.state
                ))
            })?;
        free_to_run(ws, &n, stage)?;
        // The task's own agent is the one that builds it; the other stages run as theirs.
        let own = (stage == Stage::Build)
            .then(|| {
                n.props
                    .get("agent")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .flatten();
        let agent = agent.or(own).or_else(|| suggest(stage, files));
        let live = Live {
            session: Uuid::now_v7().to_string(),
            key: n.key.clone(),
            agent: agent.clone(),
            stage,
            cwd: ws.root().to_path_buf(),
            allowed: crate::permits::permits(stage, &ws.config().runs.rules()).allow,
            read: 0,
            pid: None,
            stopping: None,
            stop_asked: None,
        };
        let text = match &agent {
            Some(a) => format!("Started to {} it, as {a}.", stage.as_str()),
            None => format!("Started to {} it.", stage.as_str()),
        };
        note(ws, &n.key, &live, RunMark::Started, &text, by)?;
        let mut live = live;
        let prompt = stage.prompt(&n.key, &n.title);
        let run = ws
            .runs()?
            .into_iter()
            .rev()
            .find(|r| r.session == live.session)
            .ok_or_else(|| Error::Refused("the run's start was not recorded".into()))?;
        // Where it works, made now that its start is on record: one that cannot be made ends
        // the run, saying why, so it neither waits nor holds a build's place.
        match workplace(ws, &n.key, stage) {
            Ok(cwd) => live.cwd = cwd,
            Err(e) => {
                let why = format!("Its worktree could not be made: {e}");
                end(ws, &run, &mut live, Ending::Failed, &why, &claude())?;
                return Err(e);
            }
        }
        self.turn_or_fail(ws, &run, &mut live, &prompt, false)?;
        Ok(live)
    }

    /// Start a turn; if it cannot start, the run ends as failed, saying why, and the error is
    /// returned.
    fn turn_or_fail(
        &self,
        ws: &mut Workspace,
        run: &Run,
        live: &mut Live,
        prompt: &str,
        resume: bool,
    ) -> Result<()> {
        // Refused outright, as the project says now: never asked.
        let deny = crate::permits::permits(live.stage, &ws.config().runs.rules()).deny;
        match self.turn(ws.dir(), live, prompt, resume, &deny) {
            Ok(()) => Ok(()),
            Err(e) => {
                end(
                    ws,
                    run,
                    live,
                    Ending::Failed,
                    &format!("It could not start: {e}"),
                    &claude(),
                )?;
                Err(e)
            }
        }
    }

    /// Start a turn of `live`'s session with `prompt`: the first, or resumed.
    fn turn(
        &self,
        livtk: &Path,
        live: &mut Live,
        prompt: &str,
        resume: bool,
        deny: &[String],
    ) -> Result<()> {
        ensure_dir(livtk)?;
        let out = OpenOptions::new()
            .create(true)
            .append(true)
            .open(stream_path(livtk, &live.session))?;
        let err = OpenOptions::new()
            .create(true)
            .append(true)
            .open(runs_dir(livtk).join(format!("{}.err", live.session)))?;
        let mut cmd = Process::new(&self.claude);
        cmd.arg("-p")
            .arg(prompt)
            .args([
                if resume { "--resume" } else { "--session-id" },
                &live.session,
            ])
            .args(["--output-format", "stream-json", "--verbose"])
            .args(["--permission-prompts", "none"]);
        if let Some(a) = &live.agent {
            cmd.args(["--agent", a]);
        }
        if live.stage == Stage::Build {
            cmd.args(["--permission-mode", "acceptEdits"]);
        }
        if !live.allowed.is_empty() {
            cmd.arg("--allowedTools").args(&live.allowed);
        }
        if !deny.is_empty() {
            cmd.arg("--disallowedTools").args(deny);
        }
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd
            .current_dir(&live.cwd)
            .env("PATH", self.turn_path())
            .env("LIVTK_TASK", &live.key)
            .env("LIVTK_SESSION", &live.session)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .spawn()
            .map_err(|e| Error::Refused(format!("claude would not start: {e}")))?;
        live.pid = Some(child.id());
        save(livtk, live)?;
        // Reaped when it ends, while this process lives; after, the system does.
        std::thread::spawn(move || child.wait());
        Ok(())
    }

    /// The PATH a turn runs with: the livtk that started it, `claude`'s folder and the usual
    /// install folders first, then the PATH this process has. An app started from the Finder
    /// has a short one, without `livtk`, `cargo` or Homebrew's tools.
    fn turn_path(&self) -> std::ffi::OsString {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        // The livtk CLI beside this process (in the app's bundle, or the CLI itself).
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .filter(|dir| dir.join("livtk").is_file());
        let mut dirs: Vec<PathBuf> = beside
            .into_iter()
            .chain(self.claude.parent().map(Path::to_path_buf))
            .chain(
                home.iter()
                    .flat_map(|h| [h.join(".local/bin"), h.join(".cargo/bin")]),
            )
            .chain(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from))
            .collect();
        dirs.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let mut seen = std::collections::HashSet::new();
        dirs.retain(|d| seen.insert(d.clone()));
        std::env::join_paths(dirs).unwrap_or_default()
    }

    /// Stop task `key`'s run: its turn ends cleanly (the work kept), and the run ends `how`
    /// (stopped, or stopped by a quit). A run waiting for you ends at once.
    pub fn stop(&self, ws: &mut Workspace, key: &str, how: Ending, by: &Actor) -> Result<()> {
        let _hold = Hold::wait(ws.dir())?;
        let n = ws.node(key)?;
        let Some(run) = ws
            .runs()?
            .into_iter()
            .find(|r| r.node == n.id && r.ended().is_none())
        else {
            return Err(Error::Refused(format!("{} has no run going", n.key)));
        };
        let livtk = ws.dir().to_path_buf();
        let mut live = live(&livtk, &run.session).unwrap_or_else(|| gone(&run, ws.root()));
        if writing(&live) {
            live.stopping = Some(how);
            live.stop_asked = Some(now_secs());
            save(&livtk, &live)?;
            return signal_turn(&live, rustix::process::Signal::INT).map(|_| ());
        }
        end(ws, &run, &mut live, how, stopped_text(how), by)
    }

    /// Answer task `key`'s waiting run: let it run what it asked for, or not. A note of yours
    /// answers a question the same way, on its own.
    /// `always`, with `allow`: the project's runs of this stage may run what it asked for from
    /// now on (each command's kind, as `permits::always_rule` takes it), never asked again.
    pub fn answer(
        &self,
        ws: &mut Workspace,
        key: &str,
        allow: bool,
        always: bool,
        by: &Actor,
    ) -> Result<()> {
        if !by.is_human() {
            return Err(Error::Refused(
                "letting a run do what it asks is the human's decision".into(),
            ));
        }
        let n = ws.node(key)?;
        let Some(run) = ws
            .runs()?
            .into_iter()
            .find(|r| r.node == n.id && r.needs_you())
        else {
            return Err(Error::Refused(format!(
                "{} has no run waiting for you",
                n.key
            )));
        };
        let asks = run.asks_to_run().to_vec();
        if asks.is_empty() {
            return Err(Error::Refused(format!(
                "{}'s run asks a question, not to run something: answer it with a note",
                n.key
            )));
        }
        if allow && always {
            ws.allow_always(run.stage, &asks)?;
        }
        let (mark, text) = if allow && always {
            (
                RunMark::Allowed,
                format!("Allowed from now on: {}", asks.join(", ")),
            )
        } else if allow {
            (RunMark::Allowed, format!("Allowed: {}", asks.join(", ")))
        } else {
            (RunMark::Denied, format!("Denied: {}", asks.join(", ")))
        };
        let live = live(ws.dir(), &run.session).unwrap_or_else(|| gone(&run, ws.root()));
        note(ws, &n.key, &live, mark, &text, by)?;
        settle(ws, self)?;
        Ok(())
    }

    /// Start the next turn of a run that waits and has been answered.
    fn resume(&self, ws: &mut Workspace, run: &Run, answer: &Answer) -> Result<()> {
        let livtk = ws.dir().to_path_buf();
        let mut live = live(&livtk, &run.session).unwrap_or_else(|| gone(run, ws.root()));
        let prompt = match answer {
            Answer::Allowed(tools) => {
                for t in tools {
                    if !live.allowed.contains(t) {
                        live.allowed.push(t.clone());
                    }
                }
                format!("You may now run: {}. Go on.", tools.join(", "))
            }
            Answer::Denied => format!(
                "Not allowed: {}. Go on without it, or say what you need.",
                run.asks_to_run().join(", ")
            ),
            Answer::Note(text) => text.clone(),
        };
        self.turn_or_fail(ws, run, &mut live, &prompt, true)
    }

    /// Continue a run that stopped (by hand, or when the app quit): a new turn of the same
    /// session, where it left off.
    pub fn resume_stopped(&self, ws: &mut Workspace, key: &str, by: &Actor) -> Result<()> {
        let _hold = Hold::wait(ws.dir())?;
        let n = ws.node(key)?;
        let Some(run) =
            ws.runs()?.into_iter().rev().find(|r| {
                r.node == n.id && matches!(r.ended(), Some(Ending::Stopped | Ending::Quit))
            })
        else {
            return Err(Error::Refused(format!("{} has no stopped run", n.key)));
        };
        if ws
            .runs()?
            .iter()
            .any(|r| r.node == n.id && r.ended().is_none())
        {
            return Err(Error::Refused(format!("{} has a run going", n.key)));
        }
        free_to_run(ws, &n, run.stage)?;
        let livtk = ws.dir().to_path_buf();
        let mut live = live(&livtk, &run.session).unwrap_or_else(|| gone(&run, ws.root()));
        live.stopping = None;
        live.stop_asked = None;
        note(
            ws,
            &n.key,
            &live,
            RunMark::Started,
            "Continued where it left off.",
            by,
        )?;
        let run = ws
            .runs()?
            .into_iter()
            .rev()
            .find(|r| r.session == live.session)
            .unwrap_or(run);
        self.turn_or_fail(ws, &run, &mut live, "Continue where you left off.", true)
    }
}

/// Leave task `key`'s run that a quit stopped as it is: noted as stopped for good, so it is not
/// offered to continue again.
pub fn leave_stopped(ws: &mut Workspace, key: &str, by: &Actor) -> Result<()> {
    let n = ws.node(key)?;
    let Some(run) = ws
        .runs()?
        .into_iter()
        .rev()
        .find(|r| r.node == n.id && r.ended() == Some(Ending::Quit))
    else {
        return Err(Error::Refused(format!(
            "{} has no run a quit stopped",
            n.key
        )));
    };
    let mut live = live(ws.dir(), &run.session).unwrap_or_else(|| gone(&run, ws.root()));
    end(ws, &run, &mut live, Ending::Stopped, "Left stopped.", by)
}

/// A run's state, rebuilt from the log when its file is gone (another machine, a clean).
fn gone(run: &Run, root: &Path) -> Live {
    Live {
        session: run.session.clone(),
        key: run.key.clone(),
        agent: run.agent.clone(),
        stage: run.stage,
        cwd: root.to_path_buf(),
        allowed: crate::permits::defaults(run.stage),
        read: 0,
        pid: None,
        stopping: None,
        stop_asked: None,
    }
}

const fn stopped_text(how: Ending) -> &'static str {
    match how {
        Ending::Quit => "Stopped when the app quit; the work so far is kept.",
        _ => "Stopped; the work so far is kept.",
    }
}

/// How long a turn asked to stop has to end on its own (SIGINT: Claude ends the turn cleanly)
/// before it is told to (SIGTERM), and then made to (SIGKILL). SIGINT can be ignored: a turn
/// started by a program run in the background of a shell script inherits it ignored.
const STOP_TERM: u64 = 3;
const STOP_KILL: u64 = 8;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// End `live`'s turn at once, if it is still going: its process group killed. The one way
/// anything (a stop that waited too long, a test cleaning up) kills a turn.
pub fn kill_turn(live: &Live) -> Result<bool> {
    signal_turn(live, rustix::process::Signal::KILL)
}

/// `sig` to `live`'s turn, only while its pid still runs that turn: a turn that has ended
/// leaves its pid in the record until the run ends, and the system may have handed that pid,
/// and so its process group, to another process since. Whether it was sent.
fn signal_turn(live: &Live, sig: rustix::process::Signal) -> Result<bool> {
    match live.pid.filter(|&p| running(p, &live.session)) {
        Some(pid) => signal(pid, sig).map(|()| true),
        None => Ok(false),
    }
}

/// `sig` to the turn's process group (its own, led by `pid`), by the system call itself, so a
/// busy machine cannot keep it from going.
fn signal(pid: u32, sig: rustix::process::Signal) -> Result<()> {
    let group = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .ok_or_else(|| Error::Refused(format!("no process group {pid}")))?;
    rustix::process::kill_process_group(group, sig).map_err(|e| Error::Io(e.into()))
}

/// Write a run's note on task `key`, in the run's own session.
fn note(
    ws: &mut Workspace,
    key: &str,
    live: &Live,
    mark: RunMark,
    text: &str,
    by: &Actor,
) -> Result<()> {
    let run = RunData {
        session: live.session.clone(),
        agent: live.agent.clone(),
        stage: live.stage,
        mark,
    };
    let by = match by {
        Actor::Human => Actor::Human,
        Actor::Claude { .. } => Actor::Claude {
            agent: live.agent.as_deref().map(agent_short),
        },
    };
    let was = ws.session().map(str::to_owned);
    ws.set_session(Some(live.session.clone()));
    let done = ws.execute(
        Command::RunNote {
            node: key.to_owned(),
            text: text.to_owned(),
            run,
        },
        &by,
    );
    ws.set_session(was);
    done.map(|_| ())
}

/// `livtk:implementer` as the log signs it: `implementer`.
fn agent_short(agent: &str) -> String {
    agent.rsplit(':').next().unwrap_or(agent).to_owned()
}

fn end(
    ws: &mut Workspace,
    run: &Run,
    live: &mut Live,
    how: Ending,
    text: &str,
    by: &Actor,
) -> Result<()> {
    note(ws, &run.key, live, RunMark::Ended { how }, text, by)?;
    live.pid = None;
    live.stopping = None;
    live.stop_asked = None;
    save(ws.dir(), live)
}

fn short(session: &str) -> &str {
    &session[..8.min(session.len())]
}

// ---- reading what the turns wrote -----------------------------------------------------------

/// A turn's end, as its stream's `result` line says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnEnd {
    /// What Claude said last.
    pub text: String,
    /// It ended on an error (or was interrupted).
    pub error: bool,
    /// The tools it was refused, as permission rules: each simple command of a compound one.
    pub denied: Vec<String>,
}

/// The turn ends in `path` from byte `from`, and where reading stopped: after the last whole
/// line.
pub fn read_turns(path: &Path, from: u64) -> std::io::Result<(Vec<TurnEnd>, u64)> {
    let Ok(mut f) = File::open(path) else {
        return Ok((Vec::new(), from));
    };
    f.seek(SeekFrom::Start(from))?;
    let mut reader = BufReader::new(f);
    let mut at = from;
    let mut out = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 || !line.ends_with('\n') {
            return Ok((out, at));
        }
        at += n as u64;
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if v["type"] == "result" {
            out.push(TurnEnd {
                text: v["result"].as_str().unwrap_or_default().trim().to_owned(),
                error: v["is_error"].as_bool().unwrap_or(false)
                    || v["subtype"].as_str().is_some_and(|s| s != "success"),
                denied: denials(&v["permission_denials"]),
            });
        }
    }
}

/// The permission rules for what a turn was refused: `Bash(cmd)` for each simple command of a
/// shell line, the tool's name for any other tool.
fn denials(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in v.as_array().into_iter().flatten() {
        let tool = d["tool_name"].as_str().unwrap_or_default();
        let rules: Vec<String> = match (tool, d["tool_input"]["command"].as_str()) {
            ("Bash", Some(cmd)) => simple_commands(cmd)
                .into_iter()
                .map(|c| format!("Bash({c})"))
                .collect(),
            ("", _) => Vec::new(),
            (t, _) => vec![t.to_owned()],
        };
        for r in rules {
            if !out.contains(&r) {
                out.push(r);
            }
        }
    }
    out
}

/// A shell line's simple commands: split at `&&`, `||`, `;`, `|` and newlines, outside quotes.
/// A here-document (`<<EOF`, `<<'EOF'`, `<<-"END-X"`, `<<\EOF`; not `<<<` or a shift in
/// `$((…))`) is its command's input: its lines, to the line that ends it, are no commands, and
/// several on one line are skipped in turn.
pub fn simple_commands(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    // Here-documents opened on this line, by their ending words, in order.
    let mut docs: Vec<String> = Vec::new();
    // How deep in `$((`…`))` it is: a `<<` there shifts.
    let mut arith = 0usize;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                cur.push(c);
            }
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                cur.push(c);
            }
            (None, '$') if chars.get(i + 1) == Some(&'(') && chars.get(i + 2) == Some(&'(') => {
                arith += 1;
                cur.push_str("$((");
                i += 2;
            }
            (None, ')') if arith > 0 && chars.get(i + 1) == Some(&')') => {
                arith -= 1;
                cur.push_str("))");
                i += 1;
            }
            (None, '<')
                if arith == 0
                    && chars.get(i + 1) == Some(&'<')
                    && chars.get(i + 2) != Some(&'<')
                    && (i == 0 || chars[i - 1] != '<') =>
            {
                let (word, used) = doc_word(&chars[i + 2..]);
                cur.extend(&chars[i..i + 2 + used]);
                i += 1 + used;
                if let Some(w) = word {
                    docs.push(w);
                }
            }
            (None, '&' | '|') if chars.get(i + 1) == Some(&c) => {
                i += 1;
                out.push(std::mem::take(&mut cur));
            }
            (None, ';' | '|') => out.push(std::mem::take(&mut cur)),
            (None, '\n') => {
                out.push(std::mem::take(&mut cur));
                // Each here-document's lines, to its ending word, in turn.
                for end in std::mem::take(&mut docs) {
                    loop {
                        let start = i + 1;
                        let stop = chars
                            .get(start..)
                            .and_then(|rest| rest.iter().position(|c| *c == '\n'))
                            .map_or(chars.len(), |n| start + n);
                        let text: String = chars.get(start..stop).unwrap_or(&[]).iter().collect();
                        i = stop;
                        if text.trim() == end || stop >= chars.len() {
                            break;
                        }
                    }
                }
            }
            (None, c) => cur.push(c),
        }
        i += 1;
    }
    out.push(cur);
    out.into_iter()
        .map(|c| c.trim().to_owned())
        .filter(|c| !c.is_empty())
        .collect()
}

/// The ending word of a here-document whose `<<` came just before `rest`, and how many of
/// `rest`'s chars it took: `-`, spaces, then the word, bare, quoted or after a backslash.
fn doc_word(rest: &[char]) -> (Option<String>, usize) {
    let mut n = 0;
    if rest.first() == Some(&'-') {
        n += 1;
    }
    while rest.get(n).is_some_and(|c| *c == ' ' || *c == '\t') {
        n += 1;
    }
    let mut word = String::new();
    match rest.get(n).copied() {
        Some(q @ ('\'' | '"')) => {
            n += 1;
            while let Some(&c) = rest.get(n) {
                n += 1;
                if c == q {
                    break;
                }
                word.push(c);
            }
        }
        _ => {
            if rest.get(n) == Some(&'\\') {
                n += 1;
            }
            while let Some(&c) = rest.get(n) {
                if c.is_whitespace() || matches!(c, ';' | '|' | '&' | '<' | '>' | '(' | ')') {
                    break;
                }
                word.push(c);
                n += 1;
            }
        }
    }
    // A number after `<<` is no word a here-document ends at (`1<<2` outside `$((`)).
    let word = (!word.is_empty() && !word.chars().all(|c| c.is_ascii_digit())).then_some(word);
    (word, n)
}

// ---- settling -------------------------------------------------------------------------------

/// How long a settle may hold the runs before another may take them: one stuck never blocks.
const SETTLE_HOLD: Duration = Duration::from_secs(60);

/// Bring the log up to what the runs did: each turn that ended becomes a note (it waits for
/// you, it asks to run something, or it ended), a turn whose process went without a word ends
/// its run as failed, and a run you have answered starts its next turn. Returns whether
/// anything changed. Another settle going at the same time makes this one a no-op.
pub fn settle(ws: &mut Workspace, runner: &Runner) -> Result<bool> {
    let livtk = ws.dir().to_path_buf();
    // Nothing has run here, and nothing starts on its own: nothing to do.
    if !runs_dir(&livtk).exists() && !ws.config().runs.auto {
        return Ok(false);
    }
    ensure_dir(&livtk)?;
    let Some(_hold) = Hold::take(&hold_path(&livtk)) else {
        return Ok(false);
    };
    let mut changed = false;
    let runs = ws.runs()?;
    for mut live in lives(&livtk) {
        let Some(run) = runs
            .iter()
            .rev()
            .find(|r| r.session == live.session)
            .cloned()
        else {
            continue;
        };
        if run.ended().is_some() {
            continue;
        }
        if writing(&live) {
            // Asked to stop, and still going: made to, harder as it goes on.
            if let Some(asked) = live.stop_asked {
                let waited = now_secs().saturating_sub(asked);
                if waited >= STOP_KILL {
                    let _ = signal_turn(&live, rustix::process::Signal::KILL);
                } else if waited >= STOP_TERM {
                    let _ = signal_turn(&live, rustix::process::Signal::TERM);
                }
            }
            continue;
        }
        let (turns, read) = read_turns(&stream_path(&livtk, &live.session), live.read)?;
        live.read = read;
        if live.pid.take().is_some() && turns.is_empty() {
            // Its process went, and said nothing: the run failed.
            let how = live.stopping.unwrap_or(Ending::Failed);
            let text = match how {
                Ending::Failed => "The run stopped without a word; see .livtk/runs/ for why.",
                h => stopped_text(h),
            };
            end(ws, &run, &mut live, how, text, &claude())?;
            changed = true;
            continue;
        }
        let Some(turn) = turns.last() else {
            save(&livtk, &live)?;
            continue;
        };
        let state = ws.node(&run.key)?.state;
        changed = true;
        if let Some(how) = live.stopping {
            end(ws, &run, &mut live, how, stopped_text(how), &claude())?;
        } else if !turn.denied.is_empty() {
            // What it was refused outright (never allowed, or refused by the project) is said and
            // answered at once; only the rest waits for the human.
            let deny = crate::permits::permits(live.stage, &ws.config().runs.rules()).deny;
            let (never, asks): (Vec<String>, Vec<String>) = turn
                .denied
                .iter()
                .cloned()
                .partition(|r| deny.iter().any(|d| crate::permits::covers(d, r)));
            let (shown, asked) = if asks.is_empty() {
                (&never, &never)
            } else {
                (&asks, &asks)
            };
            let mut text = format!("{}\n\nAsks to run: {}", said(&turn.text), shown.join(", "));
            if !never.is_empty() && !asks.is_empty() {
                text.push_str(&format!("\n\nNot allowed, ever: {}", never.join(", ")));
            }
            note(
                ws,
                &run.key,
                &live,
                RunMark::Asks {
                    allow: asked.clone(),
                },
                &text,
                &claude(),
            )?;
            if asks.is_empty() {
                note(
                    ws,
                    &run.key,
                    &live,
                    RunMark::Denied,
                    &format!("Not allowed, ever: {}", never.join(", ")),
                    &claude(),
                )?;
            }
            save(&livtk, &live)?;
        } else if turn.error {
            end(
                ws,
                &run,
                &mut live,
                Ending::Failed,
                &said(&turn.text),
                &claude(),
            )?;
        } else if run.stage.finished(state) {
            end(
                ws,
                &run,
                &mut live,
                Ending::Done,
                &said(&turn.text),
                &claude(),
            )?;
        } else {
            note(
                ws,
                &run.key,
                &live,
                RunMark::Asks { allow: Vec::new() },
                &said(&turn.text),
                &claude(),
            )?;
            save(&livtk, &live)?;
        }
    }
    changed |= merge_done(ws)?;
    changed |= start_on_their_own(ws, runner)?;
    // Answered: the next turn, once (a turn already started on the answer is left to run).
    for run in ws.runs()? {
        let started = live(&livtk, &run.session).is_some_and(|l| l.pid.is_some());
        if let (RunMark::Asks { .. }, Some(answer), false) = (&run.last, &run.answer, started) {
            // One that cannot start has ended its run, saying why: the others go on.
            let _ = runner.resume(ws, &run, answer);
            changed = true;
        }
    }
    Ok(changed)
}

fn claude() -> Actor {
    Actor::Claude { agent: None }
}

/// What Claude said, never nothing: a note cannot be empty.
fn said(text: &str) -> String {
    if text.trim().is_empty() {
        "(It said nothing.)".to_owned()
    } else {
        text.trim().to_owned()
    }
}

// ---- builds in their own worktrees, merged when done ----------------------------------------

/// Where builds' worktrees are, beside the runs: never committed.
fn worktrees_dir(livtk: &Path) -> PathBuf {
    runs_dir(livtk).join(WORKTREES)
}

/// The branch task `key`'s builds work on.
pub fn branch(key: &str) -> String {
    format!("livtk/{}", key.to_lowercase())
}

/// Task `key`'s worktree, if its builds have one.
pub fn worktree(livtk: &Path, key: &str) -> Option<PathBuf> {
    let path = worktrees_dir(livtk).join(key);
    path.join(".git").exists().then_some(path)
}

/// `git args` in `dir`: its output, or what git said went wrong.
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Process::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Refused(format!("git would not run: {e}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(Error::Refused(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ))
    }
}

/// Whether task `key`'s builds work in their own worktree: as it says for itself, else as the
/// project does.
pub fn wants_worktree(ws: &Workspace, key: &str) -> bool {
    ws.node(key)
        .ok()
        .and_then(|n| n.props.get("worktree").and_then(Value::as_bool))
        .unwrap_or(ws.config().runs.worktree)
}

/// Where a run of `stage` on task `key` works: a build in its own worktree (made on its first
/// build, on its own branch from the checkout's HEAD) when the project says so and is a git
/// repository; a review where the build was; else the project's folder.
fn workplace(ws: &Workspace, key: &str, stage: Stage) -> Result<PathBuf> {
    let root = ws.root().to_path_buf();
    if let Some(tree) = worktree(ws.dir(), key)
        && matches!(stage, Stage::Build | Stage::Review)
    {
        return Ok(tree);
    }
    let in_git = git(&root, &["rev-parse", "--is-inside-work-tree"]).is_ok();
    if stage != Stage::Build || !wants_worktree(ws, key) || !in_git {
        return Ok(root);
    }
    ensure_dir(ws.dir())?;
    let path = worktrees_dir(ws.dir()).join(key);
    let b = branch(key);
    let exists = git(&root, &["rev-parse", "--verify", "--quiet", &b]).is_ok();
    let path_s = path.to_string_lossy().into_owned();
    if exists {
        git(&root, &["worktree", "add", &path_s, &b])?;
    } else {
        git(&root, &["worktree", "add", "-b", &b, &path_s, "HEAD"])?;
    }
    Ok(path)
}

/// A done task's build branch, brought into the checkout's branch: fast-forward only, its
/// worktree and branch then removed, and said on the task; or why not, once for each head the
/// branch reaches. Returns whether anything was said.
fn merge_done(ws: &mut Workspace) -> Result<bool> {
    let Ok(entries) = fs::read_dir(worktrees_dir(ws.dir())) else {
        return Ok(false);
    };
    let root = ws.root().to_path_buf();
    let mut said = false;
    for e in entries.flatten() {
        let key = e.file_name().to_string_lossy().into_owned();
        if !e.path().join(".git").exists() {
            continue;
        }
        let Ok(n) = ws.node(&key) else { continue };
        if n.state != State::Done {
            continue;
        }
        let b = branch(&n.key);
        let head = git(&root, &["rev-parse", &b]).unwrap_or_default();
        let tried = e.path().with_extension("merge");
        if fs::read_to_string(&tried).is_ok_and(|t| t == head) {
            continue;
        }
        let path_s = e.path().to_string_lossy().into_owned();
        // Said once for each head the branch reaches, however it went.
        fs::write(&tried, &head)?;
        let kept = format!(
            "Its worktree is kept at {path_s}; bring it in by hand, and livtk tries again when \
             the branch moves."
        );
        let left = git(&e.path(), &["status", "--porcelain"]).unwrap_or_default();
        let text = if !left.is_empty() {
            let files: Vec<&str> = left
                .lines()
                .take(5)
                .map(|l| l.get(3..).unwrap_or(l))
                .collect();
            format!(
                "Not merged: the build left work uncommitted in {b}'s worktree ({}). {kept}",
                files.join(", ")
            )
        } else {
            match git(&root, &["merge", "--ff-only", &b]) {
                Err(why) => format!("Could not merge {b} (fast-forward only): {why}. {kept}"),
                Ok(_) => {
                    let gone = git(&root, &["worktree", "remove", &path_s])
                        .and_then(|_| git(&root, &["branch", "-d", &b]));
                    match gone {
                        Ok(_) => {
                            let _ = fs::remove_file(&tried);
                            format!("Merged {b} into the checkout's branch (fast-forward).")
                        }
                        Err(why) => format!(
                            "Merged {b} into the checkout's branch (fast-forward), but its \
                             worktree or branch could not be removed: {why}."
                        ),
                    }
                }
            }
        };
        ws.execute(
            Command::Note {
                node: n.key.clone(),
                text,
            },
            &Actor::Claude {
                agent: Some("runner".into()),
            },
        )?;
        said = true;
    }
    Ok(said)
}

// ---- runs starting on their own ---------------------------------------------------------------

/// The tasks whose build waits its turn, first first: acknowledged, given to an agent, free of
/// unfinished work, and never built since. Empty unless the project starts runs on its own.
pub fn queue(ws: &Workspace) -> Result<Vec<crate::Summary>> {
    if !ws.config().runs.auto {
        return Ok(Vec::new());
    }
    let runs = ws.runs()?;
    let mut out: Vec<crate::Summary> = ws
        .tasks(&[State::Acked])?
        .into_iter()
        .filter(|t| !t.blocked)
        .filter(|t| {
            !runs
                .iter()
                .any(|r| r.node == t.id && r.stage == Stage::Build)
        })
        .filter(|t| {
            ws.node(&t.key)
                .is_ok_and(|n| n.props.get("agent").and_then(Value::as_str).is_some())
        })
        .collect();
    out.sort_by_key(|t| (t.priority, t.id));
    Ok(out)
}

/// When the project says so: start the builds that wait their turn, as many as it builds at
/// once, and the review of each task that came into review since its last review began.
fn start_on_their_own(ws: &mut Workspace, runner: &Runner) -> Result<bool> {
    if !ws.config().runs.auto {
        return Ok(false);
    }
    let files = crate::claude::home()
        .map(|home| crate::agents::agents(ws.root(), &home))
        .unwrap_or_default();
    let human = Actor::Claude {
        agent: Some("runner".into()),
    };
    let mut started = false;
    let runs = ws.runs()?;
    let going = runs
        .iter()
        .filter(|r| r.stage == Stage::Build && r.ended().is_none())
        .count();
    let room = usize::from(ws.config().runs.builds_at_once).saturating_sub(going);
    for t in queue(ws)?.into_iter().take(room) {
        if runner
            .start_held(ws, &t.key, Some(Stage::Build), None, &files, &human)
            .is_ok()
        {
            started = true;
        }
    }
    for t in ws.tasks(&[State::Review])? {
        let since = ws.entered(&t.key)?.unwrap_or_default();
        let reviewed = runs
            .iter()
            .any(|r| r.node == t.id && r.stage == Stage::Review && r.started >= since);
        let going = runs.iter().any(|r| r.node == t.id && r.ended().is_none());
        if !reviewed
            && !going
            && runner
                .start_held(ws, &t.key, Some(Stage::Review), None, &files, &human)
                .is_ok()
        {
            started = true;
        }
    }
    Ok(started)
}

fn hold_path(livtk: &Path) -> PathBuf {
    runs_dir(livtk).join("settle.lock")
}

/// How long a change to a run waits for a settle going to finish.
const HOLD_WAIT: Duration = Duration::from_secs(5);

/// A lock file held while it lives; taken from a holder that has held it too long. Settling
/// and changing a run (stopping, continuing) hold it, so neither writes over the other.
struct Hold(PathBuf);

impl Hold {
    /// Take it, waiting a while for a holder to let go.
    fn wait(livtk: &Path) -> Result<Hold> {
        ensure_dir(livtk)?;
        let start = std::time::Instant::now();
        loop {
            if let Some(h) = Hold::take(&hold_path(livtk)) {
                return Ok(h);
            }
            if start.elapsed() > HOLD_WAIT {
                return Err(Error::Refused("the runs are busy; try again".into()));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn take(path: &Path) -> Option<Hold> {
        let stale = fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > SETTLE_HOLD);
        if stale {
            let _ = fs::remove_file(path);
        }
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .ok()
            .map(|_| Hold(path.to_path_buf()))
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

// ---- sessions livtk did not start -----------------------------------------------------------

/// How long a session's "writing" mark from its hooks counts without a Stop clearing it.
const MARK_FOR: Duration = Duration::from_secs(60 * 60);

fn marks_dir(livtk: &Path) -> PathBuf {
    runs_dir(livtk).join("writing")
}

/// Mark session `session` writing (a turn began) or not (it ended): for sessions livtk did not
/// start, from their UserPromptSubmit and Stop hooks. Beside the log, never in it.
pub fn mark_writing(livtk: &Path, session: &str, on: bool) -> Result<()> {
    if session.is_empty() || session.contains(['/', '\\']) {
        return Ok(());
    }
    let path = marks_dir(livtk).join(session);
    if on {
        ensure_dir(livtk)?;
        fs::create_dir_all(marks_dir(livtk))?;
        fs::write(path, b"")?;
    } else {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

/// The sessions writing now: livtk's runs with a turn going, and the others their hooks marked.
pub fn writing_sessions(livtk: &Path) -> Vec<String> {
    let mut out: Vec<String> = lives(livtk)
        .into_iter()
        .filter(writing)
        .map(|l| l.session)
        .collect();
    if let Ok(entries) = fs::read_dir(marks_dir(livtk)) {
        for e in entries.flatten() {
            let fresh = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age < MARK_FOR);
            let name = e.file_name().to_string_lossy().into_owned();
            if fresh && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}
