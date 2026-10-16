//! Everything `livtk` prints. Text for people, `--json` for programs: the JSON shapes are the
//! core's own types, so the CLI and the app agree by construction.

use std::fmt::Write as _;
use std::io::Write as _;

use anyhow::Result;
use livtk_core::event::{Event, Op};
use livtk_core::{Hit, Kind, Node, Rel, State, Summary, Workspace};
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
                let verb = match data.kind {
                    Kind::Task => "created",
                    Kind::Lesson | Kind::Decision => "proposed",
                    Kind::Finding => "recorded",
                };
                println!("{key} {verb}: {}", data.title);
                match data.kind {
                    Kind::Lesson => println!("  it binds once the human runs `livtk ratify {key}`"),
                    Kind::Decision => {
                        println!("  it binds once the human runs `livtk accept {key}`")
                    }
                    _ => {}
                }
            }
            Op::EdgeAdd { data } => {
                if let Some(dst) = key_of(ws, data.dst) {
                    let what = match data.rel {
                        Rel::DependsOn => "waits for",
                        Rel::Supersedes => "would replace",
                        Rel::LearnedFrom => "learned from",
                        Rel::Relates => "relates to",
                        Rel::Parent => "is part of",
                    };
                    println!("  {what} {dst}");
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
        println!(
            "Nothing here. `livtk list --all` includes what is over; `livtk new \"title\"` adds a task."
        );
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
        if t.kind == Kind::Task {
            writeln!(
                text,
                "  {:width$}  P{}  {}{flag}",
                t.key, t.priority, t.title
            )?;
        } else {
            writeln!(text, "  {:width$}  {}", t.key, t.title)?;
        }
    }
    print_block(&text)
}

pub fn show(ws: &Workspace, key: &str, json: bool) -> Result<()> {
    let node = ws.node(key)?;
    let links = ws.links(key)?;
    let history = ws.history(Some(key))?;
    let (replaces, replaced_by) = ws.related(key, Rel::Supersedes)?;
    let (learned_from, taught) = ws.related(key, Rel::LearnedFrom)?;
    let (part_of, parts) = ws.related(key, Rel::Parent)?;
    let (relates, related_by) = ws.related(key, Rel::Relates)?;
    // Relating has no direction worth showing: both ends list the other.
    let related: Vec<_> = relates.into_iter().chain(related_by).collect();
    if json {
        return print_json(&json!({
            "node": node, "links": links, "history": history,
            "supersedes": replaces, "superseded_by": replaced_by,
            "learned_from": learned_from, "taught": taught,
            "part_of": part_of, "sub_tasks": parts, "related": related,
        }));
    }
    println!("{}  {}", node.key, node.title);
    let mut facts = vec![node.state.to_string()];
    if node.kind == Kind::Task {
        facts.push(format!("P{}", node.priority));
    } else {
        facts.push(node.kind.to_string());
    }
    if node.props.get("scope").and_then(|v| v.as_str()) == Some("always") {
        facts.push("standing rule".into());
    }
    facts.push(format!("created {} by {}", node.created, node.created_by));
    if node.blocked {
        facts.push("blocked".into());
    }
    println!("{}", facts.join(" · "));
    for (label, briefs) in [
        ("part of", &part_of),
        ("sub-task", &parts),
        ("waits for", &links.depends_on),
        ("blocks", &links.blocks),
        ("related", &related),
        ("replaces", &replaces),
        ("replaced by", &replaced_by),
        ("from", &learned_from),
        ("taught", &taught),
    ] {
        for b in briefs {
            println!("{label:>11}  {} ({}) {}", b.key, b.state, b.title);
        }
    }
    if !node.body.is_empty() {
        println!("\n{}", node.body);
    }
    println!("\nhistory");
    let w = history.iter().map(|e| e.by.len()).max().unwrap_or(0);
    for e in &history {
        match &e.op {
            // Notes are the working record, so `show` prints them whole.
            Op::NodeNote { data, .. } => {
                println!("  {}  {:<w$}  note:", e.at, e.by);
                for line in data.text.lines() {
                    println!("      {line}");
                }
            }
            _ => println!("  {}  {:<w$}  {}", e.at, e.by, describe(ws, e)),
        }
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
    if before.kind.on_edit(before.state) == Some(after.state) {
        println!(
            "  the text changed, so it is {} again and needs the human's approval",
            after.state
        );
    }
    Ok(())
}

pub fn moved(ws: &Workspace, before: &Node, events: &[Event], json: bool) -> Result<()> {
    if json {
        return print_json(&events);
    }
    for e in events {
        let Op::NodeSet { node, data } = &e.op else {
            continue;
        };
        let key = key_of(ws, *node).unwrap_or_else(|| "?".into());
        match data.state {
            Some(to) if *node == before.id => println!("{key}: {} -> {to}", before.state),
            Some(to) => println!("{key} -> {to} (replaced by {})", before.key),
            None => {}
        }
        if let Some(props) = &data.props
            && let Some(scope) = props.get("scope")
        {
            if scope.as_str() == Some("always") {
                println!("{key}: a standing rule, shown at every session start");
            } else {
                println!("{key}: recalled when relevant");
            }
        }
    }
    if events.is_empty() {
        println!("{}: nothing changed", before.key);
    }
    Ok(())
}

pub fn inbox(items: &[Summary], json: bool) -> Result<()> {
    if json {
        return print_json(&items);
    }
    if items.is_empty() {
        println!("Nothing waits for you.");
        return Ok(());
    }
    let width = items.iter().map(|t| t.key.len()).max().unwrap_or(0);
    let mut text = String::new();
    for t in items {
        let action = match t.kind {
            Kind::Task => "ack",
            Kind::Lesson => "ratify or reject",
            Kind::Decision => "accept or reject",
            Kind::Finding => "",
        };
        writeln!(text, "  {:width$}  {:<16}  {}", t.key, action, t.title)?;
    }
    print_block(&text)
}

pub fn hits(hits: &[Hit], json: bool) -> Result<()> {
    if json {
        return print_json(&hits);
    }
    if hits.is_empty() {
        println!("Nothing found.");
        return Ok(());
    }
    let width = hits.iter().map(|h| h.key.len()).max().unwrap_or(0);
    let mut text = String::new();
    for h in hits {
        let meaning = h
            .cosine
            .map(|c| format!("  meaning {c:.2}"))
            .unwrap_or_default();
        writeln!(
            text,
            "  {:width$}  {:<9}  {}{meaning}",
            h.key,
            h.state.as_str(),
            h.title
        )?;
    }
    print_block(&text)
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

/// What `link` or `unlink` did, in the words it was given: `LIV-4 part-of LIV-1: linked`.
pub fn link_changed(
    ws: &Workspace,
    key: &str,
    rel: &str,
    other: &str,
    add: bool,
    events: &[Event],
    json: bool,
) -> Result<()> {
    if json {
        return print_json(&events);
    }
    let (k, o) = (ws.node(key)?.key, ws.node(other)?.key);
    let what = match (add, events.is_empty()) {
        (true, false) => "linked",
        (true, true) => "already linked",
        (false, _) => "unlinked",
    };
    println!("{k} {rel} {o}: {what}");
    Ok(())
}

pub fn noted(key: &str, events: &[Event], json: bool) -> Result<()> {
    if json {
        return print_json(&events);
    }
    println!("{key}: note added");
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
    let by_width = events.iter().map(|e| e.by.len()).max().unwrap_or(0);
    let mut text = String::new();
    for e in &events {
        let (subject, _) = e.op.subjects();
        let k = key_of(ws, subject).unwrap_or_else(|| "?".into());
        writeln!(
            text,
            "{}  {:<by_width$}  {:<8}  {}",
            e.at,
            e.by,
            k,
            describe(ws, e)
        )?;
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
        Op::NodeNote { data, .. } => {
            let first = data.text.lines().next().unwrap_or_default();
            let short: String = first.chars().take(80).collect();
            let more = if short.len() < data.text.len() {
                " ..."
            } else {
                ""
            };
            format!("note: {short}{more}")
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
