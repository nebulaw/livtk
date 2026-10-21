//! `livtk run`, `livtk runs` and `livtk agent`: Claude run on a task headless, as livtk drives
//! it, and the agent a task is given to.

use anyhow::{Context, Result};
use clap::Subcommand;
use livtk_core::event::{Ending, RunMark};
use livtk_core::format::session as short;
use livtk_core::run::{self, Run, Runner, Stage};
use livtk_core::{Actor, Command, Workspace};
use serde_json::json;

use crate::output::print_json;

#[derive(Subcommand)]
pub enum RunCmd {
    /// Stop KEY's run: its turn ends cleanly and the work so far is kept.
    Stop { key: String },
    /// Let KEY's run do what it asked to: the commands it was refused. Human only.
    Allow { key: String },
    /// Refuse what KEY's run asked to do; it goes on without. Human only.
    Deny { key: String },
    /// Continue KEY's stopped run where it left off.
    Continue { key: String },
}

#[derive(Subcommand)]
pub enum AgentCmd {
    /// Give KEY to an agent (as `--agent` names it: livtk:implementer, code-reviewer).
    Set { key: String, name: String },
    /// Take KEY back from its agent: runs use the suggested one.
    Clear { key: String },
    /// KEY's agent, and the one each stage would run as.
    Show { key: String },
}

fn runner() -> Result<Runner> {
    Runner::find().context("the claude command is not installed, or not where livtk looks")
}

fn agent_files(ws: &Workspace) -> Vec<livtk_core::agents::AgentFile> {
    livtk_core::claude::home()
        .map(|home| livtk_core::agents::agents(ws.root(), &home))
        .unwrap_or_default()
}

/// Bring the log up to what the runs did, quietly: a run that cannot be settled now is
/// settled the next time.
pub fn settle(ws: &mut Workspace) {
    if let Some(r) = Runner::find() {
        let _ = run::settle(ws, &r);
    }
}

pub fn start(
    ws: &mut Workspace,
    key: &str,
    stage: Option<&str>,
    agent: Option<String>,
    by: &Actor,
    json: bool,
) -> Result<()> {
    let stage = stage.map(str::parse::<Stage>).transpose()?;
    let files = agent_files(ws);
    let live = runner()?.start(ws, key, stage, agent, &files, by)?;
    if json {
        return print_json(&live);
    }
    let as_agent = live
        .agent
        .as_deref()
        .map(|a| format!(" as {a}"))
        .unwrap_or_default();
    println!(
        "{}: started to {}{as_agent} (session {}); `livtk runs` follows it",
        live.key,
        live.stage.as_str(),
        short(&live.session)
    );
    Ok(())
}

pub fn run(ws: &mut Workspace, cmd: RunCmd, by: &Actor, json: bool) -> Result<()> {
    let r = runner()?;
    let (key, said) = match cmd {
        RunCmd::Stop { key } => {
            r.stop(ws, &key, Ending::Stopped, by)?;
            (key, "stopping; the work so far is kept")
        }
        RunCmd::Allow { key } => {
            r.answer(ws, &key, true, by)?;
            (key, "allowed; it goes on")
        }
        RunCmd::Deny { key } => {
            r.answer(ws, &key, false, by)?;
            (key, "denied; it goes on without")
        }
        RunCmd::Continue { key } => {
            r.resume_stopped(ws, &key, by)?;
            (key, "continuing where it left off")
        }
    };
    let key = ws.node(&key)?.key;
    if json {
        return print_json(&json!({ "key": key, "done": said }));
    }
    println!("{key}: {said}");
    Ok(())
}

/// Where a run stands, as people say it.
fn standing(r: &Run, writing: &[String]) -> String {
    if writing.contains(&r.session) {
        return "writing".into();
    }
    match (&r.last, &r.answer) {
        (RunMark::Asks { allow }, None) if allow.is_empty() => "needs you: a question".into(),
        (RunMark::Asks { allow }, None) => format!("needs you: to run {}", allow.join(", ")),
        (RunMark::Asks { .. }, Some(_)) => "answered".into(),
        (RunMark::Ended { how }, _) => match how {
            Ending::Done => "done".into(),
            Ending::Failed => "failed".into(),
            Ending::Stopped => "stopped".into(),
            Ending::Quit => "stopped by a quit".into(),
        },
        _ => "starting".into(),
    }
}

pub fn list(ws: &mut Workspace, all: bool, json: bool) -> Result<()> {
    settle(ws);
    let writing = run::writing_sessions(ws.dir());
    let runs: Vec<Run> = ws
        .runs()?
        .into_iter()
        .filter(|r| all || r.ended().is_none())
        .collect();
    if json {
        let rows: Vec<_> = runs
            .iter()
            .map(|r| {
                json!({
                    "key": r.key,
                    "session": r.session,
                    "agent": r.agent,
                    "stage": r.stage,
                    "started": r.started,
                    "standing": standing(r, &writing),
                    "text": r.text,
                })
            })
            .collect();
        return print_json(&rows);
    }
    if runs.is_empty() {
        println!(
            "{}",
            if all {
                "No run has been started here."
            } else {
                "No run is going; `livtk runs --all` lists the ended ones."
            }
        );
        return Ok(());
    }
    for r in &runs {
        println!(
            "{:<8}  {:<8}  {:<18}  {}  {}  {}",
            r.key,
            r.stage.as_str(),
            r.agent.as_deref().unwrap_or("(session)"),
            short(&r.session),
            r.started,
            standing(r, &writing)
        );
    }
    Ok(())
}

pub fn agent(ws: &mut Workspace, cmd: AgentCmd, by: &Actor, json: bool) -> Result<()> {
    let (key, name) = match cmd {
        AgentCmd::Set { key, name } => (key, name),
        AgentCmd::Clear { key } => (key, String::new()),
        AgentCmd::Show { key } => {
            let n = ws.node(&key)?;
            let own = n
                .props
                .get("agent")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let files = agent_files(ws);
            let stages: Vec<(Stage, Option<String>)> = Stage::ALL
                .into_iter()
                .map(|s| (s, own.clone().or_else(|| run::suggest(s, &files))))
                .collect();
            if json {
                return print_json(&json!({
                    "key": n.key,
                    "agent": own,
                    "runs_as": stages.iter().map(|(s, a)| json!({"stage": s, "agent": a})).collect::<Vec<_>>(),
                }));
            }
            match &own {
                Some(a) => println!("{}: given to {a}", n.key),
                None => println!(
                    "{}: no agent of its own; each stage runs as suggested",
                    n.key
                ),
            }
            for (s, a) in stages {
                println!(
                    "  {:<8}  {}",
                    s.as_str(),
                    a.as_deref().unwrap_or("(the session itself)")
                );
            }
            return Ok(());
        }
    };
    let events = ws.execute(
        Command::SetAgent {
            node: key.clone(),
            agent: name.clone(),
        },
        by,
    )?;
    let key = ws.node(&key)?.key;
    if json {
        return print_json(&events);
    }
    match (events.is_empty(), name.is_empty()) {
        (true, _) => println!("{key}: unchanged"),
        (false, true) => println!("{key}: taken back from its agent"),
        (false, false) => println!("{key}: given to {name}"),
    }
    Ok(())
}
