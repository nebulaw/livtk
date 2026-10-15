//! `livtk hook <event>`: Claude Code hooks. Each reads the hook's JSON on stdin and answers in
//! the documented shapes on stdout.
//!
//! Fail safe: outside a livtk project, on unreadable input, or on any internal error, a hook
//! prints nothing and exits 0, so it never breaks a session. Only the guard refuses things, and it
//! refuses through the documented JSON, never through a crash.

use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use livtk_core::event::Op;
use livtk_core::{Config, State, Workspace, find_dir};
use serde_json::{Value, json};

use crate::guard;

const BRIEF_MAX: usize = 1500;
const LIST_MAX: usize = 5;
const TITLE_SHOWN: usize = 80;
const CHECK_LINES: usize = 40;

pub fn run(event: &str) -> ExitCode {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return ExitCode::SUCCESS;
    }
    let input: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    let answer = match event {
        "session-start" => session_start(&input),
        "subagent-start" => subagent_start(&input),
        "pre-tool-use" => pre_tool_use(&input),
        "post-tool-use" => post_tool_use(&input),
        "subagent-stop" => subagent_stop(&input),
        // A hooks.json newer than this binary: stay out of the way.
        _ => None,
    };
    if let Some(answer) = answer {
        println!("{answer}");
    }
    ExitCode::SUCCESS
}

// ---- events -----------------------------------------------------------------------------------

fn session_start(input: &Value) -> Option<Value> {
    let ws = Workspace::open(&cwd(input)).ok()?;
    Some(context("SessionStart", brief(&ws, false)?))
}

fn subagent_start(input: &Value) -> Option<Value> {
    let ws = Workspace::open(&cwd(input)).ok()?;
    Some(context("SubagentStart", brief(&ws, true)?))
}

fn pre_tool_use(input: &Value) -> Option<Value> {
    let tool = input.get("tool_name")?.as_str()?;
    let tool_input = input.get("tool_input")?;
    if tool == "Bash" {
        let cmd = tool_input.get("command")?.as_str()?;
        if let Some(reason) = guard::refuse_bash(cmd) {
            return Some(deny(&reason));
        }
        let agent = input
            .get("agent_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if agent.is_empty() {
            return None;
        }
        let signed = guard::sign(cmd, &guard::agent_name(agent))?;
        let mut updated = tool_input.clone();
        updated["command"] = Value::String(signed);
        return Some(json!({
            "hookSpecificOutput": { "hookEventName": "PreToolUse", "updatedInput": updated }
        }));
    }
    let path = tool_input
        .get("file_path")
        .or_else(|| tool_input.get("notebook_path"))?
        .as_str()?;
    guard::refuse_path(path).map(|reason| deny(&reason))
}

fn post_tool_use(input: &Value) -> Option<Value> {
    let file = PathBuf::from(input.get("tool_input")?.get("file_path")?.as_str()?);
    let dir = find_dir(file.parent()?).ok()?;
    let hooks = Config::load(&dir).ok()?.hooks;
    let ext = file.extension()?.to_str()?;
    if hooks.format.is_empty() || !hooks.format_extensions.iter().any(|e| e == ext) {
        return None;
    }
    let args: Vec<String> = hooks
        .format
        .iter()
        .map(|a| a.replace("{file}", &file.to_string_lossy()))
        .collect();
    let (program, rest) = args.split_first()?;
    // A formatter that fails leaves the file as Claude wrote it; the check will speak up.
    let _ = Command::new(program)
        .args(rest)
        .current_dir(dir.parent()?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    None
}

fn subagent_stop(input: &Value) -> Option<Value> {
    // Once blocked already: let it stop rather than loop.
    if input.get("stop_hook_active").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let agent = input.get("agent_type")?.as_str()?;
    let dir = find_dir(&cwd(input)).ok()?;
    let hooks = Config::load(&dir).ok()?.hooks;
    if hooks.check.is_empty() || !hooks.check_agents.iter().any(|a| a == agent) {
        return None;
    }
    let (program, rest) = hooks.check.split_first()?;
    let out = Command::new(program)
        .args(rest)
        .current_dir(dir.parent()?)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if out.status.success() {
        return None;
    }
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(CHECK_LINES)..].join("\n");
    Some(json!({
        "decision": "block",
        "reason": format!(
            "The project check `{}` fails. Fix it before finishing:\n{tail}",
            hooks.check.join(" ")
        ),
    }))
}

// ---- the brief --------------------------------------------------------------------------------

/// What Claude should know about the project's work, in a few lines.
fn brief(ws: &Workspace, subagent: bool) -> Option<String> {
    let doing = ws.tasks(&[State::Doing]).ok()?;
    let review = ws.tasks(&[State::Review]).ok()?;
    let mut b = String::new();
    let prefix = &ws.config().prefix;
    if subagent {
        let _ = writeln!(
            b,
            "livtk ({prefix}): this project tracks its work with the `livtk` CLI."
        );
        section(&mut b, "In progress", &doing, ws, false);
        b.push_str(
            "Your livtk commands are signed with your agent name automatically. `livtk show KEY` \
             gives a task's plan and acceptance check. Record what you find or try with \
             `livtk note KEY \"...\"`. Never edit .livtk/ by hand; only the human acknowledges.\n",
        );
        return Some(clip(b));
    }
    let waiting = ws.tasks(&[State::Planned]).ok()?;
    let _ = writeln!(
        b,
        "livtk ({prefix}): this project tracks its work with the `livtk` CLI (`livtk --help`). \
         The livtk:plan and livtk:work skills describe the workflow."
    );
    section(&mut b, "In progress", &doing, ws, true);
    section(&mut b, "In review", &review, ws, false);
    section(
        &mut b,
        "Waiting for the human to acknowledge (do not start these)",
        &waiting,
        ws,
        false,
    );
    match ws.next().ok()? {
        Some(n) => {
            let _ = writeln!(b, "Next ready to start: {} {}", n.key, short(&n.title));
        }
        None if doing.is_empty() => b.push_str("Nothing is ready to start.\n"),
        None => {}
    }
    b.push_str(
        "Rules: plan work as livtk tasks and wait for the human's acknowledgement before starting \
         one; only the human acknowledges; never edit .livtk/ by hand; record what you try or \
         learn with `livtk note KEY \"...\"`; commit .livtk/ with the work it describes.\n",
    );
    Some(clip(b))
}

fn section(b: &mut String, title: &str, tasks: &[livtk_core::Summary], ws: &Workspace, last: bool) {
    if tasks.is_empty() {
        return;
    }
    let _ = writeln!(b, "{title}:");
    for t in tasks.iter().take(LIST_MAX) {
        let _ = write!(b, "  {} {}", t.key, short(&t.title));
        if t.blocked {
            b.push_str(" (blocked)");
        }
        if last
            && let Some(e) = ws
                .history(Some(&t.key))
                .ok()
                .and_then(|h| h.into_iter().last())
        {
            let what = match &e.op {
                Op::NodeNote { data, .. } => {
                    format!(
                        "note \"{}\"",
                        short(data.text.lines().next().unwrap_or_default())
                    )
                }
                Op::NodeSet { data, .. } if data.state.is_some() => {
                    format!(
                        "moved to {}",
                        data.state.map(State::as_str).unwrap_or_default()
                    )
                }
                op => op.name().to_owned(),
            };
            let _ = write!(b, " (last: {what}, by {})", e.by);
        }
        b.push('\n');
    }
    if tasks.len() > LIST_MAX {
        let _ = writeln!(b, "  and {} more (`livtk list`)", tasks.len() - LIST_MAX);
    }
}

fn short(title: &str) -> String {
    if title.chars().count() <= TITLE_SHOWN {
        title.to_owned()
    } else {
        let cut: String = title.chars().take(TITLE_SHOWN - 3).collect();
        format!("{cut}...")
    }
}

fn clip(mut b: String) -> String {
    if b.len() > BRIEF_MAX {
        let mut end = BRIEF_MAX;
        while !b.is_char_boundary(end) {
            end -= 1;
        }
        b.truncate(end);
        b.push_str("\n(brief cut short; run `livtk list`)\n");
    }
    b
}

// ---- answers ----------------------------------------------------------------------------------

fn context(event: &str, text: String) -> Value {
    json!({ "hookSpecificOutput": { "hookEventName": event, "additionalContext": text } })
}

fn deny(reason: &str) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
}

fn cwd(input: &Value) -> PathBuf {
    input
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| Path::new(".").to_path_buf())
}
