//! Everything `livtk` prints. Text for people, `--json` for programs: the JSON shapes are the
//! core's own types, so the CLI and the app agree by construction.

use std::fmt::Write as _;
use std::io::Write as _;

use anyhow::Result;
use livtk_core::event::{Event, Op};
use livtk_core::{Node, State, Summary, Workspace};
use serde::Serialize;
use serde_json::json;

fn print_json(value: &impl Serialize) -> Result<()> {
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    serde_json::to_writer(&mut out, value)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

/// Write a whole block of text at once: stdout flushes on every newline otherwise.
fn print_block(text: &str) -> Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes())?;
    out.flush()?;
    Ok(())
}

pub fn initialised(ws: &Workspace, json: bool) -> Result<()> {
    if json {
        return print_json(&json!({ "dir": ws.dir(), "prefix": ws.config().prefix }));
    }
    println!(
        "Initialised {} with prefix {}. Commit .livtk/ with the project.",
        ws.dir().display(),
        ws.config().prefix
    );
    Ok(())
}

pub fn created(ws: &Workspace, events: &[Event], json: bool) -> Result<()> {
    if json {
        return print_json(&events);
    }
    for e in events {
        match &e.op {
            Op::NodeCreate { node, data } => {
                let key = key_of(ws, *node).unwrap_or_else(|| data.key.clone());
                println!("{key} created: {}", data.title);
            }
            Op::EdgeAdd { data } => {
                if let Some(dst) = key_of(ws, data.dst) {
                    println!("  waits for {dst}");
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn list(tasks: &[Summary], json: bool) -> Result<()> {
    if json {
        return print_json(&tasks);
    }
    if tasks.is_empty() {
        println!("No tasks here. Create one with `livtk new \"title\"`.");
        return Ok(());
    }
    let width = tasks.iter().map(|t| t.key.len()).max().unwrap_or(0);
    let mut text = String::with_capacity(tasks.len() * 64);
    let mut current: Option<State> = None;
    for t in tasks {
        if current != Some(t.state) {
            if current.is_some() {
                text.push('\n');
            }
            text.push_str(&t.state.as_str().to_uppercase());
            text.push('\n');
            current = Some(t.state);
        }
        let flag = if t.blocked && !t.state.is_closed() {
            "  (blocked)"
        } else {
            ""
        };
        writeln!(
            text,
            "  {:width$}  P{}  {}{flag}",
            t.key, t.priority, t.title
        )?;
    }
    print_block(&text)
}

pub fn show(ws: &Workspace, key: &str, json: bool) -> Result<()> {
    let node = ws.node(key)?;
    let links = ws.links(key)?;
    let history = ws.history(Some(key))?;
    if json {
        return print_json(&json!({ "node": node, "links": links, "history": history }));
    }
    println!("{}  {}", node.key, node.title);
    let blocked = if node.blocked { "   blocked" } else { "" };
    println!(
        "{} · P{} · created {} by {}{blocked}",
        node.state, node.priority, node.created, node.created_by
    );
    for (label, briefs) in [("waits for", &links.depends_on), ("blocks", &links.blocks)] {
        for b in briefs {
            println!("{label:>9}  {} ({}) {}", b.key, b.state, b.title);
        }
    }
    if !node.body.is_empty() {
        println!("\n{}", node.body);
    }
    println!("\nhistory");
    for e in &history {
        println!("  {}  {:<8}  {}", e.at, e.by, describe(ws, e));
    }
    Ok(())
}

pub fn edited(ws: &Workspace, before: &Node, events: &[Event], json: bool) -> Result<()> {
    if json {
        return print_json(&events);
    }
    if events.is_empty() {
        println!("{}: nothing changed", before.key);
        return Ok(());
    }
    let after = ws.node(&before.key)?;
    println!("{} updated", after.key);
    if before.state == State::Acked && after.state == State::Planned {
        println!("  the plan changed, so it is planned again and needs a new acknowledgement");
    }
    Ok(())
}

pub fn moved(before: &Node, to: State, events: &[Event], json: bool) -> Result<()> {
    if json {
        return print_json(&events);
    }
    println!("{}: {} -> {to}", before.key, before.state);
    Ok(())
}

pub fn linked(
    ws: &Workspace,
    src: &str,
    dst: &str,
    add: bool,
    events: &[Event],
    json: bool,
) -> Result<()> {
    if json {
        return print_json(&events);
    }
    let (s, d) = (ws.node(src)?.key, ws.node(dst)?.key);
    match (add, events.is_empty()) {
        (true, false) => println!("{s} now waits for {d}"),
        (true, true) => println!("{s} already waits for {d}"),
        (false, _) => println!("{s} no longer waits for {d}"),
    }
    Ok(())
}

pub fn next(node: Option<&Node>, json: bool) -> Result<()> {
    if json {
        return print_json(&node);
    }
    match node {
        Some(n) => println!("{}  P{}  {}", n.key, n.priority, n.title),
        None => {
            println!("Nothing is ready: no acknowledged task is free of unfinished dependencies.")
        }
    }
    Ok(())
}

pub fn log(ws: &Workspace, key: Option<&str>, json: bool) -> Result<()> {
    let events = ws.history(key)?;
    if json {
        return print_json(&events);
    }
    let mut text = String::new();
    for e in &events {
        let (subject, _) = e.op.subjects();
        let k = key_of(ws, subject).unwrap_or_else(|| "?".into());
        writeln!(text, "{}  {:<8}  {:<8}  {}", e.at, e.by, k, describe(ws, e))?;
    }
    print_block(&text)
}

pub fn rebuilt(ws: &Workspace, json: bool) -> Result<()> {
    let snapshot = ws.snapshot()?;
    if json {
        return print_json(
            &json!({ "nodes": snapshot.nodes.len(), "edges": snapshot.edges.len() }),
        );
    }
    println!(
        "Rebuilt from the log: {} nodes, {} links.",
        snapshot.nodes.len(),
        snapshot.edges.len()
    );
    Ok(())
}

fn key_of(ws: &Workspace, id: uuid::Uuid) -> Option<String> {
    ws.node(&id.to_string()).ok().map(|n| n.key)
}

/// One event as a short phrase.
fn describe(ws: &Workspace, e: &Event) -> String {
    match &e.op {
        Op::NodeCreate { node, data } => {
            // A key renumbered after a merge still shows the key it was created with.
            match key_of(ws, *node) {
                Some(k) if k != data.key => format!("created \"{}\" as {}", data.title, data.key),
                _ => format!("created \"{}\"", data.title),
            }
        }
        Op::NodeSet { data, .. } => {
            let mut parts = Vec::new();
            if let Some(s) = data.state {
                parts.push(format!("-> {s}"));
            }
            if let Some(t) = &data.title {
                parts.push(format!("title \"{t}\""));
            }
            if data.body.is_some() {
                parts.push("body edited".into());
            }
            if let Some(p) = data.priority {
                parts.push(format!("priority P{}", p.get()));
            }
            if data.props.is_some() {
                parts.push("props edited".into());
            }
            parts.join(", ")
        }
        Op::EdgeAdd { data } | Op::EdgeRemove { data } => {
            let what = if matches!(e.op, Op::EdgeAdd { .. }) {
                "linked"
            } else {
                "unlinked"
            };
            let src = key_of(ws, data.src).unwrap_or_else(|| "?".into());
            let dst = key_of(ws, data.dst).unwrap_or_else(|| "?".into());
            format!("{what}: {src} {} {dst}", data.rel)
        }
    }
}
