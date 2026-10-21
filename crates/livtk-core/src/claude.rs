//! Claude Code's own files, read and never written: the transcripts of the sessions run in a
//! project. livtk lists them, reads them a page at a time, and matches its events to the calls
//! that wrote them. A transcript can run to tens of megabytes, so nothing here reads one whole
//! to list it.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::Serialize;
use serde_json::Value;

/// How much of a transcript's start and end the list reads: its first prompt and its title.
const EDGE: u64 = 256 * 1024;

/// Where Claude Code keeps the transcripts of the sessions run in `root`: its projects folder,
/// under the folder's path with `/` and `.` as `-`.
pub fn transcripts_dir(root: &Path) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(transcripts_in(&PathBuf::from(home).join(".claude"), root))
}

/// The same, under Claude Code's folder `claude_home` (`~/.claude`, or a test's).
pub fn transcripts_in(claude_home: &Path, root: &Path) -> PathBuf {
    let name: String = root
        .display()
        .to_string()
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect();
    claude_home.join("projects").join(name)
}

/// A session's transcript, as the list shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Transcript {
    pub session: String,
    pub path: PathBuf,
    /// The name it was given (`/rename`), if any.
    pub title: Option<String>,
    /// The first thing asked in it, for a session without a title.
    pub first_prompt: Option<String>,
    pub started: Option<String>,
    pub ended: Option<String>,
    pub bytes: u64,
    /// Its subagents, each in its own transcript beside it.
    pub subagents: Vec<Subagent>,
}

impl Transcript {
    /// What it is called: its title, else its first prompt's first line, else its id.
    pub fn name(&self) -> String {
        self.title
            .clone()
            .or_else(|| {
                self.first_prompt
                    .as_deref()
                    .and_then(|p| p.lines().find(|l| !l.trim().is_empty()))
                    .map(|l| l.chars().take(80).collect())
            })
            .unwrap_or_else(|| format!("Session {}", crate::format::session(&self.session)))
    }
}

/// A subagent's transcript, and the call in its session that started it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Subagent {
    pub id: String,
    pub path: PathBuf,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    /// The tool call that started it, in the parent's transcript.
    pub tool_use_id: Option<String>,
}

/// Every session's transcript in `dir`, the latest first.
pub fn transcripts(dir: &Path) -> Vec<Transcript> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Transcript> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|path| {
            let session = path.file_stem()?.to_str()?.to_owned();
            let bytes = std::fs::metadata(&path).ok()?.len();
            let (head, tail) = edges(&path, bytes)?;
            let mut t = Transcript {
                subagents: subagents(&dir.join(&session)),
                session,
                path,
                title: None,
                first_prompt: None,
                started: None,
                ended: None,
                bytes,
            };
            for v in head.iter().chain(&tail) {
                if let Some(at) = v["timestamp"].as_str() {
                    t.started.get_or_insert_with(|| at.to_owned());
                    t.ended = Some(at.to_owned());
                }
                if v["type"] == "custom-title"
                    && let Some(title) = v["customTitle"].as_str()
                {
                    t.title = Some(title.to_owned());
                }
                if t.first_prompt.is_none()
                    && let Some(Entry::Prompt { text, .. }) = entry(v).first()
                {
                    t.first_prompt = Some(text.clone());
                }
            }
            Some(t)
        })
        .collect();
    out.sort_by(|a, b| b.ended.cmp(&a.ended));
    out
}

/// The records at the start and at the end of a transcript: whole lines only.
fn edges(path: &Path, bytes: u64) -> Option<(Vec<Value>, Vec<Value>)> {
    let mut f = File::open(path).ok()?;
    let mut head = String::new();
    Read::by_ref(&mut f)
        .take(EDGE)
        .read_to_string(&mut head)
        .ok()?;
    let parse = |text: &str, skip_first: bool| -> Vec<Value> {
        text.lines()
            .skip(usize::from(skip_first))
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    };
    let head_records = parse(&head, false);
    if bytes <= EDGE {
        return Some((head_records, Vec::new()));
    }
    f.seek(SeekFrom::Start(bytes.saturating_sub(EDGE))).ok()?;
    let mut tail = Vec::new();
    f.read_to_end(&mut tail).ok()?;
    // The first line read from the middle is likely cut: skipped.
    Some((head_records, parse(&String::from_utf8_lossy(&tail), true)))
}

fn subagents(dir: &Path) -> Vec<Subagent> {
    let Ok(entries) = std::fs::read_dir(dir.join("subagents")) else {
        return Vec::new();
    };
    let mut out: Vec<Subagent> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?.to_owned();
            let id = stem.strip_prefix("agent-").unwrap_or(&stem).to_owned();
            let meta: Value = std::fs::read_to_string(path.with_extension("meta.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(Value::Null);
            let s = |k: &str| meta[k].as_str().map(str::to_owned);
            Some(Subagent {
                id,
                agent_type: s("agentType"),
                description: s("description"),
                tool_use_id: s("toolUseId"),
                path,
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// One thing in a session, as a person reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    /// What the person asked.
    Prompt { at: String, text: String },
    /// What Claude said.
    Reply { at: String, text: String },
    /// A tool Claude used: its name, a line saying what on, and its input in full.
    Tool {
        at: String,
        id: String,
        name: String,
        summary: String,
        input: String,
    },
    /// What a tool gave back, for the call with `id`.
    Result {
        at: String,
        id: String,
        text: String,
        error: bool,
    },
    /// The conversation was compacted here: what came before is summarised.
    Compacted { at: String },
}

impl Entry {
    pub fn at(&self) -> &str {
        match self {
            Entry::Prompt { at, .. }
            | Entry::Reply { at, .. }
            | Entry::Tool { at, .. }
            | Entry::Result { at, .. }
            | Entry::Compacted { at } => at,
        }
    }
}

/// A page of a transcript: its entries, and where the next page starts (`None` at the end).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub next: Option<u64>,
}

/// Up to `limit` entries of the transcript at `path`, from byte `from`.
pub fn read(path: &Path, from: u64, limit: usize) -> std::io::Result<Page> {
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(from))?;
    let mut reader = BufReader::new(f);
    let mut entries = Vec::new();
    let mut at = from;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(Page {
                entries,
                next: None,
            });
        }
        at += n as u64;
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            entries.extend(entry(&v));
        }
        if entries.len() >= limit {
            return Ok(Page {
                entries,
                next: Some(at),
            });
        }
    }
}

/// What a person reads of one record: none for the bookkeeping and the hidden parts (thinking,
/// injected context, attachments), several for an assistant turn with text and calls.
pub fn entry(v: &Value) -> Vec<Entry> {
    let at = v["timestamp"].as_str().unwrap_or_default().to_owned();
    match v["type"].as_str() {
        Some("system") if v["subtype"] == "compact_boundary" => vec![Entry::Compacted { at }],
        Some("user") if v["isMeta"] == true || v["isCompactSummary"] == true => Vec::new(),
        Some("user") => {
            let content = &v["message"]["content"];
            if let Some(text) = content.as_str() {
                // Text the harness put in a turn (`<task-notification>` and the like): not asked.
                return if text.trim_start().starts_with('<') {
                    Vec::new()
                } else {
                    vec![Entry::Prompt {
                        at,
                        text: text.to_owned(),
                    }]
                };
            }
            content
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| match b["type"].as_str()? {
                    "text" => {
                        let text = b["text"].as_str()?;
                        (!text.trim_start().starts_with('<')).then(|| Entry::Prompt {
                            at: at.clone(),
                            text: text.to_owned(),
                        })
                    }
                    "tool_result" => Some(Entry::Result {
                        at: at.clone(),
                        id: b["tool_use_id"].as_str()?.to_owned(),
                        text: result_text(&b["content"]),
                        error: b["is_error"] == true,
                    }),
                    _ => None,
                })
                .collect()
        }
        Some("assistant") => v["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| match b["type"].as_str()? {
                "text" => {
                    let text = b["text"].as_str()?;
                    (!text.trim().is_empty()).then(|| Entry::Reply {
                        at: at.clone(),
                        text: text.to_owned(),
                    })
                }
                "tool_use" => {
                    let name = b["name"].as_str()?.to_owned();
                    Some(Entry::Tool {
                        at: at.clone(),
                        id: b["id"].as_str()?.to_owned(),
                        summary: tool_summary(&name, &b["input"]),
                        input: serde_json::to_string_pretty(&b["input"]).unwrap_or_default(),
                        name,
                    })
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A tool call in one line: what it did it on, as each tool says it.
fn tool_summary(name: &str, input: &Value) -> String {
    let s = |k: &str| input[k].as_str().map(str::to_owned);
    let first_line = |t: String| {
        t.lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(120)
            .collect()
    };
    match name {
        "Bash" => s("description").or_else(|| s("command").map(first_line)),
        "Read" | "Edit" | "Write" | "NotebookEdit" => s("file_path").or_else(|| s("notebook_path")),
        "Grep" | "Glob" => s("pattern"),
        "Agent" | "Task" => s("description"),
        "WebFetch" => s("url"),
        "WebSearch" => s("query"),
        _ => None,
    }
    .unwrap_or_default()
}

/// A tool result's text: a string, or the text blocks of a list.
fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Every Bash call that ran `livtk` in the transcripts under `dir` (subagents' too, which name
/// their parent session), by time: what `livtk sessions claim` matches events to.
pub fn livtk_calls(dir: &Path, runs_livtk: impl Fn(&str) -> bool) -> Vec<(Timestamp, String)> {
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
            } else if path.extension().is_some_and(|x| x == "jsonl")
                && let Ok(f) = File::open(&path)
            {
                for line in BufReader::new(f).lines().map_while(Result::ok) {
                    // Cheap first: most lines are not a livtk call.
                    if !line.contains("livtk") || !line.contains("tool_use") {
                        continue;
                    }
                    let Ok(v) = serde_json::from_str::<Value>(&line) else {
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
                    let calls = v["message"]["content"].as_array().is_some_and(|c| {
                        c.iter().any(|b| {
                            b["type"] == "tool_use"
                                && b["name"] == "Bash"
                                && b["input"]["command"].as_str().is_some_and(&runs_livtk)
                        })
                    });
                    if calls {
                        out.push((at, session.to_owned()));
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// Each tool call's result, by the call's id: what a page of entries pairs up.
pub fn results(entries: &[Entry]) -> HashMap<&str, (&str, bool)> {
    entries
        .iter()
        .filter_map(|e| match e {
            Entry::Result {
                id, text, error, ..
            } => Some((id.as_str(), (text.as_str(), *error))),
            _ => None,
        })
        .collect()
}
