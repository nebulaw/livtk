//! `livtk project`: an outcome above tasks. Claude proposes one, the human commits to it; its
//! progress is counted from the tasks that are part of it.

use std::fmt::Write as _;

use anyhow::Result;
use clap::Subcommand;
use jiff::Timestamp;
use livtk_core::{
    ACTIVE_AT_ONCE, Actor, Command, Health, Kind, Progress, Rel, State, Summary, Workspace,
};
use serde_json::json;

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// Propose a project: an outcome, its body ending in a "Done when:" line. It is committed to
    /// once the human moves it to active.
    New {
        title: String,
        /// The outcome, ending in what "done" means. `-` reads it from stdin.
        #[arg(long)]
        body: Option<String>,
        /// When it should be reached: YYYY-MM-DD.
        #[arg(long)]
        target: Option<String>,
        /// The person who owns the outcome (never Claude).
        #[arg(long)]
        owner: Option<String>,
        /// The project this one is part of.
        #[arg(long = "part-of")]
        part_of: Option<String>,
    },
    /// The projects, active ones first, with their progress; dropped and done ones with --all.
    #[command(visible_alias = "ls")]
    List {
        #[arg(long)]
        all: bool,
    },
    /// A project: its outcome, its progress, what waits on you, its newest update.
    Show { key: String },
    /// Say how a project stands: on_track, at_risk or off_track, with what moved and what is
    /// next.
    Update {
        key: String,
        #[arg(long)]
        health: String,
        text: String,
    },
    /// Set a project's target date or owner; an empty value clears it.
    Set {
        key: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        owner: Option<String>,
    },
}

pub fn run(ws: &mut Workspace, cmd: ProjectCmd, by: &Actor, json: bool) -> Result<()> {
    let now = Timestamp::now();
    match cmd {
        ProjectCmd::New {
            title,
            body,
            target,
            owner,
            part_of,
        } => {
            let body = crate::read_body(body)?.unwrap_or_default();
            let mut events = ws.execute(
                Command::Create {
                    kind: Kind::Project,
                    title,
                    body,
                    priority: None,
                    links: part_of.map(|p| (Rel::Parent, p)).into_iter().collect(),
                },
                by,
            )?;
            if (target.is_some() || owner.is_some())
                && let Some(livtk_core::Op::NodeCreate { node, .. }) = events.first().map(|e| &e.op)
            {
                let node = node.to_string();
                events.extend(ws.execute(
                    Command::SetProject {
                        node,
                        target,
                        owner,
                    },
                    by,
                )?);
            }
            crate::output::created(ws, &events, json)
        }
        ProjectCmd::List { all } => {
            let mut rows: Vec<Summary> = ws
                .projects()?
                .into_iter()
                .filter(|p| all || p.kind.is_active(p.state))
                .collect();
            // Active first, then the rest in their states' order.
            rows.sort_by_key(|p| (p.state != State::Active, p.state.rank(), p.id));
            let progress = rows
                .iter()
                .map(|p| ws.progress(&p.key, now))
                .collect::<livtk_core::Result<Vec<_>>>()?;
            let active = rows.iter().filter(|p| p.state == State::Active).count();
            if json {
                let list: Vec<_> = rows
                    .iter()
                    .zip(&progress)
                    .map(|(p, g)| json!({"project": p, "progress": g}))
                    .collect();
                return crate::output::print_json(&list);
            }
            if rows.is_empty() {
                println!("No projects. `livtk project new \"outcome\"` proposes one.");
                return Ok(());
            }
            let width = rows.iter().map(|p| p.key.len()).max().unwrap_or(0);
            let mut text = String::new();
            for (p, g) in rows.iter().zip(&progress) {
                writeln!(
                    text,
                    "  {:width$}  {:<8}  {:>5}  {:<12}  {}",
                    p.key,
                    p.state.as_str(),
                    format!("{}/{}", g.done, g.total),
                    health_word(g),
                    p.title
                )?;
                let waits = waiting_line(g);
                if !waits.is_empty() || g.target.is_some() {
                    let target = g
                        .target
                        .as_deref()
                        .map(|t| format!("target {t}"))
                        .unwrap_or_default();
                    let sep = if waits.is_empty() || target.is_empty() {
                        ""
                    } else {
                        "; "
                    };
                    writeln!(text, "  {:width$}  {waits}{sep}{target}", "")?;
                }
            }
            if active > ACTIVE_AT_ONCE {
                writeln!(
                    text,
                    "\n{active} projects are active at once; fewer at a time finish sooner."
                )?;
            }
            print!("{text}");
            Ok(())
        }
        ProjectCmd::Show { key } => {
            let g = ws.progress(&key, now)?;
            if json {
                let node = ws.node(&key)?;
                let (_, parts) = ws.related(&key, Rel::Parent)?;
                return crate::output::print_json(
                    &json!({"node": node, "progress": g, "parts": parts}),
                );
            }
            let node = ws.node(&key)?;
            println!("{}  {}", node.key, node.title);
            println!(
                "{} · {}/{} tasks done · {}",
                node.state,
                g.done,
                g.total,
                health_word(&g)
            );
            if let Some(o) = &g.owner {
                println!("owner {o}");
            }
            if let Some(t) = &g.target {
                println!("target {t}");
            }
            if !node.body.is_empty() {
                println!("\n{}", node.body);
            }
            let waits = waiting_line(&g);
            if !waits.is_empty() {
                println!("\nwaiting on you: {waits}");
            }
            if !g.projects.is_empty() {
                println!("projects in it: {}", g.projects.join(", "));
            }
            let (_, parts) = ws.related(&key, Rel::Parent)?;
            if !parts.is_empty() {
                println!();
                for b in &parts {
                    println!("  {:<8} {:<9} {}", b.key, b.state.as_str(), b.title);
                }
            }
            if let Some(u) = &g.update {
                println!("\nupdate {} ({}): {}", u.at, u.health, u.text);
            }
            Ok(())
        }
        ProjectCmd::Update { key, health, text } => {
            let health: Health = health.parse()?;
            let events = ws.execute(
                Command::Update {
                    node: key.clone(),
                    health,
                    text,
                },
                by,
            )?;
            crate::output::noted(&key, &events, json)
        }
        ProjectCmd::Set { key, target, owner } => {
            let before = ws.node(&key)?;
            let events = ws.execute(
                Command::SetProject {
                    node: key,
                    target,
                    owner,
                },
                by,
            )?;
            crate::output::edited(ws, &before, &events, json)
        }
    }
}

/// How it stands in a word: its newest update's health, "update missing" once a week passes
/// without one while active, or "no update".
fn health_word(g: &Progress) -> &'static str {
    if g.update_missing {
        return "update missing";
    }
    match &g.update {
        Some(u) => match u.health {
            Health::OnTrack => "on track",
            Health::AtRisk => "at risk",
            Health::OffTrack => "off track",
        },
        None => "no update",
    }
}

/// What waits on the human, as "2 planned (oldest LIV-3 since …), 1 review (…)".
fn waiting_line(g: &Progress) -> String {
    g.waiting
        .iter()
        .map(|w| {
            format!(
                "{} {} (oldest {} since {})",
                w.count,
                w.state,
                w.oldest,
                w.since.get(..10).unwrap_or(&w.since)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `livtk flow`: time in each state, what waits on the human and for how long, a project's lead
/// time. Measured from the log.
pub fn flow(ws: &Workspace, project: Option<&str>, json: bool) -> Result<()> {
    let f = ws.flow(project, Timestamp::now())?;
    if json {
        return crate::output::print_json(&f);
    }
    let mut text = String::new();
    if f.states.is_empty() {
        text.push_str("No task has moved yet.\n");
    } else {
        writeln!(
            text,
            "Time in each state (median per task, then all together):"
        )?;
        for t in &f.states {
            writeln!(
                text,
                "  {:<9} {:>3} task(s)  {:>8}  {:>8}",
                t.state.as_str(),
                t.tasks,
                span(t.median_secs),
                span(t.total_secs)
            )?;
        }
    }
    if !f.queues.is_empty() {
        writeln!(text, "Waiting on the human:")?;
        for q in &f.queues {
            writeln!(
                text,
                "  {:<9} {:>3}  the longest {} ({})",
                q.state.as_str(),
                q.count,
                q.oldest,
                span(q.oldest_secs)
            )?;
        }
    }
    if let Some(l) = &f.lead {
        let state = if l.finished.is_some() {
            "done"
        } else {
            "so far"
        };
        writeln!(
            text,
            "Lead time: {} {state}, since {}",
            span(l.secs),
            l.started.get(..10).unwrap_or(&l.started)
        )?;
    }
    print!("{text}");
    Ok(())
}

/// A duration as people say it: `3d 4h`, `5h 20m`, `12m`, `40s`.
pub fn span(secs: i64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    match (d, h, m) {
        (0, 0, 0) => format!("{secs}s"),
        (0, 0, m) => format!("{m}m"),
        (0, h, m) => format!("{h}h {m}m"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}
