//! `livtk sessions`: the Claude Code sessions that worked in this project, and the claim that
//! credits work done before livtk recorded sessions to the session it came from.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use jiff::Timestamp;
use livtk_core::format::session as short;
use livtk_core::{Actor, Command, Workspace};
use serde_json::{Value, json};
use uuid::Uuid;

/// How long after a `livtk` call its event may be written and still be the call's own.
const MATCH_SECS: i64 = 300;

#[derive(Subcommand)]
pub enum SessionsCmd {
    /// Credit Claude's earlier events, written before livtk recorded sessions, to the session
    /// each came from: the one whose transcript ran the `livtk` command that wrote it, else the
    /// session given with --session.
    Claim {
        /// The session for what no transcript places (a Claude Code session id).
        #[arg(long = "default", value_name = "SESSION")]
        default: Option<String>,
        /// Where Claude Code keeps this project's transcripts; found from the project's folder
        /// when not given.
        #[arg(long)]
        transcripts: Option<PathBuf>,
        /// Say what would be claimed, and claim nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

/// The sessions, or the sessions of one node.
pub fn list(ws: &Workspace, key: Option<&str>, json: bool) -> Result<()> {
    if let Some(key) = key {
        let uses = ws.sessions_of(key)?;
        if json {
            return crate::output::print_json(&uses);
        }
        if uses.is_empty() {
            println!("No Claude session is on record for {key}.");
        }
        for u in &uses {
            println!(
                "{}  {:<16} {} event(s), {} .. {}",
                short(&u.session),
                u.by,
                u.events,
                u.first.get(..16).unwrap_or(&u.first),
                u.last.get(..16).unwrap_or(&u.last)
            );
        }
        return Ok(());
    }
    let all = ws.sessions()?;
    if json {
        let rows: Vec<Value> = all
            .iter()
            .map(|s| {
                let nodes: Vec<String> = ws
                    .session_nodes(&s.session)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|b| b.key)
                    .collect();
                json!({"session": s, "nodes": nodes})
            })
            .collect();
        return crate::output::print_json(&rows);
    }
    if all.is_empty() {
        println!("No Claude session is on record yet.");
    }
    for s in &all {
        println!(
            "{}  {} .. {}  {} node(s)  {}",
            short(&s.session),
            s.first.get(..16).unwrap_or(&s.first),
            s.last.get(..16).unwrap_or(&s.last),
            s.nodes,
            s.by.join(", ")
        );
    }
    Ok(())
}

pub fn run(ws: &mut Workspace, cmd: SessionsCmd, by: &Actor, json: bool) -> Result<()> {
    let SessionsCmd::Claim {
        default,
        transcripts,
        dry_run,
    } = cmd;
    let dir = match transcripts {
        Some(d) => d,
        None => transcripts_dir(ws.root())
            .context("no home folder to find Claude Code's transcripts in")?,
    };
    let calls = livtk_calls(&dir);
    // Each unclaimed event to its session: the latest `livtk` call before it, close enough.
    let mut claims: BTreeMap<(Uuid, String), Vec<Uuid>> = BTreeMap::new();
    let (mut matched, mut defaulted, mut unplaced) = (0usize, 0usize, 0usize);
    for u in ws.unclaimed()? {
        let Ok(at) = u.at.parse::<Timestamp>() else {
            continue;
        };
        let call = calls
            .iter()
            .rev()
            .find(|(t, _)| *t <= at && at.as_second() - t.as_second() <= MATCH_SECS);
        let session = match (call, &default) {
            (Some((_, s)), _) => {
                matched += 1;
                s.clone()
            }
            (None, Some(d)) => {
                defaulted += 1;
                d.clone()
            }
            (None, None) => {
                unplaced += 1;
                continue;
            }
        };
        claims.entry((u.node, session)).or_default().push(u.id);
    }
    let mut per_session: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for ((node, session), events) in &claims {
        let e = per_session.entry(session.clone()).or_default();
        e.0 += events.len();
        e.1 += 1;
        if !dry_run {
            ws.execute(
                Command::ClaimSession {
                    node: node.to_string(),
                    session: session.clone(),
                    events: events.clone(),
                },
                by,
            )?;
        }
    }
    if json {
        return crate::output::print_json(&json!({
            "transcripts": dir, "calls": calls.len(), "matched": matched,
            "defaulted": defaulted, "unplaced": unplaced, "dry_run": dry_run,
            "sessions": per_session.iter().map(|(s, (e, n))| json!({"session": s, "events": e, "nodes": n})).collect::<Vec<_>>(),
        }));
    }
    println!(
        "{} {} livtk call(s) in {}: {matched} event(s) matched to a transcript, {defaulted} to the default session{}",
        if dry_run {
            "Would claim from"
        } else {
            "Claimed from"
        },
        calls.len(),
        dir.display(),
        if unplaced > 0 {
            format!(", {unplaced} left unplaced (give --default)")
        } else {
            String::new()
        }
    );
    for (s, (events, nodes)) in &per_session {
        println!("  {}  {events} event(s) on {nodes} node(s)", short(s));
    }
    Ok(())
}

/// Where Claude Code keeps the transcripts of sessions run in `root`: its projects folder, under
/// the folder's path with `/` and `.` as `-`.
fn transcripts_dir(root: &Path) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let name: String = root
        .display()
        .to_string()
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect();
    Some(PathBuf::from(home).join(".claude/projects").join(name))
}

/// Every Bash call that ran `livtk`, in every transcript under `dir` (subagents' too, which name
/// their parent session), by time.
fn livtk_calls(dir: &Path) -> Vec<(Timestamp, String)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "jsonl") {
                read_calls(&path, &mut out);
            }
        }
    }
    out.sort();
    out
}

fn read_calls(path: &Path, out: &mut Vec<(Timestamp, String)>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        // Cheap first: most lines are not a livtk call.
        if !line.contains("livtk") || !line.contains("tool_use") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let (Some(at), Some(session)) = (
            v["timestamp"]
                .as_str()
                .and_then(|t| t.parse::<Timestamp>().ok()),
            v["sessionId"].as_str(),
        ) else {
            continue;
        };
        let runs_livtk = v["message"]["content"].as_array().is_some_and(|c| {
            c.iter().any(|b| {
                b["type"] == "tool_use"
                    && b["name"] == "Bash"
                    && b["input"]["command"]
                        .as_str()
                        .is_some_and(|cmd| !crate::guard::invocations(cmd).is_empty())
            })
        });
        if runs_livtk {
            out.push((at, session.to_owned()));
        }
    }
}
