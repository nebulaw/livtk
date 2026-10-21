//! Reading Claude Code's transcripts: the list, a page of a session, its subagents.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::fs;

use livtk_core::claude::{Entry, read, transcripts};
use serde_json::json;

fn line(v: serde_json::Value) -> String {
    format!("{v}\n")
}

/// A session as Claude Code writes it: a prompt, an assistant turn with thinking, text and a
/// call, the call's result, a harness notice, a compaction, and its title at the end.
fn fixture(dir: &std::path::Path) {
    let s = "11111111-aaaa";
    let mut t = String::new();
    t += &line(
        json!({"type": "user", "timestamp": "2026-10-16T08:00:00.000Z", "sessionId": s,
        "message": {"role": "user", "content": "Plan the importer"}}),
    );
    t += &line(
        json!({"type": "assistant", "timestamp": "2026-10-16T08:00:05.000Z", "sessionId": s,
        "message": {"role": "assistant", "content": [
            {"type": "thinking", "thinking": "", "signature": "x"},
            {"type": "text", "text": "Looking at the board first."},
            {"type": "tool_use", "id": "toolu_1", "name": "Bash",
             "input": {"command": "livtk list", "description": "List the tasks"}}]}}),
    );
    t += &line(
        json!({"type": "user", "timestamp": "2026-10-16T08:00:06.000Z", "sessionId": s,
        "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "toolu_1", "content": "PLANNED\n  LIV-1", "is_error": false}]}}),
    );
    t += &line(
        json!({"type": "user", "timestamp": "2026-10-16T08:00:07.000Z", "sessionId": s, "isMeta": true,
        "message": {"role": "user", "content": "Continue from where you left off."}}),
    );
    t += &line(
        json!({"type": "user", "timestamp": "2026-10-16T08:00:08.000Z", "sessionId": s,
        "message": {"role": "user", "content": "<task-notification>done</task-notification>"}}),
    );
    t += &line(
        json!({"type": "system", "subtype": "compact_boundary", "timestamp": "2026-10-16T09:00:00.000Z", "sessionId": s}),
    );
    t += &line(
        json!({"type": "user", "timestamp": "2026-10-16T09:00:00.500Z", "sessionId": s, "isCompactSummary": true,
        "message": {"role": "user", "content": "This session is being continued…"}}),
    );
    t += &line(
        json!({"type": "assistant", "timestamp": "2026-10-16T09:00:02.000Z", "sessionId": s,
        "message": {"role": "assistant", "content": [{"type": "text", "text": "Done."}]}}),
    );
    t += &line(json!({"type": "custom-title", "customTitle": "Import", "sessionId": s}));
    fs::write(dir.join(format!("{s}.jsonl")), t).unwrap();
    // A subagent it started, beside it.
    let sub = dir.join(s).join("subagents");
    fs::create_dir_all(&sub).unwrap();
    fs::write(
        sub.join("agent-abc.jsonl"),
        line(
            json!({"type": "user", "timestamp": "2026-10-16T08:30:00.000Z",
        "isSidechain": true, "message": {"role": "user", "content": "Refute LIV-1"}}),
        ),
    )
    .unwrap();
    fs::write(sub.join("agent-abc.meta.json"),
        json!({"agentType": "livtk:refuter", "description": "Refute LIV-1", "toolUseId": "toolu_9"}).to_string()).unwrap();
    // An older one, untitled.
    fs::write(dir.join("22222222-bbbb.jsonl"), line(json!({"type": "user", "timestamp": "2026-10-15T08:00:00.000Z",
        "sessionId": "22222222-bbbb", "message": {"role": "user", "content": "Fix the build\nit fails on CI"}}))).unwrap();
}

#[test]
fn the_list_names_each_session_latest_first_with_its_subagents() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let all = transcripts(dir.path());
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].session, "11111111-aaaa", "the latest first");
    assert_eq!(all[0].name(), "Import", "its title");
    assert_eq!(all[0].started.as_deref(), Some("2026-10-16T08:00:00.000Z"));
    assert_eq!(all[0].ended.as_deref(), Some("2026-10-16T09:00:02.000Z"));
    assert_eq!(all[0].subagents.len(), 1);
    assert_eq!(
        all[0].subagents[0].agent_type.as_deref(),
        Some("livtk:refuter")
    );
    assert_eq!(all[0].subagents[0].tool_use_id.as_deref(), Some("toolu_9"));
    assert_eq!(
        all[1].name(),
        "Fix the build",
        "untitled: its first prompt's first line"
    );
}

#[test]
fn a_session_reads_as_a_person_reads_it_a_page_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let path = dir.path().join("11111111-aaaa.jsonl");
    let all = read(&path, 0, 100).unwrap();
    assert_eq!(all.next, None, "all of it");
    let kinds: Vec<&str> = all
        .entries
        .iter()
        .map(|e| match e {
            Entry::Prompt { .. } => "prompt",
            Entry::Reply { .. } => "reply",
            Entry::Tool { .. } => "tool",
            Entry::Result { .. } => "result",
            Entry::Compacted { .. } => "compacted",
        })
        .collect();
    // Thinking, the injected nudge, the harness's notice and the summary are not read.
    assert_eq!(
        kinds,
        ["prompt", "reply", "tool", "result", "compacted", "reply"]
    );
    match &all.entries[2] {
        Entry::Tool { name, summary, .. } => {
            assert_eq!(
                (name.as_str(), summary.as_str()),
                ("Bash", "List the tasks")
            );
        }
        other => panic!("{other:?}"),
    }
    // In pages: the second starts where the first stopped, and the two make the whole.
    let first = read(&path, 0, 3).unwrap();
    let next = first.next.expect("more to read");
    let rest = read(&path, next, 100).unwrap();
    let joined: Vec<Entry> = first.entries.into_iter().chain(rest.entries).collect();
    assert_eq!(joined, all.entries);
}

#[test]
fn reading_starts_a_little_before_a_time_without_reading_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let path = dir.path().join("11111111-aaaa.jsonl");
    // One record before the first at or after 08:00:06 (the result): the call's turn.
    let from = livtk_core::claude::offset_near(&path, "2026-10-16T08:00:06.000Z", 1).unwrap();
    let page = read(&path, from, 100).unwrap();
    assert!(
        matches!(page.entries[0], Entry::Reply { .. }),
        "{:?}",
        page.entries[0]
    );
    // Before everything: the start; after everything: a little before the end.
    assert_eq!(
        livtk_core::claude::offset_near(&path, "2000-01-01T00:00:00.000Z", 3).unwrap(),
        0
    );
    assert!(livtk_core::claude::offset_near(&path, "2999-01-01T00:00:00.000Z", 1).unwrap() > 0);
}
