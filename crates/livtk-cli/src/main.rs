//! `livtk`: the command line over livtk-core. Thin by design: parse, call the workspace, print.

mod guard;
mod hook;
mod output;

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use livtk_core::{Actor, Command, Error, Rel, State, Workspace};

#[derive(Parser)]
#[command(
    name = "livtk",
    version,
    about = "Durable tasks for Claude Code and its human"
)]
struct Cli {
    /// Print machine-readable JSON instead of text.
    #[arg(long, global = true)]
    json: bool,

    /// Run as if started in this directory.
    #[arg(short = 'C', long, global = true, value_name = "DIR")]
    dir: Option<PathBuf>,

    /// The Claude Code subagent running this command. The plugin adds it; people never need it.
    #[arg(long, global = true, hide = true, value_name = "NAME")]
    agent: Option<String>,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create .livtk/ in the current directory.
    Init {
        /// Key prefix, as in LIV-12. Defaults to the directory's first three letters.
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Create a task in the backlog.
    New {
        title: String,
        /// Markdown body; `-` reads it from stdin.
        #[arg(long)]
        body: Option<String>,
        /// 1 urgent, 2 high, 3 medium (default), 4 low.
        #[arg(short, long)]
        priority: Option<u8>,
        /// A task this one waits for. Repeatable.
        #[arg(long, value_name = "KEY")]
        after: Vec<String>,
    },
    /// List tasks grouped by state; open ones unless told otherwise.
    #[command(visible_alias = "ls", visible_alias = "board")]
    List {
        /// Only these states. Repeatable.
        #[arg(long, short)]
        state: Vec<State>,
        /// Include done and canceled tasks.
        #[arg(long, short)]
        all: bool,
    },
    /// Show a task with its links and history.
    Show { key: String },
    /// Change a task's title, body or priority. An acknowledged task returns to planned.
    Edit {
        key: String,
        #[arg(long)]
        title: Option<String>,
        /// Markdown body; `-` reads it from stdin.
        #[arg(long)]
        body: Option<String>,
        #[arg(short, long)]
        priority: Option<u8>,
    },
    /// Move a task to another state.
    Move { key: String, state: State },
    /// Acknowledge a planned task. Human only.
    Ack { key: String },
    /// Start an acknowledged task.
    Start { key: String },
    /// Mark a task in review as done.
    Done { key: String },
    /// Add or remove a dependency.
    Dep {
        #[command(subcommand)]
        action: DepAction,
    },
    /// Add a note to a task's history: what was tried, what failed, what was learned.
    Note {
        key: String,
        /// The note; `-` reads it from stdin.
        text: String,
    },
    /// The best task to start now.
    Next,
    /// Event history of a task, or of the whole project.
    Log { key: Option<String> },
    /// Rebuild the database from the event log.
    Rebuild,
    /// Answer a Claude Code hook: reads its JSON on stdin. Used by the livtk plugin.
    #[command(hide = true)]
    Hook { event: String },
}

#[derive(Subcommand)]
enum DepAction {
    /// KEY waits for the task given with --on.
    Add(DepArgs),
    /// KEY no longer waits for the task given with --on.
    Rm(DepArgs),
}

#[derive(Args)]
struct DepArgs {
    key: String,
    #[arg(long, value_name = "KEY")]
    on: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Cmd::Hook { event } = &cli.command {
        return hook::run(event);
    }
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("livtk: {e:#}");
            match e.downcast_ref::<Error>() {
                Some(Error::Refused(_)) => ExitCode::from(3),
                _ => ExitCode::FAILURE,
            }
        }
    }
}

/// Claude Code sets CLAUDECODE=1 in every shell it runs; its plugin hook names subagents with
/// `--agent`.
fn actor(agent: Option<String>) -> Result<Actor> {
    let claude = std::env::var_os("CLAUDECODE").is_some_and(|v| v == "1");
    match (claude, agent) {
        (true, agent) => Ok(Actor::Claude {
            agent: agent.filter(|a| !a.is_empty()),
        }),
        (false, None) => Ok(Actor::Human),
        (false, Some(_)) => {
            anyhow::bail!("--agent names a Claude Code subagent; commands you run need no label")
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let cwd = match &cli.dir {
        Some(d) => d.clone(),
        None => std::env::current_dir().context("cannot read the current directory")?,
    };
    if let Cmd::Init { prefix } = &cli.command {
        let ws = Workspace::init(&cwd, prefix.as_deref())?;
        return output::initialised(&ws, cli.json);
    }
    let by = actor(cli.agent)?;
    let mut ws = Workspace::open(&cwd)?;
    let result = dispatch(&mut ws, cli.command, &by, cli.json);
    for w in ws.take_warnings() {
        eprintln!("livtk: warning: {w}");
    }
    result
}

fn dispatch(ws: &mut Workspace, cmd: Cmd, by: &Actor, json: bool) -> Result<()> {
    match cmd {
        Cmd::Init { .. } | Cmd::Hook { .. } => unreachable!("handled before opening a workspace"),
        Cmd::New {
            title,
            body,
            priority,
            after,
        } => {
            let events = ws.execute(
                Command::CreateTask {
                    title,
                    body: read_body(body)?.unwrap_or_default(),
                    priority,
                    depends_on: after,
                },
                by,
            )?;
            output::created(ws, &events, json)
        }
        Cmd::List { state, all } => {
            let states: Vec<State> = if !state.is_empty() {
                state
            } else if all {
                Vec::new()
            } else {
                State::ALL.into_iter().filter(|s| !s.is_closed()).collect()
            };
            output::list(&ws.tasks(&states)?, json)
        }
        Cmd::Show { key } => output::show(ws, &key, json),
        Cmd::Edit {
            key,
            title,
            body,
            priority,
        } => {
            let before = ws.node(&key)?;
            let mut events = Vec::new();
            let body = read_body(body)?;
            if title.is_some() || body.is_some() {
                events.extend(ws.execute(
                    Command::Edit {
                        node: key.clone(),
                        title,
                        body,
                    },
                    by,
                )?);
            }
            if let Some(priority) = priority {
                events.extend(ws.execute(
                    Command::SetPriority {
                        node: key.clone(),
                        priority,
                    },
                    by,
                )?);
            }
            output::edited(ws, &before, &events, json)
        }
        Cmd::Move { key, state } => moved(ws, &key, state, by, json),
        Cmd::Ack { key } => moved(ws, &key, State::Acked, by, json),
        Cmd::Start { key } => moved(ws, &key, State::Doing, by, json),
        Cmd::Done { key } => moved(ws, &key, State::Done, by, json),
        Cmd::Dep { action } => {
            let (args, add) = match action {
                DepAction::Add(a) => (a, true),
                DepAction::Rm(a) => (a, false),
            };
            let (src, dst) = (args.key, args.on);
            let cmd = if add {
                Command::Link {
                    src: src.clone(),
                    rel: Rel::DependsOn,
                    dst: dst.clone(),
                }
            } else {
                Command::Unlink {
                    src: src.clone(),
                    rel: Rel::DependsOn,
                    dst: dst.clone(),
                }
            };
            let events = ws.execute(cmd, by)?;
            output::linked(ws, &src, &dst, add, &events, json)
        }
        Cmd::Note { key, text } => {
            let text = read_body(Some(text))?.unwrap_or_default();
            let events = ws.execute(
                Command::Note {
                    node: key.clone(),
                    text,
                },
                by,
            )?;
            output::noted(&ws.node(&key)?.key, &events, json)
        }
        Cmd::Next => output::next(ws.next()?.as_ref(), json),
        Cmd::Log { key } => output::log(ws, key.as_deref(), json),
        Cmd::Rebuild => {
            ws.rebuild()?;
            output::rebuilt(ws, json)
        }
    }
}

fn moved(ws: &mut Workspace, key: &str, to: State, by: &Actor, json: bool) -> Result<()> {
    let before = ws.node(key)?;
    let events = ws.execute(
        Command::Move {
            node: key.to_owned(),
            to,
        },
        by,
    )?;
    output::moved(&before, to, &events, json)
}

/// `--body -` reads the body from stdin, so long plans need no shell quoting.
fn read_body(body: Option<String>) -> Result<Option<String>> {
    match body.as_deref() {
        Some("-") => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .context("cannot read the body from stdin")?;
            Ok(Some(text.trim_end().to_owned()))
        }
        _ => Ok(body),
    }
}
