//! `livtk hook <event>`: Claude Code hooks. Each reads the hook's JSON on stdin and answers in
//! the documented shapes on stdout.
//!
//! Fail safe: outside a livtk project, on unreadable input, or on any internal error, a hook
//! prints nothing and exits 0, so it never breaks a session. Only the guard refuses things, and it
//! refuses through the documented JSON, never through a crash.

use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use livtk_core::event::Op;
use livtk_core::{Kind, State, Workspace};
use serde_json::{Value, json};

use crate::guard;

const BRIEF_MAX: usize = 1500;
const LIST_MAX: usize = 5;
const TITLE_SHOWN: usize = 80;
/// The agent that carries a task's work, and so must leave a note of it.
const IMPLEMENTER: &str = "livtk:implementer";
/// Prompts longer than this are usually pastes: no correction nudge, and recall reads the start.
const PROMPT_MAX: usize = 4000;
const RECALL_INPUT: usize = 2000;
const RULES_MAX: usize = 600;

/// Phrases that make a prompt read as a correction or a standing rule.
const CORRECTIONS: [&str; 30] = [
    "from now on",
    "going forward",
    "in the future",
    "remember to",
    "remember that",
    "make sure you",
    "make sure to",
    "never do",
    "never use",
    "never run",
    "never commit",
    "don't ever",
    "do not ever",
    "stop doing",
    "stop using",
    "i told you",
    "i already told",
    "i already said",
    "you keep",
    "you forgot",
    "you should not",
    "you shouldn't",
    "that's wrong",
    "that is wrong",
    "that's not what",
    "not what i asked",
    "why did you",
    "the rule is",
    "we always",
    "we never",
];

pub fn run(event: &str) -> ExitCode {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return ExitCode::SUCCESS;
    }
    let input: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    let answer = match event {
        "session-start" => session_start(&input),
        "user-prompt-submit" => user_prompt_submit(&input),
        "subagent-start" => subagent_start(&input),
        "pre-tool-use" => pre_tool_use(&input),
        // Formatting is the project's own hooks' to do; an older hooks.json still calls this.
        "post-tool-use" => None,
        "subagent-stop" => unrecorded_stop(&input, "subagent-stop"),
        "stop" => stop(&input),
        // A hooks.json newer than this binary: stay out of the way.
        _ => None,
    };
    if let Some(answer) = answer {
        println!("{answer}");
    }
    ExitCode::SUCCESS
}

// ---- events -----------------------------------------------------------------------------------

/// The project the hook runs in. Outside one, the hook says nothing; a project whose config
/// livtk cannot read says so, so a typo never silences livtk without a word.
fn open(input: &Value, event: &str) -> Result<Workspace, Option<Value>> {
    match Workspace::open(&cwd(input)) {
        Ok(ws) => Ok(ws),
        Err(e @ livtk_core::Error::Config { .. }) => Err(Some(context(
            event,
            format!(
                "livtk cannot read this project's config, so it tells you nothing else here: {e}. \
                 Tell the developer; only they edit .livtk/.\n"
            ),
        ))),
        Err(_) => Err(None),
    }
}

fn session_start(input: &Value) -> Option<Value> {
    let mut ws = match open(input, "SessionStart") {
        Ok(ws) => ws,
        Err(said) => return said,
    };
    if !ws.config().hook_on("session-start") {
        return None;
    }
    let mut text = brief(&ws, false)?;
    // What the project's config asks that livtk no longer does: said where Claude reads it.
    for w in ws.take_warnings() {
        let _ = writeln!(text, "livtk warning: {w}");
    }
    Some(context("SessionStart", text))
}

/// Mark the session writing (a turn begins) or not (it ends), beside the log: so the app shows
/// "writing" on its work for a session livtk did not start.
fn mark_writing(input: &Value, on: bool) {
    let Some(session) = input.get("session_id").and_then(Value::as_str) else {
        return;
    };
    if let Ok(dir) = livtk_core::find_dir(&cwd(input)) {
        let _ = livtk_core::run::mark_writing(&dir, session, on);
    }
}

fn stop(input: &Value) -> Option<Value> {
    mark_writing(input, false);
    unrecorded_stop(input, "stop")
}

fn user_prompt_submit(input: &Value) -> Option<Value> {
    mark_writing(input, true);
    let prompt = input.get("prompt")?.as_str()?;
    if prompt.trim().is_empty() {
        return None;
    }
    let mut ws = match open(input, "UserPromptSubmit") {
        Ok(ws) => ws,
        Err(said) => return said,
    };
    if !ws.config().hook_on("user-prompt-submit") {
        return None;
    }
    let say = ws.config().user_prompt_submit.clone();
    let head: String = prompt.chars().take(RECALL_INPUT).collect();
    let mut text = String::new();
    let hits = if say.has("memory") {
        ws.recall(&head).unwrap_or_default()
    } else {
        Vec::new()
    };
    if !hits.is_empty() {
        text.push_str("livtk memory relevant to this prompt (`livtk show KEY` gives the why):\n");
        for h in &hits {
            let what = match h.kind {
                Kind::Lesson => "rule",
                Kind::Decision => "decision",
                _ => "finding",
            };
            // Whole: a rule cut short can lose the very instruction. Titles are at most 200 characters.
            let _ = writeln!(text, "- {} ({what}) {}", h.key, h.title);
        }
    }
    if say.has("nudge") && prompt.len() <= PROMPT_MAX && reads_as_correction(prompt) {
        text.push_str(
            "This reads like a correction or a standing rule. After you act on it, if it should \
             hold beyond this task, propose it: `livtk lesson \"<the rule in one sentence>\" \
             --why \"<what happened>\"`. The human ratifies it; you never do.\n",
        );
    }
    (!text.is_empty()).then(|| context("UserPromptSubmit", text))
}

/// Whether a prompt reads as a correction or a standing rule.
fn reads_as_correction(prompt: &str) -> bool {
    let words: String = prompt
        .to_lowercase()
        .replace(['\u{2019}', '`'], "'")
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let padded = format!(
        " {} ",
        words.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    CORRECTIONS
        .iter()
        .any(|p| padded.contains(&format!(" {p} ")))
}

fn subagent_start(input: &Value) -> Option<Value> {
    let ws = match open(input, "SubagentStart") {
        Ok(ws) => ws,
        Err(said) => return said,
    };
    if !ws.config().hook_on("subagent-start") {
        return None;
    }
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
        // Signed with the session it comes from, and the subagent when one runs it.
        let flags = guard::signature(
            input.get("session_id").and_then(Value::as_str),
            input.get("agent_type").and_then(Value::as_str),
        );
        let signed = guard::sign(cmd, &flags)?;
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

/// A stop that would leave the work unrecorded: the implementer stopping (as a subagent, or as a
/// session started as the implementer, which a run is) while a task it worked on (one it moved
/// or wrote to) is in progress with nothing recorded about it. Others' tasks are theirs to
/// record. What only livtk can know, so the one thing it refuses at a stop; a project's own
/// checks run from its own hooks. `hook` is the name a project switches it off by.
fn unrecorded_stop(input: &Value, hook: &str) -> Option<Value> {
    // Once blocked already: let it stop rather than loop.
    if input.get("stop_hook_active").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let agent = input.get("agent_type")?.as_str()?;
    if agent != IMPLEMENTER {
        return None;
    }
    let ws = Workspace::open(&cwd(input)).ok()?;
    if !ws.config().hook_on(hook) {
        return None;
    }
    // Its events are signed with its name (see the guard).
    let by = format!("claude:{}", guard::agent_name(agent));
    let silent: Vec<String> = ws
        .tasks(&[State::Doing])
        .ok()?
        .into_iter()
        .filter(|t| {
            let Ok(history) = ws.history(Some(&t.key)) else {
                return false;
            };
            history.iter().any(|e| e.by == by)
                && !history.iter().any(|e| matches!(e.op, Op::NodeNote { .. }))
        })
        .map(|t| t.key)
        .collect();
    if silent.is_empty() {
        return None;
    }
    let shown = silent
        .iter()
        .take(5)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let more = silent.len().saturating_sub(5);
    let more = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    Some(json!({
        "decision": "block",
        "reason": format!(
            "{shown}{more}: in progress, worked on by you, with nothing recorded about it. Before stopping, note what you did, tried or found: `livtk note KEY \"...\"`.",
        ),
    }))
}

// ---- the brief --------------------------------------------------------------------------------

/// What Claude should know about the project's work, in a few lines.
fn brief(ws: &Workspace, subagent: bool) -> Option<String> {
    let doing = ws.tasks(&[State::Doing]).ok()?;
    let rules = ws.always_rules().ok()?;
    let prefix = &ws.config().prefix;
    let mut b = String::new();
    if subagent {
        let _ = writeln!(
            b,
            "livtk ({prefix}): this project tracks its work with the `livtk` CLI."
        );
        section(&mut b, "In progress", &doing, ws, false);
        standing_rules(&mut b, &rules);
        b.push_str(
            "Your livtk commands are signed with your agent name automatically. `livtk show KEY` \
             gives a task's plan and acceptance check. Record what you find or try with \
             `livtk note KEY \"...\"`. Never edit .livtk/ by hand; only the human acknowledges \
             plans and ratifies rules.\n",
        );
        return Some(clip(b));
    }
    let review = ws.tasks(&[State::Review]).ok()?;
    let waiting = ws.tasks(&[State::Planned]).ok()?;
    let inbox = ws.inbox().ok()?;
    // The sections the project keeps (`[session_start] include`): all, unless it names some.
    let say = &ws.config().session_start;
    if say.has("intro") {
        let _ = writeln!(
            b,
            "livtk ({prefix}): this project tracks its work with the `livtk` CLI (`livtk --help`). \
             The livtk:plan, livtk:work and livtk:remember skills describe the workflow."
        );
    }
    if say.has("tasks") {
        section(&mut b, "In progress", &doing, ws, true);
        section(&mut b, "In review", &review, ws, false);
        section(
            &mut b,
            "Waiting for the human to acknowledge (do not start these)",
            &waiting,
            ws,
            false,
        );
    }
    if say.has("projects") {
        projects(&mut b, ws);
    }
    if say.has("gates") {
        // The built-in gates are said in the protocol; without it, here.
        if !say.has("protocol") {
            b.push_str(
                "Only the human acknowledges a plan and decides what binds (ratifying a lesson, \
                 accepting a decision).\n",
            );
        }
        if !ws.config().gates.human.is_empty() {
            let _ = writeln!(
                b,
                "This project also keeps these moves for the human: {}.",
                ws.config().gates.human.join("; ")
            );
        }
    }
    if say.has("tasks") {
        waiting_and_next(&mut b, ws, &doing, &inbox)?;
    }
    if say.has("rules") {
        standing_rules(&mut b, &rules);
    }
    if say.has("protocol") {
        b.push_str(
            "How to work: plan work as livtk tasks and wait for the human's acknowledgement before \
             starting one; only the human acknowledges and ratifies; never edit .livtk/ by hand; \
             record what you try with `livtk note KEY \"...\"`; propose what should hold beyond one \
             task as a lesson (`livtk lesson`); commit .livtk/ with the work it describes.\n",
        );
    }
    (!b.is_empty()).then(|| clip(b))
}

/// What else waits for the human, and the task to start next.
fn waiting_and_next(
    b: &mut String,
    ws: &Workspace,
    doing: &[livtk_core::Summary],
    inbox: &[livtk_core::Summary],
) -> Option<()> {
    let lessons = inbox.iter().filter(|s| s.kind == Kind::Lesson).count();
    let decisions = inbox.iter().filter(|s| s.kind == Kind::Decision).count();
    let projects = inbox.iter().filter(|s| s.kind == Kind::Project).count();
    if projects > 0 {
        let _ = writeln!(
            b,
            "Also waiting for the human: {projects} project(s) to commit to (`livtk inbox`)."
        );
    }
    if lessons + decisions > 0 {
        let _ = writeln!(
            b,
            "Also waiting for the human: {lessons} lesson(s) to ratify, {decisions} decision(s) \
             to accept (`livtk inbox`)."
        );
    }
    match ws.next().ok()? {
        Some(n) => {
            let _ = writeln!(b, "Next ready to start: {} {}", n.key, short(&n.title));
        }
        None if doing.is_empty() => b.push_str("Nothing is ready to start.\n"),
        None => {}
    }
    Some(())
}

/// The ratified standing rules, within a fixed budget.
fn standing_rules(b: &mut String, rules: &[livtk_core::Brief]) {
    if rules.is_empty() {
        return;
    }
    let mut block = String::from("Standing rules (ratified by the human; follow them):\n");
    for r in rules {
        let line = format!("- {} {}\n", r.key, r.title);
        if block.len() + line.len() > RULES_MAX {
            block.push_str("- and more: `livtk list --kind lesson`\n");
            break;
        }
        block.push_str(&line);
    }
    b.push_str(&block);
}

/// The active projects: how far each has come, how it stands, what waits on the human, and an
/// update asked for when one is missing.
fn projects(b: &mut String, ws: &Workspace) {
    let now = jiff::Timestamp::now();
    let active: Vec<_> = ws
        .projects()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.state == State::Active)
        .collect();
    if active.is_empty() {
        return;
    }
    let _ = writeln!(b, "Active projects:");
    for p in active.iter().take(LIST_MAX) {
        let Ok(g) = ws.progress(&p.key, now) else {
            continue;
        };
        let health = match (&g.update, g.update_missing) {
            (_, true) => "update missing".to_owned(),
            (Some(u), false) => u.health.as_str().replace('_', " "),
            (None, false) => "no update yet".to_owned(),
        };
        let waiting: usize = g.waiting.iter().map(|w| w.count).sum();
        let _ = write!(
            b,
            "  {} {}: {}/{} tasks done, {health}",
            p.key,
            short(&p.title),
            g.done,
            g.total
        );
        if waiting > 0 {
            let _ = write!(b, ", {waiting} waiting on the human");
        }
        b.push('\n');
        if g.update_missing {
            let _ = writeln!(
                b,
                "    write one: `livtk project update {} --health on_track|at_risk|off_track \"what moved; what is next\"`",
                p.key
            );
        }
    }
}

fn section(b: &mut String, title: &str, tasks: &[livtk_core::Summary], ws: &Workspace, last: bool) {
    if tasks.is_empty() {
        return;
    }
    // Which project each task serves, so its outcome travels with it.
    let projects = ws.projects_of().unwrap_or_default();
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
        // The outcome it serves: its project's "Done when".
        if let Some((_, p)) = projects.get(&t.id) {
            let outcome = ws
                .node(&p.key)
                .ok()
                .and_then(|n| livtk_core::done_when(&n.body).map(short));
            let _ = writeln!(
                b,
                "    part of {} {}{}",
                p.key,
                short(&p.title),
                outcome.map(|o| format!(": {o}")).unwrap_or_default()
            );
        }
        // Its docs, to read: what the work is written against.
        let docs: Vec<String> = ws
            .docs_of(&t.key)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(path, title)| title.map(|t| format!("{path} ({})", short(&t))))
            .collect();
        if !docs.is_empty() {
            let _ = writeln!(b, "    docs to read: {}", docs.join(", "));
        }
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

#[cfg(test)]
mod tests {
    use super::reads_as_correction;

    #[test]
    fn corrections_are_spotted_and_requests_are_not() {
        for p in [
            "From now on, run the tests first.",
            "no — I told you not to push",
            "That\u{2019}s wrong, use the other crate",
            "why did you delete that file?",
            "make sure to commit .livtk too",
            "We never use unwrap here.",
        ] {
            assert!(reads_as_correction(p), "{p}");
        }
        for p in [
            "add a button that saves the draft",
            "make the window wider",
            "can you explain how recall works",
            "use the newer API instead of the old one",
            "remember the password field needs a label",
        ] {
            assert!(!reads_as_correction(p), "{p}");
        }
    }
}
