//! `livtk hook <event>` as Claude Code runs it: a process with the hook JSON on stdin.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

fn hook(event: &str, input: &Value) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_livtk"))
        .args(["hook", event])
        .env("CLAUDECODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "hooks always exit 0");
    out
}

/// The hook's JSON answer, or `None` when it stayed silent.
fn answer(event: &str, input: &Value) -> Option<Value> {
    let out = hook(event, input);
    let text = String::from_utf8(out.stdout).unwrap();
    (!text.trim().is_empty()).then(|| serde_json::from_str(&text).unwrap())
}

fn livtk(dir: &Path, human: bool, args: &[&str]) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_livtk"));
    cmd.current_dir(dir).args(args);
    if human {
        cmd.env_remove("CLAUDECODE");
    } else {
        cmd.env("CLAUDECODE", "1");
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    livtk(dir.path(), true, &["init", "--prefix", "LIV"]);
    dir
}

fn with_hooks(dir: &Path, toml: &str) {
    let path = dir.join(".livtk/config.toml");
    let mut config = fs::read_to_string(&path).unwrap();
    config.push_str("\n[hooks]\n");
    config.push_str(toml);
    fs::write(path, config).unwrap();
}

fn bash(cmd: &str, agent: Option<&str>) -> Value {
    let mut input = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": cmd, "description": "keep me", "timeout": 120000 },
    });
    if let Some(a) = agent {
        input["agent_id"] = json!("a1");
        input["agent_type"] = json!(a);
    }
    input
}

fn context_of(v: &Value) -> &str {
    v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
}

// ---- briefs -------------------------------------------------------------------------------------

#[test]
fn the_session_brief_shows_the_state_of_the_work() {
    let dir = project();
    let d = dir.path();
    livtk(d, false, &["new", "draw a window"]);
    livtk(d, false, &["new", "wire the input"]);
    livtk(d, false, &["new", "later"]);
    for (k, human, args) in [
        ("1", false, vec!["move", "1", "planned"]),
        ("1", true, vec!["ack", "1"]),
        ("1", false, vec!["start", "1"]),
        ("1", false, vec!["note", "1", "winit backend first"]),
        ("2", false, vec!["move", "2", "planned"]),
    ] {
        let _ = k;
        livtk(d, human, &args);
    }
    let input = json!({ "hook_event_name": "SessionStart", "source": "startup", "cwd": d });
    let v = answer("session-start", &input).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
    let brief = context_of(&v);
    assert!(brief.contains("livtk (LIV)"), "{brief}");
    assert!(
        brief.contains(
            "In progress:\n  LIV-1 draw a window (last: note \"winit backend first\", by claude)"
        ),
        "{brief}"
    );
    assert!(
        brief.contains(
            "Waiting for the human to acknowledge (do not start these):\n  LIV-2 wire the input"
        ),
        "{brief}"
    );
    assert!(brief.contains("only the human acknowledges"), "{brief}");
    assert!(brief.len() <= 1600, "{} characters", brief.len());

    let compact = json!({ "hook_event_name": "SessionStart", "source": "compact", "cwd": d });
    assert_eq!(
        answer("session-start", &compact),
        Some(v),
        "the same brief after compaction"
    );
}

#[test]
fn a_subagent_brief_is_short_and_says_it_is_signed() {
    let dir = project();
    livtk(dir.path(), false, &["new", "x"]);
    let input = json!({ "hook_event_name": "SubagentStart", "agent_type": "livtk:implementer", "cwd": dir.path() });
    let v = answer("subagent-start", &input).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SubagentStart");
    assert!(context_of(&v).contains("signed with your agent name"));
}

#[test]
fn outside_a_project_and_on_bad_input_every_hook_is_silent() {
    let elsewhere = tempfile::tempdir().unwrap();
    let input = json!({ "cwd": elsewhere.path(), "agent_type": "livtk:implementer", "tool_name": "Edit",
                        "tool_input": { "file_path": elsewhere.path().join("a.rs") } });
    for event in [
        "session-start",
        "subagent-start",
        "post-tool-use",
        "subagent-stop",
        "pre-tool-use",
        "from-the-future",
    ] {
        assert_eq!(answer(event, &input), None, "{event}");
        let out = hook(event, &json!("not an object"));
        assert!(out.stdout.is_empty(), "{event}");
    }
}

// ---- the guard ----------------------------------------------------------------------------------

#[test]
fn the_guard_refuses_human_steps_and_explains() {
    for cmd in [
        "livtk ack 1",
        "cd x && livtk --json move 2 acked",
        "env -u CLAUDECODE livtk list",
        "sh -c \"livtk ack 1\"",
    ] {
        let v = answer("pre-tool-use", &bash(cmd, None)).unwrap();
        let out = &v["hookSpecificOutput"];
        assert_eq!(out["hookEventName"], "PreToolUse");
        assert_eq!(out["permissionDecision"], "deny", "{cmd}");
        assert!(out["permissionDecisionReason"].as_str().unwrap().len() > 20);
    }
    for cmd in ["livtk list", "git add .livtk", "cargo test"] {
        assert_eq!(answer("pre-tool-use", &bash(cmd, None)), None, "{cmd}");
    }
}

#[test]
fn the_guard_keeps_file_tools_out_of_livtk() {
    for tool in ["Edit", "Write", "MultiEdit"] {
        let input =
            json!({ "tool_name": tool, "tool_input": { "file_path": "/p/.livtk/events.jsonl" } });
        let v = answer("pre-tool-use", &input).unwrap();
        assert_eq!(
            v["hookSpecificOutput"]["permissionDecision"], "deny",
            "{tool}"
        );
        let fine = json!({ "tool_name": tool, "tool_input": { "file_path": "/p/src/main.rs" } });
        assert_eq!(answer("pre-tool-use", &fine), None, "{tool}");
    }
}

#[test]
fn a_subagents_livtk_commands_are_signed_and_nothing_else_changes() {
    let v = answer(
        "pre-tool-use",
        &bash("livtk note 3 found it", Some("livtk:refuter")),
    )
    .unwrap();
    let updated = &v["hookSpecificOutput"]["updatedInput"];
    assert_eq!(
        updated["command"],
        "LIVTK_AGENT=refuter livtk note 3 found it"
    );
    assert_eq!(updated["description"], "keep me");
    assert_eq!(updated["timeout"], 120000);
    assert!(
        v["hookSpecificOutput"].get("permissionDecision").is_none(),
        "normal permissions still apply"
    );

    assert_eq!(
        answer("pre-tool-use", &bash("cargo test", Some("livtk:refuter"))),
        None
    );
    assert_eq!(
        answer("pre-tool-use", &bash("livtk list", None)),
        None,
        "the main session is not rewritten"
    );
}

#[test]
fn a_signed_command_really_signs_the_event() {
    let dir = project();
    let v = answer(
        "pre-tool-use",
        &bash("livtk new \"from the refuter\"", Some("livtk:refuter")),
    )
    .unwrap();
    let cmd = v["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .unwrap();
    let out = Command::new("sh")
        .args(["-c", &cmd.replacen("livtk", env!("CARGO_BIN_EXE_livtk"), 1)])
        .current_dir(dir.path())
        .env("CLAUDECODE", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = fs::read_to_string(dir.path().join(".livtk/events.jsonl")).unwrap();
    assert!(log.contains(r#""by":"claude:refuter""#), "{log}");
}

// ---- format and check ---------------------------------------------------------------------------

#[test]
fn edited_files_are_formatted_when_configured() {
    let dir = project();
    with_hooks(
        dir.path(),
        "format = [\"touch\", \"{file}.formatted\"]\nformat_extensions = [\"rs\"]\n",
    );
    fs::create_dir(dir.path().join("src")).unwrap();
    let edit = |name: &str| {
        let file = dir.path().join("src").join(name);
        fs::write(&file, "x").unwrap();
        answer(
            "post-tool-use",
            &json!({ "tool_name": "Edit", "tool_input": { "file_path": file } }),
        )
    };
    assert_eq!(edit("main.rs"), None);
    assert!(dir.path().join("src/main.rs.formatted").exists());
    assert_eq!(edit("notes.md"), None);
    assert!(
        !dir.path().join("src/notes.md.formatted").exists(),
        "other extensions are left alone"
    );
}

#[test]
fn the_check_blocks_an_implementer_once() {
    let dir = project();
    with_hooks(
        dir.path(),
        "check = [\"sh\", \"-c\", \"echo boom; exit 1\"]\n",
    );
    let stop = |agent: &str, active: bool| {
        answer(
            "subagent-stop",
            &json!({
                "hook_event_name": "SubagentStop", "cwd": dir.path(),
                "agent_type": agent, "stop_hook_active": active,
            }),
        )
    };
    let v = stop("livtk:implementer", false).unwrap();
    assert_eq!(v["decision"], "block");
    assert!(v["reason"].as_str().unwrap().contains("boom"), "{v}");
    assert_eq!(
        stop("livtk:implementer", true),
        None,
        "never twice in a row"
    );
    assert_eq!(
        stop("livtk:refuter", false),
        None,
        "only the configured agents"
    );
}

#[test]
fn a_passing_check_lets_the_agent_stop() {
    let dir = project();
    with_hooks(dir.path(), "check = [\"true\"]\n");
    let input =
        json!({ "cwd": dir.path(), "agent_type": "livtk:implementer", "stop_hook_active": false });
    assert_eq!(answer("subagent-stop", &input), None);
}

// ---- speed --------------------------------------------------------------------------------------

/// `cargo test --release -p livtk-cli -- --ignored`: every hook under 20 ms.
#[test]
#[ignore = "a benchmark: run in release with --ignored"]
fn hooks_stay_under_20ms() {
    let dir = project();
    for i in 0..200 {
        livtk(dir.path(), false, &["new", &format!("task {i}")]);
    }
    let file = dir.path().join("src.rs");
    for (event, input) in [
        (
            "session-start",
            json!({ "cwd": dir.path(), "source": "startup" }),
        ),
        (
            "subagent-start",
            json!({ "cwd": dir.path(), "agent_type": "Explore" }),
        ),
        (
            "pre-tool-use",
            bash("livtk list && cargo test", Some("livtk:implementer")),
        ),
        (
            "post-tool-use",
            json!({ "tool_name": "Edit", "tool_input": { "file_path": file } }),
        ),
        (
            "subagent-stop",
            json!({ "cwd": dir.path(), "agent_type": "Explore" }),
        ),
    ] {
        let best = (0..10)
            .map(|_| {
                let t = Instant::now();
                hook(event, &input);
                t.elapsed()
            })
            .min()
            .unwrap();
        eprintln!("{event}: {best:?}");
        assert!(best < Duration::from_millis(20), "{event} took {best:?}");
    }
}
