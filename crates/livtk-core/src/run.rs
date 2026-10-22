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

    /// The tools a run of the stage uses without asking: reading, and livtk's own commands; a
    /// build also edits files.
    pub fn allowed(self) -> Vec<String> {
        let mut out: Vec<String> = ["Read", "Grep", "Glob", "Bash(livtk *)"]
            .map(str::to_owned)
            .to_vec();
        if self == Stage::Build {
            out.extend(["Edit", "Write", "NotebookEdit", "TodoWrite"].map(str::to_owned));
        }
        out
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
}

/// The folder runs keep their state in, inside the project's `.livtk/`: never committed.
pub fn runs_dir(livtk: &Path) -> PathBuf {
    livtk.join("runs")
}

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
/// can be reused, and a finished child left unreaped no longer names its session).
pub fn running(pid: u32, session: &str) -> bool {
    Process::new("ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(session))
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
        let agent = agent
            .or_else(|| {
                n.props
                    .get("agent")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| suggest(stage, files));
        let live = Live {
            session: Uuid::now_v7().to_string(),
            key: n.key.clone(),
            agent: agent.clone(),
            stage,
            cwd: ws.root().to_path_buf(),
            allowed: stage.allowed(),
            read: 0,
            pid: None,
            stopping: None,
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
        match self.turn(ws.dir(), live, prompt, resume) {
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
    fn turn(&self, livtk: &Path, live: &mut Live, prompt: &str, resume: bool) -> Result<()> {
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
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd
            .current_dir(&live.cwd)
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
        if let Some(pid) = live.pid.filter(|&p| running(p, &live.session)) {
            live.stopping = Some(how);
            save(&livtk, &live)?;
            interrupt(pid);
            return Ok(());
        }
        end(ws, &run, &mut live, how, stopped_text(how), by)
    }

    /// Answer task `key`'s waiting run: let it run what it asked for, or not. A note of yours
    /// answers a question the same way, on its own.
    pub fn answer(&self, ws: &mut Workspace, key: &str, allow: bool, by: &Actor) -> Result<()> {
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
        let (mark, text) = if allow {
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
                "The human did not allow: {}. Go on without it, or say what you need.",
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
        let livtk = ws.dir().to_path_buf();
        let mut live = live(&livtk, &run.session).unwrap_or_else(|| gone(&run, ws.root()));
        live.stopping = None;
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
        allowed: run.stage.allowed(),
        read: 0,
        pid: None,
        stopping: None,
    }
}

const fn stopped_text(how: Ending) -> &'static str {
    match how {
        Ending::Quit => "Stopped when the app quit; the work so far is kept.",
        _ => "Stopped; the work so far is kept.",
    }
}

/// SIGINT to the turn's process group: Claude ends the turn cleanly.
fn interrupt(pid: u32) {
    let _ = Process::new("kill")
        .args(["-INT", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status();
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
pub fn simple_commands(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
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
            (None, '&' | '|') if chars.peek() == Some(&c) => {
                chars.next();
                out.push(std::mem::take(&mut cur));
            }
            (None, ';' | '|' | '\n') => out.push(std::mem::take(&mut cur)),
            (None, c) => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter()
        .map(|c| c.trim().to_owned())
        .filter(|c| !c.is_empty())
        .collect()
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
    if !runs_dir(&livtk).exists() {
        return Ok(false);
    }
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
        if run.ended().is_some() || writing(&live) {
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
            let text = format!(
                "{}\n\nAsks to run: {}",
                said(&turn.text),
                turn.denied.join(", ")
            );
            note(
                ws,
                &run.key,
                &live,
                RunMark::Asks {
                    allow: turn.denied.clone(),
                },
                &text,
                &claude(),
            )?;
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
