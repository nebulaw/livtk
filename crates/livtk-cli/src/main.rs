//! `livtk`: the command line over livtk-core. Thin by design: parse, call the workspace, print.

mod embed;
mod guard;
mod hook;
mod output;

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use livtk_core::{Actor, Command, Error, Kind, Rel, Scope, State, Workspace};

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
    /// Propose a lesson: a rule learned from experience. It binds once the human ratifies it.
    Lesson {
        /// The rule, in one sentence.
        rule: String,
        /// Why: what happened, the evidence. `-` reads it from stdin.
        #[arg(long)]
        why: Option<String>,
        /// What it was learned from (a task, a finding). Repeatable.
        #[arg(long, value_name = "KEY")]
        from: Vec<String>,
        /// The lesson this one replaces once ratified.
        #[arg(long, value_name = "KEY")]
        supersedes: Option<String>,
    },
    /// Propose a decision: a choice and its reasons. It binds once the human accepts it.
    Decision {
        title: String,
        /// The context, the options and the reasons. `-` reads it from stdin.
        #[arg(long)]
        body: Option<String>,
        #[arg(long, value_name = "KEY")]
        from: Vec<String>,
        /// The decision this one replaces once accepted.
        #[arg(long, value_name = "KEY")]
        supersedes: Option<String>,
    },
    /// Record a finding: something found out, kept for later.
    Finding {
        title: String,
        /// What was found, with sources. `-` reads it from stdin.
        #[arg(long)]
        body: Option<String>,
        #[arg(long, value_name = "KEY")]
        from: Vec<String>,
    },
    /// Ratify a proposed lesson, making it a rule. Human only.
    Ratify {
        key: String,
        /// A standing rule: shown at every session start, not only when relevant.
        #[arg(long)]
        always: bool,
    },
    /// Reject a proposed lesson or decision. Human only.
    Reject { key: String },
    /// Retire a ratified lesson. Human only.
    Retire { key: String },
    /// Accept a proposed decision. Human only.
    Accept { key: String },
    /// Everything waiting for the human: plans, lessons, decisions.
    Inbox,
    /// List nodes of a kind grouped by state; the active ones unless told otherwise.
    #[command(visible_alias = "ls", visible_alias = "board")]
    List {
        /// task (default), lesson, decision or finding.
        #[arg(long, short, default_value = "task")]
        kind: Kind,
        /// Only these states. Repeatable.
        #[arg(long, short)]
        state: Vec<State>,
        /// Include what is over: done, canceled, rejected, retired, superseded.
        #[arg(long, short)]
        all: bool,
    },
    /// Show a node with its links, notes and history.
    Show { key: String },
    /// Search every node, by keywords and by meaning.
    Search {
        query: String,
        #[arg(long, short = 'n', default_value_t = 10)]
        limit: usize,
    },
    /// What memory would be recalled for this text: rules, decisions and findings that pass the
    /// relevance bar.
    Recall { query: String },
    /// The local embedding model that ranks search by meaning.
    Embed {
        #[command(subcommand)]
        action: EmbedAction,
    },
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
enum EmbedAction {
    /// Download the model (once) with the system curl.
    Setup {
        /// potion-retrieval-32M (default, 128 MB) or potion-base-8M (30 MB).
        #[arg(long)]
        model: Option<String>,
    },
    /// Whether the model is installed and loads.
    Status,
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
        // So the app lists it. A failure here costs a convenience, never the project.
        if let Some(config) = livtk_core::registry::config_dir()
            && let Err(e) = livtk_core::registry::add(&config, &cwd)
        {
            eprintln!("livtk: warning: could not add the project to the app's list: {e}");
        }
        return output::initialised(&ws, cli.json);
    }
    if let Cmd::Embed { action } = &cli.command {
        return embed::run(action, &cwd, cli.json);
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
        Cmd::Init { .. } | Cmd::Hook { .. } | Cmd::Embed { .. } => {
            unreachable!("handled before opening a workspace")
        }
        Cmd::New {
            title,
            body,
            priority,
            after,
        } => {
            let links = after.into_iter().map(|k| (Rel::DependsOn, k)).collect();
            let body = read_body(body)?.unwrap_or_default();
            create(ws, Kind::Task, title, body, priority, links, by, json)
        }
        Cmd::Lesson {
            rule,
            why,
            from,
            supersedes,
        } => {
            let links = memory_links(from, supersedes);
            let body = read_body(why)?.unwrap_or_default();
            create(ws, Kind::Lesson, rule, body, None, links, by, json)
        }
        Cmd::Decision {
            title,
            body,
            from,
            supersedes,
        } => {
            let links = memory_links(from, supersedes);
            let body = read_body(body)?.unwrap_or_default();
            create(ws, Kind::Decision, title, body, None, links, by, json)
        }
        Cmd::Finding { title, body, from } => {
            let links = memory_links(from, None);
            let body = read_body(body)?.unwrap_or_default();
            create(ws, Kind::Finding, title, body, None, links, by, json)
        }
        Cmd::Ratify { key, always } => {
            let before = ws.node(&key)?;
            let events = ws.execute(
                Command::Ratify {
                    node: key.clone(),
                    always,
                },
                by,
            )?;
            output::moved(ws, &before, &events, json)
        }
        Cmd::Reject { key } => moved(ws, &key, State::Rejected, by, json),
        Cmd::Retire { key } => moved(ws, &key, State::Retired, by, json),
        Cmd::Accept { key } => moved(ws, &key, State::Accepted, by, json),
        Cmd::Inbox => output::inbox(&ws.inbox()?, json),
        Cmd::List { kind, state, all } => {
            let states: Vec<State> = if !state.is_empty() {
                state
            } else if all {
                Vec::new()
            } else {
                kind.states()
                    .iter()
                    .copied()
                    .filter(|s| s.is_active())
                    .collect()
            };
            output::list(&ws.list(kind, &states)?, json)
        }
        Cmd::Search { query, limit } => output::hits(&ws.search(&query, Scope::All, limit)?, json),
        Cmd::Recall { query } => output::hits(&ws.recall(&query)?, json),
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
    output::moved(ws, &before, &events, json)
}

#[allow(clippy::too_many_arguments)] // one call site per kind; a struct would only rename them
fn create(
    ws: &mut Workspace,
    kind: Kind,
    title: String,
    body: String,
    priority: Option<u8>,
    links: Vec<(Rel, String)>,
    by: &Actor,
    json: bool,
) -> Result<()> {
    let events = ws.execute(
        Command::Create {
            kind,
            title,
            body,
            priority,
            links,
        },
        by,
    )?;
    output::created(ws, &events, json)
}

fn memory_links(from: Vec<String>, supersedes: Option<String>) -> Vec<(Rel, String)> {
    from.into_iter()
        .map(|k| (Rel::LearnedFrom, k))
        .chain(supersedes.map(|k| (Rel::Supersedes, k)))
        .collect()
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
